use crate::agenda::{AgendaEvent, AgendaSnapshot, AgendaStore};
use crate::approvals::{ApprovalOutcome, ApprovalRequest, ApprovalStore, Decision};
use crate::config::{save_config, AppConfig};
use crate::events::{now_ms, EventBus, NotificationEvent, StreamEvent};
use crate::hooks::{
    format_pre_tool_output, parse_pre_tool, parse_stop, PreToolAction,
};
use crate::monitors::adb::{AdbBridge, AdbStatus};
use crate::monitors::antigravity::{AntigravityMonitor, AntigravitySnapshot};
use crate::monitors::media::{parse_media_command, MediaBackend, MediaSnapshot};
use crate::monitors::system::{PcSnapshot, SystemMonitor};
use crate::security::{guard_middleware, SecurityState};
use crate::todos::{TodoItem, TodoStore};
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::middleware;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures_util::stream::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::broadcast;

static INDEX_HTML: &str = include_str!("../web/index.html");
static MANIFEST_JSON: &str = include_str!("../assets/manifest.webmanifest");
static ICON_SVG: &str = include_str!("../assets/icon.svg");
static ICON_192_PNG: &[u8] = include_bytes!("../assets/icon-192.png");
static ICON_512_PNG: &[u8] = include_bytes!("../assets/icon-512.png");
static SW_JS: &str = include_str!("../assets/sw.js");

#[derive(Clone)]
pub struct AppState {
    pub token: String,
    pub port: u16,
    pub approval_timeout_secs: u64,
    pub config_path: PathBuf,
    pub events: Arc<EventBus>,
    pub approvals: Arc<ApprovalStore>,
    pub system: Arc<SystemMonitor>,
    pub adb: Arc<AdbBridge>,
    pub media: Arc<dyn MediaBackend>,
    pub antigravity: Arc<AntigravityMonitor>,
    pub agenda: Arc<AgendaStore>,
    pub todos: Arc<TodoStore>,
    pub remote_approval_enabled: Arc<AtomicBool>,
    pub calendar_ics_url: Arc<StdMutex<Option<String>>>,
}

#[derive(Debug, Serialize)]
pub struct DashboardState {
    pub pc: Option<PcSnapshot>,
    pub media: MediaSnapshot,
    pub adb: AdbStatus,
    pub antigravity: AntigravitySnapshot,
    pub agenda: AgendaSnapshot,
    pub todos: Vec<TodoItem>,
    pub notifications: Vec<NotificationEvent>,
    pub approvals: Vec<ApprovalRequest>,
    pub remote_approval_enabled: bool,
}

#[derive(Debug, Deserialize, Default)]
struct MediaActionBody {
    level: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct ApprovalDecisionBody {
    decision: Decision,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AddTodoBody {
    text: String,
}

#[derive(Debug, Deserialize)]
struct PushAgendaBody {
    events: Vec<AgendaEvent>,
}

fn deserialize_optional_nullable<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize, Default)]
struct SettingsUpdateBody {
    remote_approval_enabled: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_nullable")]
    calendar_ics_url: Option<Option<String>>,
}

fn json_error(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({ "ok": false, "error": msg.into() }))).into_response()
}

fn static_asset(content_type: &'static str, body: &'static [u8]) -> Response {
    ([(header::CONTENT_TYPE, content_type)], body).into_response()
}

async fn collect_dashboard_state(state: &AppState) -> DashboardState {
    DashboardState {
        pc: state.system.snapshot(),
        media: state.media.snapshot().await,
        adb: state.adb.status(),
        antigravity: state.antigravity.snapshot(),
        agenda: state.agenda.snapshot(),
        todos: state.todos.list(),
        notifications: state.events.recent_notifications(),
        approvals: state.approvals.snapshot(),
        remote_approval_enabled: state.remote_approval_enabled.load(Ordering::Relaxed),
    }
}

async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn manifest_handler() -> Response {
    static_asset(
        "application/manifest+json; charset=utf-8",
        MANIFEST_JSON.as_bytes(),
    )
}

async fn icon_192_handler() -> Response {
    static_asset("image/png", ICON_192_PNG)
}

async fn icon_512_handler() -> Response {
    static_asset("image/png", ICON_512_PNG)
}

async fn icon_svg_handler() -> Response {
    static_asset("image/svg+xml; charset=utf-8", ICON_SVG.as_bytes())
}

async fn sw_handler() -> Response {
    (
        [
            (header::CONTENT_TYPE, "application/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        SW_JS.as_bytes(),
    )
        .into_response()
}

async fn state_handler(State(state): State<AppState>) -> Json<DashboardState> {
    Json(collect_dashboard_state(&state).await)
}

async fn stream_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    state.events.touch_client();
    let rx = state.events.subscribe();
    let interval = tokio::time::interval(Duration::from_secs(1));

    let stream = futures_util::stream::unfold(
        (state, rx, interval, true),
        |(state, mut rx, mut interval, send_initial)| async move {
            if send_initial {
                interval.tick().await;
                let snap = collect_dashboard_state(&state).await;
                let ev = Event::default()
                    .event("state")
                    .json_data(snap)
                    .unwrap_or_else(|_| Event::default().event("state").data("{}"));
                return Some((Ok(ev), (state, rx, interval, false)));
            }

            loop {
                tokio::select! {
                    biased;
                    msg = rx.recv() => {
                        match msg {
                            Ok(StreamEvent::Notify(n)) => {
                                let ev = Event::default()
                                    .event("notify")
                                    .json_data(n)
                                    .unwrap_or_else(|_| Event::default().event("notify").data("{}"));
                                return Some((Ok(ev), (state, rx, interval, false)));
                            }
                            Ok(StreamEvent::ApprovalPending(req)) => {
                                let ev = Event::default()
                                    .event("approval")
                                    .json_data(req)
                                    .unwrap_or_else(|_| Event::default().event("approval").data("{}"));
                                return Some((Ok(ev), (state, rx, interval, false)));
                            }
                            Ok(StreamEvent::ApprovalDone { id }) => {
                                let ev = Event::default()
                                    .event("approval-done")
                                    .json_data(json!({ "id": id }))
                                    .unwrap_or_else(|_| Event::default().event("approval-done").data("{}"));
                                return Some((Ok(ev), (state, rx, interval, false)));
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => return None,
                        }
                    }
                    _ = interval.tick() => {
                        state.events.touch_client();
                        let snap = collect_dashboard_state(&state).await;
                        let ev = Event::default()
                            .event("state")
                            .json_data(snap)
                            .unwrap_or_else(|_| Event::default().event("state").data("{}"));
                        return Some((Ok(ev), (state, rx, interval, false)));
                    }
                }
            }
        },
    );

    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn media_action_handler(
    State(state): State<AppState>,
    Path(action): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    let parsed_body: MediaActionBody = if body.is_empty() {
        MediaActionBody::default()
    } else {
        match serde_json::from_slice(&body) {
            Ok(b) => b,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("invalid json: {e}") })),
                )
                    .into_response();
            }
        }
    };

    let cmd = match parse_media_command(&action, parsed_body.level) {
        Ok(c) => c,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": err }))).into_response();
        }
    };

    match state.media.execute(cmd).await {
        Ok(snap) => (StatusCode::OK, Json(snap)).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": err })),
        )
            .into_response(),
    }
}

async fn pre_tool_hook_handler(State(state): State<AppState>, body: String) -> Json<Value> {
    let action = match parse_pre_tool(&body) {
        Ok(a) => a,
        Err(_) => return Json(format_pre_tool_output(Decision::Ask, None)),
    };

    match action {
        PreToolAction::Passthrough => Json(format_pre_tool_output(Decision::Ask, None)),
        PreToolAction::AskQuestion { summary } => {
            let created_at_ms = now_ms();
            state.events.emit(StreamEvent::Notify(NotificationEvent {
                id: format!("askq-{created_at_ms}"),
                kind: "ask_question".to_string(),
                title: "Antigravity fez uma pergunta".to_string(),
                body: summary,
                level: "info".to_string(),
                created_at_ms,
            }));
            Json(format_pre_tool_output(Decision::Ask, None))
        }
        PreToolAction::Approve(input) => {
            if !state.remote_approval_enabled.load(Ordering::Relaxed) {
                return Json(format_pre_tool_output(Decision::Ask, None));
            }
            let outcome = state.approvals.ask(input).await;
            Json(format_pre_tool_output(
                outcome.decision,
                outcome.reason.as_deref(),
            ))
        }
    }
}

async fn stop_hook_handler(State(state): State<AppState>, body: String) -> Json<Value> {
    let ev = parse_stop(&body);
    state.events.emit(StreamEvent::Notify(ev));
    Json(json!({}))
}

async fn approval_decide_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ApprovalDecisionBody>,
) -> Response {
    let ok = state.approvals.decide(
        &id,
        ApprovalOutcome {
            decision: body.decision,
            reason: body.reason,
        },
    );
    if ok {
        (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
    } else {
        json_error(StatusCode::NOT_FOUND, "approval not found or expired")
    }
}

async fn list_todos_handler(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "todos": state.todos.list() }))
}

async fn add_todo_handler(
    State(state): State<AppState>,
    Json(body): Json<AddTodoBody>,
) -> Response {
    if body.text.trim().is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "todo text cannot be empty");
    }
    match state.todos.add(&body.text) {
        Ok(item) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "item": item,
                "todos": state.todos.list(),
            })),
        )
            .into_response(),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err),
    }
}

async fn toggle_todo_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.todos.toggle(&id) {
        Ok(Some(item)) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "item": item,
                "todos": state.todos.list(),
            })),
        )
            .into_response(),
        Ok(None) => json_error(StatusCode::NOT_FOUND, "todo not found"),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err),
    }
}

async fn delete_todo_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.todos.remove(&id) {
        Ok(true) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "todos": state.todos.list(),
            })),
        )
            .into_response(),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "todo not found"),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err),
    }
}

async fn clear_done_todos_handler(State(state): State<AppState>) -> Response {
    match state.todos.clear_done() {
        Ok(cleared) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "cleared": cleared,
                "todos": state.todos.list(),
            })),
        )
            .into_response(),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err),
    }
}

async fn get_agenda_handler(State(state): State<AppState>) -> Json<AgendaSnapshot> {
    Json(state.agenda.snapshot())
}

async fn push_agenda_handler(
    State(state): State<AppState>,
    Json(body): Json<PushAgendaBody>,
) -> Response {
    match state.agenda.replace_pushed(body.events) {
        Ok(()) => (StatusCode::OK, Json(state.agenda.snapshot())).into_response(),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err),
    }
}

async fn refresh_agenda_handler(State(state): State<AppState>) -> Response {
    match state.agenda.refresh_now().await {
        Ok(snap) => (StatusCode::OK, Json(snap)).into_response(),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err),
    }
}

async fn get_settings_handler(State(state): State<AppState>) -> Json<Value> {
    let remote_approval_enabled = state.remote_approval_enabled.load(Ordering::Relaxed);
    let calendar_ics_url = state.calendar_ics_url.lock().ok().and_then(|g| g.clone());
    Json(json!({
        "remote_approval_enabled": remote_approval_enabled,
        "calendar_ics_url": calendar_ics_url,
    }))
}

async fn update_settings_handler(
    State(state): State<AppState>,
    Json(body): Json<SettingsUpdateBody>,
) -> Response {
    let (cfg, ics_changed, new_ics_url) = {
        let mut ics_guard = match state.calendar_ics_url.lock() {
            Ok(g) => g,
            Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "settings lock poisoned"),
        };

        let prev_remote = state.remote_approval_enabled.load(Ordering::Relaxed);
        let prev_ics = ics_guard.clone();

        let next_remote = body.remote_approval_enabled.unwrap_or(prev_remote);
        let mut ics_changed = false;
        let next_ics = match body.calendar_ics_url {
            Some(opt) => {
                let normalized = opt
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                if normalized != prev_ics {
                    ics_changed = true;
                }
                normalized
            }
            None => prev_ics,
        };

        let mut cfg = if state.config_path.exists() {
            crate::config::load_or_create(&state.config_path).unwrap_or_else(|_| AppConfig {
                port: state.port,
                token: state.token.clone(),
                approval_timeout_secs: state.approval_timeout_secs,
                ..AppConfig::default()
            })
        } else {
            AppConfig {
                port: state.port,
                token: state.token.clone(),
                approval_timeout_secs: state.approval_timeout_secs,
                ..AppConfig::default()
            }
        };
        cfg.calendar_ics_url = next_ics.clone();
        cfg.remote_approval_enabled = next_remote;

        if let Err(err) = save_config(&state.config_path, &cfg) {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, err);
        }

        state
            .remote_approval_enabled
            .store(next_remote, Ordering::Relaxed);
        *ics_guard = next_ics.clone();

        (cfg, ics_changed, next_ics)
    };

    if ics_changed {
        state.agenda.set_ics_url(new_ics_url.clone());
        if new_ics_url.is_some() {
            let _ = state.agenda.refresh_now().await;
        }
    }

    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "remote_approval_enabled": cfg.remote_approval_enabled,
            "calendar_ics_url": cfg.calendar_ics_url,
            "agenda": state.agenda.snapshot(),
        })),
    )
        .into_response()
}

async fn system_lock_handler() -> Response {
    #[cfg(windows)]
    {
        let mut cmd = tokio::process::Command::new("rundll32.exe");
        cmd.args(["user32.dll,LockWorkStation"]);
        cmd.creation_flags(crate::monitors::CREATE_NO_WINDOW);
        match cmd.spawn() {
            Ok(_) => (StatusCode::OK, Json(json!({ "ok": true }))).into_response(),
            Err(err) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to lock workstation: {err}"),
            ),
        }
    }
    #[cfg(not(windows))]
    {
        json_error(StatusCode::NOT_IMPLEMENTED, "lock workstation is Windows-only")
    }
}

pub fn build_router(state: AppState) -> Router {
    let sec_state = SecurityState {
        port: state.port,
        token: state.token.clone(),
    };

    Router::new()
        .route("/", get(index_handler))
        .route("/manifest.webmanifest", get(manifest_handler))
        .route("/icon-192.png", get(icon_192_handler))
        .route("/icon-512.png", get(icon_512_handler))
        .route("/icon.svg", get(icon_svg_handler))
        .route("/sw.js", get(sw_handler))
        .route("/api/state", get(state_handler))
        .route("/api/stream", get(stream_handler))
        .route("/api/media/{action}", post(media_action_handler))
        .route("/api/hooks/pre-tool", post(pre_tool_hook_handler))
        .route("/api/hooks/stop", post(stop_hook_handler))
        .route("/api/approvals/{id}", post(approval_decide_handler))
        .route("/api/todos", get(list_todos_handler).post(add_todo_handler))
        .route("/api/todos/clear-done", post(clear_done_todos_handler))
        .route("/api/todos/{id}/toggle", post(toggle_todo_handler))
        .route("/api/todos/{id}", delete(delete_todo_handler))
        .route("/api/agenda", get(get_agenda_handler).post(push_agenda_handler))
        .route("/api/agenda/refresh", post(refresh_agenda_handler))
        .route(
            "/api/settings",
            get(get_settings_handler).post(update_settings_handler),
        )
        .route("/api/system/lock", post(system_lock_handler))
        .with_state(state)
        .layer(middleware::from_fn_with_state(sec_state, guard_middleware))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::media::{MediaCommand, MediaSnapshot};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Mutex;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::sync::watch;
    use tower::ServiceExt;

    struct MockMediaBackend {
        snap: Mutex<MediaSnapshot>,
        commands: Mutex<Vec<MediaCommand>>,
    }

    impl MockMediaBackend {
        fn new() -> Self {
            Self {
                snap: Mutex::new(MediaSnapshot {
                    playing: true,
                    status: "Playing".to_string(),
                    title: "Track One".to_string(),
                    artist: "Artist".to_string(),
                    app: "Spotify".to_string(),
                    artwork_data_url: None,
                    volume_pct: 55,
                    muted: false,
                    ..MediaSnapshot::default()
                }),
                commands: Mutex::new(Vec::new()),
            }
        }
    }

    impl MediaBackend for MockMediaBackend {
        fn snapshot(&self) -> Pin<Box<dyn Future<Output = MediaSnapshot> + Send + '_>> {
            let s = self.snap.lock().unwrap().clone();
            Box::pin(async move { s })
        }

        fn execute(
            &self,
            cmd: MediaCommand,
        ) -> Pin<Box<dyn Future<Output = Result<MediaSnapshot, String>> + Send + '_>> {
            self.commands.lock().unwrap().push(cmd);
            let mut s = self.snap.lock().unwrap();
            match cmd {
                MediaCommand::ToggleMute => s.muted = !s.muted,
                MediaCommand::SetVolume(v) => s.volume_pct = v,
                _ => {}
            }
            let out = s.clone();
            Box::pin(async move { Ok(out) })
        }
    }

    fn make_test_state() -> (AppState, Arc<MockMediaBackend>, watch::Sender<bool>, TempDir) {
        let dir = TempDir::new().expect("tempdir");
        let (tx, rx) = watch::channel(false);
        let events = Arc::new(EventBus::new());
        let approvals = Arc::new(ApprovalStore::new(
            Arc::clone(&events),
            Duration::from_secs(5),
        ));
        let system = Arc::new(SystemMonitor::spawn(rx.clone()));
        let adb = Arc::new(AdbBridge::spawn(8765, rx.clone()));
        let antigravity = Arc::new(AntigravityMonitor::spawn_for_dir(
            dir.path().join("antigravity"),
            rx,
        ));
        let agenda = Arc::new(AgendaStore::new(
            dir.path().join("agenda_push.json"),
            None,
        ));
        let todos = Arc::new(TodoStore::open(dir.path().join("todos.json")).expect("todos"));
        let mock_media = Arc::new(MockMediaBackend::new());
        let state = AppState {
            token: "test-token-123".to_string(),
            port: 8765,
            approval_timeout_secs: 45,
            config_path: dir.path().join("config.json"),
            events,
            approvals,
            system,
            adb,
            media: mock_media.clone(),
            antigravity,
            agenda,
            todos,
            remote_approval_enabled: Arc::new(AtomicBool::new(false)),
            calendar_ics_url: Arc::new(StdMutex::new(None)),
        };
        (state, mock_media, tx, dir)
    }

    #[tokio::test]
    async fn test_api_state_requires_token_and_returns_pc_media_adb_payload() {
        let (state, _media, _tx, _dir) = make_test_state();
        let app = build_router(state);

        let unauth = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/state")
                    .header("Host", "127.0.0.1:8765")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

        let auth = app
            .oneshot(
                Request::builder()
                    .uri("/api/state")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(auth.status(), StatusCode::OK);
        let bytes = auth.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(val.get("pc").is_some());
        assert_eq!(val["media"]["title"], "Track One");
        assert!(val.get("adb").is_some());
        assert!(val.get("antigravity").is_some());
        assert!(val.get("agenda").is_some());
        assert!(val.get("todos").is_some());
        assert!(val.get("notifications").is_some());
        assert!(val.get("approvals").is_some());
        assert_eq!(val["remote_approval_enabled"], false);
    }

    #[tokio::test]
    async fn test_media_endpoint_executes_command_and_rejects_unknown_action() {
        let (state, media, _tx, _dir) = make_test_state();
        let app = build_router(state);

        let ok_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/media/toggle-mute")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ok_resp.status(), StatusCode::OK);
        let bytes = ok_resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["muted"], true);
        assert_eq!(media.commands.lock().unwrap().len(), 1);

        let bad_resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/media/unknown")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad_resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_pre_tool_hook_blocks_until_approval_endpoint_decides_allow() {
        let (state, _media, _tx, _dir) = make_test_state();
        state.remote_approval_enabled.store(true, Ordering::Relaxed);
        state.events.touch_client();
        let mut rx = state.events.subscribe();
        let app = build_router(state);

        let app_clone = app.clone();
        let hook_handle = tokio::spawn(async move {
            app_clone
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/hooks/pre-tool")
                        .header("Host", "127.0.0.1:8765")
                        .header("X-DeskPulse-Token", "test-token-123")
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            json!({"toolCall":{"name":"run_command","args":{"CommandLine":"cargo test"}}})
                                .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap()
        });

        let req_id = tokio::time::timeout(Duration::from_millis(300), async {
            loop {
                if let Ok(crate::events::StreamEvent::ApprovalPending(req)) = rx.recv().await {
                    break req.id;
                }
            }
        })
        .await
        .expect("ApprovalPending emitted");

        let decide_resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/approvals/{req_id}"))
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"decision":"allow"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(decide_resp.status(), StatusCode::OK);

        let hook_resp = hook_handle.await.unwrap();
        assert_eq!(hook_resp.status(), StatusCode::OK);
        let bytes = hook_resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["decision"], "allow");
    }

    #[tokio::test]
    async fn test_pre_tool_hook_returns_ask_immediately_when_no_sse_client_connected() {
        let (state, _media, _tx, _dir) = make_test_state();
        state.remote_approval_enabled.store(true, Ordering::Relaxed);
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/hooks/pre-tool")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        json!({"toolCall":{"name":"run_command","args":{"CommandLine":"echo hi"}}})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["decision"], "ask");
    }

    #[tokio::test]
    async fn test_root_serves_html_without_leaking_token() {
        let (state, _media, _tx, _dir) = make_test_state();
        let tok = state.token.clone();
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("Host", "127.0.0.1:8765")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ct.contains("text/html"));
        let body = String::from_utf8(resp.into_body().collect().await.unwrap().to_bytes().to_vec())
            .unwrap();
        assert!(!body.contains(&tok));
    }

    #[tokio::test]
    async fn test_pre_tool_hook_defaults_to_non_blocking_ask_and_respects_settings_toggle() {
        let (state, _media, _tx, _dir) = make_test_state();
        state.events.touch_client();
        let mut rx = state.events.subscribe();
        let cfg_path = state.config_path.clone();
        let app = build_router(state);

        // Default remote_approval_enabled=false must return ask immediately even with SSE client active
        let fast_resp = tokio::time::timeout(
            Duration::from_millis(200),
            app.clone().oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/hooks/pre-tool")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        json!({"toolCall":{"name":"run_command","args":{"CommandLine":"echo fast"}}})
                            .to_string(),
                    ))
                    .unwrap(),
            ),
        )
        .await
        .expect("pre-tool should not block when remote_approval_enabled is false")
        .unwrap();
        assert_eq!(fast_resp.status(), StatusCode::OK);
        let fast_val: Value =
            serde_json::from_slice(&fast_resp.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(fast_val["decision"], "ask");

        // Enable remote approval via POST /api/settings
        let settings_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/settings")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"remote_approval_enabled":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(settings_resp.status(), StatusCode::OK);
        let saved_cfg = crate::config::load_or_create(&cfg_path).expect("saved config");
        assert!(saved_cfg.remote_approval_enabled);

        // Now pre-tool should block until phone decides
        let app_clone = app.clone();
        let hook_handle = tokio::spawn(async move {
            app_clone
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/hooks/pre-tool")
                        .header("Host", "127.0.0.1:8765")
                        .header("X-DeskPulse-Token", "test-token-123")
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            json!({"toolCall":{"name":"run_command","args":{"CommandLine":"cargo check"}}})
                                .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap()
        });

        let req_id = tokio::time::timeout(Duration::from_millis(300), async {
            loop {
                if let Ok(crate::events::StreamEvent::ApprovalPending(req)) = rx.recv().await {
                    break req.id;
                }
            }
        })
        .await
        .expect("ApprovalPending emitted after opt-in");

        let decide_resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/approvals/{req_id}"))
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"decision":"allow"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(decide_resp.status(), StatusCode::OK);

        let hook_resp = hook_handle.await.unwrap();
        let val: Value =
            serde_json::from_slice(&hook_resp.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(val["decision"], "allow");
    }

    #[tokio::test]
    async fn test_pwa_assets_accessible_without_token_while_api_remains_protected() {
        let (state, _media, _tx, _dir) = make_test_state();
        let app = build_router(state);

        let public_assets = [
            ("/manifest.webmanifest", "manifest"),
            ("/icon-192.png", "image/png"),
            ("/icon-512.png", "image/png"),
            ("/icon.svg", "image/svg+xml"),
            ("/sw.js", "javascript"),
        ];

        for (path, expected_ct) in public_assets {
            let ok_resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header("Host", "127.0.0.1:8765")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(ok_resp.status(), StatusCode::OK, "GET {path} should be 200");
            let ct = ok_resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            assert!(ct.contains(expected_ct), "{path} content-type was {ct}");
            let body = ok_resp.into_body().collect().await.unwrap().to_bytes();
            if path.ends_with(".png") {
                assert!(body.starts_with(b"\x89PNG\r\n\x1a\n"), "{path} must be valid PNG");
            } else if path == "/manifest.webmanifest" {
                let manifest: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(manifest["display"], "fullscreen");
                assert_eq!(manifest["orientation"], "landscape");
                assert_eq!(manifest["start_url"], "/");
            }

            let foreign_resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header("Host", "evil.com:8765")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                foreign_resp.status(),
                StatusCode::FORBIDDEN,
                "GET {path} with foreign Host must be 403"
            );
        }

        let unauth_api = app
            .oneshot(
                Request::builder()
                    .uri("/api/todos")
                    .header("Host", "127.0.0.1:8765")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauth_api.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_todos_and_agenda_endpoints_mutate_dashboard_state() {
        let (state, _media, _tx, _dir) = make_test_state();
        let app = build_router(state);

        // Add todo
        let add_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/todos")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"text":"Revisar PR do Hub"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(add_resp.status(), StatusCode::OK);
        let add_val: Value =
            serde_json::from_slice(&add_resp.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        let todo_id = add_val["item"]["id"].as_str().expect("todo id").to_string();

        // Toggle todo
        let toggle_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/todos/{todo_id}/toggle"))
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(toggle_resp.status(), StatusCode::OK);

        // Push today's agenda event
        let today = chrono::Local::now().date_naive();
        let start_iso = format!("{}T15:00:00", today.format("%Y-%m-%d"));
        let agenda_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/agenda")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        json!({
                            "events": [{
                                "title": "Daily Standup",
                                "start_iso": start_iso
                            }]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(agenda_resp.status(), StatusCode::OK);

        // Verify reflected in /api/state
        let state_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/state")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(state_resp.status(), StatusCode::OK);
        let state_val: Value =
            serde_json::from_slice(&state_resp.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(state_val["todos"][0]["text"], "Revisar PR do Hub");
        assert_eq!(state_val["todos"][0]["done"], true);
        assert_eq!(state_val["agenda"]["events"][0]["title"], "Daily Standup");
        assert!(state_val.get("antigravity").is_some());
        assert_eq!(state_val["remote_approval_enabled"], false);

        // Clear done removes completed todo
        let clear_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/todos/clear-done")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(clear_resp.status(), StatusCode::OK);

        // Add another and delete via DELETE /api/todos/{id}
        let add_resp2 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/todos")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"text":"Segunda tarefa"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let add_val2: Value =
            serde_json::from_slice(&add_resp2.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        let todo_id2 = add_val2["item"]["id"].as_str().unwrap().to_string();

        let del_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/todos/{todo_id2}"))
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::OK);

        let state_after = app
            .oneshot(
                Request::builder()
                    .uri("/api/state")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let after_val: Value =
            serde_json::from_slice(&state_after.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(after_val["todos"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_settings_and_agenda_refresh_endpoints() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let (state, _media, _tx, _dir) = make_test_state();
        let cfg_path = state.config_path.clone();
        let app = build_router(state);

        // Initial GET /api/settings
        let initial_get = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/settings")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(initial_get.status(), StatusCode::OK);
        let initial_val: Value =
            serde_json::from_slice(&initial_get.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(initial_val["remote_approval_enabled"], false);
        assert!(initial_val["calendar_ics_url"].is_null());

        // Local mock ICS server serving 2 sequential valid responses
        let today_yyyymmdd = chrono::Local::now().date_naive().format("%Y%m%d").to_string();
        let ics_body = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
             BEGIN:VEVENT\r\nUID:ics-test-1\r\nDTSTART;VALUE=DATE:{today_yyyymmdd}\r\nSUMMARY:ICS Synced Event\r\nEND:VEVENT\r\n\
             END:VCALENDAR\r\n"
        );
        let ok_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ok_addr = ok_listener.local_addr().unwrap();
        let ok_server = tokio::spawn(async move {
            for _ in 0..2 {
                if let Ok((mut sock, _)) = ok_listener.accept().await {
                    let mut buf = [0u8; 1024];
                    let _ = sock.read(&mut buf).await;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/calendar; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        ics_body.len(),
                        ics_body
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                }
            }
        });

        let ok_url = format!("http://{ok_addr}/cal.ics");
        let set_ok_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/settings")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .header("Content-Type", "application/json")
                    .body(Body::from(json!({ "calendar_ics_url": ok_url }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(set_ok_resp.status(), StatusCode::OK);
        let set_ok_val: Value =
            serde_json::from_slice(&set_ok_resp.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(set_ok_val["ok"], true);
        assert_eq!(set_ok_val["calendar_ics_url"], ok_url);
        assert_eq!(set_ok_val["agenda"]["ics_configured"], true);
        assert!(set_ok_val["agenda"]["last_sync_ms"].is_u64());
        assert!(set_ok_val["agenda"]["error"].is_null());
        assert_eq!(
            set_ok_val["agenda"]["events"][0]["title"],
            "ICS Synced Event"
        );

        // GET /api/settings reflects updated URL
        let get_after = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/settings")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_after.status(), StatusCode::OK);
        let get_after_val: Value =
            serde_json::from_slice(&get_after.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(get_after_val["calendar_ics_url"], ok_url);

        // POST /api/agenda/refresh succeeds
        let refresh_ok = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/agenda/refresh")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(refresh_ok.status(), StatusCode::OK);
        let refresh_ok_val: Value =
            serde_json::from_slice(&refresh_ok.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert!(refresh_ok_val["last_sync_ms"].is_u64());
        assert!(refresh_ok_val["error"].is_null());
        let _ = ok_server.await;

        // Closed local socket fails immediately in ms without DNS/network
        let closed_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed_addr = closed_listener.local_addr().unwrap();
        let closed_server = tokio::spawn(async move {
            for _ in 0..2 {
                if let Ok((sock, _)) = closed_listener.accept().await {
                    drop(sock);
                }
            }
        });

        let broken_url = format!("http://{closed_addr}/broken.ics");
        let set_broken_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/settings")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        json!({ "calendar_ics_url": broken_url }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(set_broken_resp.status(), StatusCode::OK);
        let set_broken_val: Value = serde_json::from_slice(
            &set_broken_resp.into_body().collect().await.unwrap().to_bytes(),
        )
        .unwrap();
        assert_eq!(set_broken_val["ok"], true);
        assert_eq!(set_broken_val["calendar_ics_url"], broken_url);
        assert_eq!(set_broken_val["agenda"]["ics_configured"], true);
        assert!(
            set_broken_val["agenda"]["last_sync_ms"].is_null(),
            "last_sync_ms must be cleared when calendar_ics_url changes"
        );
        assert!(
            set_broken_val["agenda"]["error"].is_string(),
            "refresh failure should record agenda error"
        );
        assert_eq!(
            set_broken_val["agenda"]["events"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        let saved_cfg = crate::config::load_or_create(&cfg_path).expect("saved config");
        assert_eq!(saved_cfg.calendar_ics_url.as_deref(), Some(broken_url.as_str()));

        // POST /api/agenda/refresh returns 500 with error details when feed fails
        let refresh_err = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/agenda/refresh")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(refresh_err.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let refresh_err_val: Value =
            serde_json::from_slice(&refresh_err.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(refresh_err_val["ok"], false);
        assert!(refresh_err_val["error"].is_string());

        let _ = closed_server.await;
    }

    #[tokio::test]
    async fn test_update_settings_handler_preserves_v2_config_fields() {
        let (state, _media, _tx, _dir) = make_test_state();
        let cfg_path = state.config_path.clone();

        // 1. Write a config with custom v2 fields
        let mut custom_cfg = AppConfig::default();
        custom_cfg.weather.city = "Campinas".to_string();
        custom_cfg.github.username = Some("octocat".to_string());
        custom_cfg.ai.budget_5h_tokens = Some(50_000);
        custom_cfg.night_mode.start_hour = 22;
        custom_cfg.shortcuts.truncate(3);
        crate::config::save_config(&cfg_path, &custom_cfg).unwrap();

        let app = build_router(state);

        // 2. Call POST /api/settings to update only remote_approval_enabled
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/settings")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"remote_approval_enabled":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 3. Reload config from disk and assert all v2 fields were preserved
        let reloaded = crate::config::load_or_create(&cfg_path).unwrap();
        assert!(reloaded.remote_approval_enabled);
        assert_eq!(reloaded.weather.city, "Campinas");
        assert_eq!(reloaded.github.username, Some("octocat".to_string()));
        assert_eq!(reloaded.ai.budget_5h_tokens, Some(50_000));
        assert_eq!(reloaded.night_mode.start_hour, 22);
        assert_eq!(reloaded.shortcuts.len(), 3);
    }
}
