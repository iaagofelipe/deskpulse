use crate::agenda::{AgendaEvent, AgendaSnapshot, AgendaStore};
use crate::approvals::{ApprovalOutcome, ApprovalRequest, ApprovalStore, Decision};
use crate::config::{
    normalize_theme, save_config, AiConfig, AppConfig, NightModeConfig, ShortcutConfig,
    WeatherConfig,
};
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
use crate::monitors::network::{NetworkMonitor, NetworkSnapshot};
use crate::monitors::ai::AiSnapshot;
use crate::monitors::weather::{WeatherMonitor, WeatherSnapshot};
use crate::monitors::github::{GithubMonitor, GithubSnapshot};
use crate::shortcuts::{ShortcutRunner, ShortcutTileState, ShortcutError};
use crate::web_assets::serve_web_asset;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::middleware;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use futures_util::stream::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::{broadcast, watch};

static INDEX_HTML: &str = include_str!("../web/index.html");
static ADMIN_HTML: &str = include_str!("../web/admin.html");
static MANIFEST_JSON: &str = include_str!("../assets/manifest.webmanifest");
static ICON_SVG: &str = include_str!("../assets/icon.svg");
static ICON_192_PNG: &[u8] = include_bytes!("../assets/icon-192.png");
static ICON_512_PNG: &[u8] = include_bytes!("../assets/icon-512.png");
static SW_JS: &str = include_str!("../assets/sw.js");

#[derive(Clone)]
pub struct AppState {
    pub token: String,
    pub port: u16,
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
    pub network: Arc<NetworkMonitor>,
    pub weather: Arc<WeatherMonitor>,
    pub github: Arc<GithubMonitor>,
    pub shortcuts: Arc<ShortcutRunner>,
    pub config_live: Arc<std::sync::RwLock<AppConfig>>,
    pub shutdown: watch::Receiver<bool>,
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
    pub network: NetworkSnapshot,
    pub ai: AiSnapshot,
    pub weather: WeatherSnapshot,
    pub github: GithubSnapshot,
    pub shortcuts: Vec<ShortcutTileState>,
    pub home_widgets: Vec<String>,
    pub night_mode: NightModeConfig,
    pub theme: String,
}

#[derive(Debug, Deserialize, Default)]
struct MediaActionBody {
    level: Option<i32>,
    value: Option<i32>,
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
        let media_snap = state.media.snapshot().await;
    let shortcuts_snap = state.shortcuts.tile_states(&media_snap);
    let cfg = state.config_live.read().unwrap().clone();
    DashboardState {
        pc: state.system.snapshot(),
        media: media_snap,
        adb: state.adb.status(),
        antigravity: state.antigravity.snapshot(),
        agenda: state.agenda.snapshot(),
        todos: state.todos.list(),
        notifications: state.events.recent_notifications(),
        approvals: state.approvals.snapshot(),
        remote_approval_enabled: state.remote_approval_enabled.load(Ordering::Relaxed),
        network: state.network.snapshot(),
        ai: state.antigravity.ai_snapshot(),
        weather: state.weather.snapshot(),
        github: state.github.snapshot(),
        shortcuts: shortcuts_snap,
        home_widgets: cfg.home_widgets,
        night_mode: cfg.night_mode,
        theme: crate::config::normalize_theme(&cfg.theme).to_string(),
    }
}

async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn admin_handler() -> Html<&'static str> {
    Html(ADMIN_HTML)
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
    let shutdown = state.shutdown.clone();
    let interval = tokio::time::interval(Duration::from_secs(1));

    let stream = futures_util::stream::unfold(
        (state, rx, shutdown, interval, true),
        |(state, mut rx, mut shutdown, mut interval, send_initial)| async move {
            if *shutdown.borrow() {
                return None;
            }

            if send_initial {
                interval.tick().await;
                if *shutdown.borrow() {
                    return None;
                }
                let snap = collect_dashboard_state(&state).await;
                let ev = Event::default()
                    .event("state")
                    .json_data(snap)
                    .unwrap_or_else(|_| Event::default().event("state").data("{}"));
                return Some((Ok(ev), (state, rx, shutdown, interval, false)));
            }

            loop {
                if *shutdown.borrow() {
                    return None;
                }
                tokio::select! {
                    biased;
                    res = shutdown.changed() => {
                        if res.is_err() || *shutdown.borrow() {
                            return None;
                        }
                    }
                    msg = rx.recv() => {
                        match msg {
                            Ok(StreamEvent::Notify(n)) => {
                                let ev = Event::default()
                                    .event("notify")
                                    .json_data(n)
                                    .unwrap_or_else(|_| Event::default().event("notify").data("{}"));
                                return Some((Ok(ev), (state, rx, shutdown, interval, false)));
                            }
                            Ok(StreamEvent::ApprovalPending(req)) => {
                                let ev = Event::default()
                                    .event("approval")
                                    .json_data(req)
                                    .unwrap_or_else(|_| Event::default().event("approval").data("{}"));
                                return Some((Ok(ev), (state, rx, shutdown, interval, false)));
                            }
                            Ok(StreamEvent::ApprovalDone { id }) => {
                                let ev = Event::default()
                                    .event("approval-done")
                                    .json_data(json!({ "id": id }))
                                    .unwrap_or_else(|_| Event::default().event("approval-done").data("{}"));
                                return Some((Ok(ev), (state, rx, shutdown, interval, false)));
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
                        return Some((Ok(ev), (state, rx, shutdown, interval, false)));
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

    let cmd = match parse_media_command(&action, parsed_body.level.or(parsed_body.value)) {
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
            crate::config::load_or_create(&state.config_path).unwrap_or_else(|_| state.config_live.read().unwrap().clone())
        } else {
            state.config_live.read().unwrap().clone()
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
        
        if let Ok(mut g) = state.config_live.write() {
            *g = cfg.clone();
        }

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

async fn assets_wildcard_handler(Path(path): Path<String>) -> Response {
    if let Some((content_type, body)) = serve_web_asset(&path) {
        let mut resp = static_asset(content_type, body);
        resp.headers_mut().insert(header::CACHE_CONTROL, header::HeaderValue::from_static("no-cache"));
        resp
    } else {
        (StatusCode::NOT_FOUND, "Not Found").into_response()
    }
}

async fn trigger_shortcut_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.shortcuts.trigger(&id, &state.media).await {
        Ok(opt_snap) => {
            let media = if let Some(s) = opt_snap {
                s
            } else {
                state.media.snapshot().await
            };
            (StatusCode::OK, Json(json!({ "ok": true, "media": media }))).into_response()
        }
        Err(ShortcutError::NotFound(_)) => json_error(StatusCode::NOT_FOUND, "Shortcut not found"),
        Err(ShortcutError::Invalid(msg)) => json_error(StatusCode::BAD_REQUEST, msg),
        Err(ShortcutError::Failed(msg)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, msg),
    }
}

async fn trigger_speedtest_handler(
    State(state): State<AppState>,
) -> Response {
    match state.network.trigger_speedtest() {
        Ok(snap) => (StatusCode::OK, Json(snap)).into_response(),
        Err(err) => {
            let status = if err.to_lowercase().contains("aguarde") || err.to_lowercase().contains("cooldown") {
                StatusCode::TOO_MANY_REQUESTS
            } else {
                StatusCode::BAD_REQUEST
            };
            json_error(status, err)
        }
    }
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
        json_error(
            StatusCode::NOT_IMPLEMENTED,
            "lock workstation is only supported on Windows",
        )
    }
}

const REDACTED_SECRET: &str = "__KEEP_EXISTING__";

fn mask_secret_preview(secret: &str) -> String {
    let trimmed = secret.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= 8 {
        return "••••••••".to_string();
    }
    let prefix: String = chars[..4].iter().collect();
    let suffix: String = chars[chars.len() - 4..].iter().collect();
    format!("{prefix}••••{suffix}")
}

fn build_admin_config_view(state: &AppState, cfg: &AppConfig) -> Value {
    let mut redacted = cfg.clone();
    redacted.token = REDACTED_SECRET.to_string();
    if redacted
        .github
        .token
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
    {
        redacted.github.token = Some(REDACTED_SECRET.to_string());
    }
    let raw_json = serde_json::to_string_pretty(&redacted).unwrap_or_else(|_| "{}".to_string());

    let github_token_preview = cfg
        .github
        .token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(mask_secret_preview);

    json!({
        "config_path": state.config_path.to_string_lossy().to_string(),
        "port": cfg.port,
        "approval_timeout_secs": cfg.approval_timeout_secs,
        "remote_approval_enabled": cfg.remote_approval_enabled,
        "calendar_ics_url": cfg.calendar_ics_url,
        "theme": normalize_theme(&cfg.theme),
        "auto_open_on_usb": cfg.auto_open_on_usb,
        "home_widgets": cfg.home_widgets,
        "night_mode": cfg.night_mode,
        "weather": cfg.weather,
        "github": {
            "username": cfg.github.username,
            "token_env": cfg.github.token_env,
            "token_set": github_token_preview.is_some(),
            "token_preview": github_token_preview,
        },
        "ai": cfg.ai,
        "shortcuts": cfg.shortcuts,
        "startup_enabled": crate::config::startup_status(),
        "raw_json": raw_json,
    })
}

#[derive(Debug, Deserialize)]
struct AdminGithubPatch {
    username: Option<String>,
    token_env: Option<String>,
    token_action: Option<String>,
    new_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AdminConfigPatch {
    theme: Option<String>,
    auto_open_on_usb: Option<bool>,
    remote_approval_enabled: Option<bool>,
    approval_timeout_secs: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_nullable")]
    calendar_ics_url: Option<Option<String>>,
    home_widgets: Option<Vec<String>>,
    night_mode: Option<NightModeConfig>,
    weather: Option<WeatherConfig>,
    github: Option<AdminGithubPatch>,
    ai: Option<AiConfig>,
    shortcuts: Option<Vec<ShortcutConfig>>,
}

#[derive(Debug, Deserialize)]
struct RawConfigBody {
    raw_json: String,
}

#[derive(Debug, Deserialize)]
struct StartupToggleBody {
    enabled: bool,
}

async fn apply_and_persist_config(state: &AppState, mut next_cfg: AppConfig) -> Result<AppConfig, String> {
    next_cfg.theme = normalize_theme(&next_cfg.theme).to_string();
    next_cfg.approval_timeout_secs = next_cfg.approval_timeout_secs.clamp(5, 300);
    next_cfg.night_mode.start_hour = next_cfg.night_mode.start_hour.min(23);
    next_cfg.night_mode.end_hour = next_cfg.night_mode.end_hour.min(23);
    next_cfg.shortcuts.truncate(16);
    if next_cfg.home_widgets.is_empty() {
        next_cfg.home_widgets = crate::config::default_home_widgets();
    }
    next_cfg.calendar_ics_url = next_cfg
        .calendar_ics_url
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    save_config(&state.config_path, &next_cfg)?;

    let ics_changed = {
        let mut ics_guard = state
            .calendar_ics_url
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        let changed = *ics_guard != next_cfg.calendar_ics_url;
        *ics_guard = next_cfg.calendar_ics_url.clone();
        changed
    };

    if let Ok(mut live) = state.config_live.write() {
        *live = next_cfg.clone();
    }

    state
        .remote_approval_enabled
        .store(next_cfg.remote_approval_enabled, Ordering::Relaxed);
    state.weather.update_config(next_cfg.weather.clone());
    state.github.update_config(next_cfg.github.clone());
    state.shortcuts.update_config(next_cfg.shortcuts.clone());
    state.antigravity.update_budgets(
        next_cfg.ai.budget_5h_tokens,
        next_cfg.ai.budget_weekly_tokens,
    );
    state.adb.set_auto_open(next_cfg.auto_open_on_usb);

    if ics_changed {
        state.agenda.set_ics_url(next_cfg.calendar_ics_url.clone());
        if next_cfg.calendar_ics_url.is_some() {
            let _ = state.agenda.refresh_now().await;
        }
    }

    Ok(next_cfg)
}

async fn get_admin_config_handler(State(state): State<AppState>) -> Response {
    let cfg = state.config_live.read().unwrap().clone();
    (StatusCode::OK, Json(build_admin_config_view(&state, &cfg))).into_response()
}

async fn put_admin_config_handler(
    State(state): State<AppState>,
    Json(patch): Json<AdminConfigPatch>,
) -> Response {
    let mut cfg = if state.config_path.exists() {
        crate::config::load_or_create(&state.config_path)
            .unwrap_or_else(|_| state.config_live.read().unwrap().clone())
    } else {
        state.config_live.read().unwrap().clone()
    };

    if let Some(theme) = patch.theme {
        cfg.theme = theme;
    }
    if let Some(auto_open) = patch.auto_open_on_usb {
        cfg.auto_open_on_usb = auto_open;
    }
    if let Some(remote) = patch.remote_approval_enabled {
        cfg.remote_approval_enabled = remote;
    }
    if let Some(timeout) = patch.approval_timeout_secs {
        cfg.approval_timeout_secs = timeout;
    }
    if let Some(ics_opt) = patch.calendar_ics_url {
        cfg.calendar_ics_url = ics_opt;
    }
    if let Some(widgets) = patch.home_widgets {
        cfg.home_widgets = widgets;
    }
    if let Some(nm) = patch.night_mode {
        cfg.night_mode = nm;
    }
    if let Some(w) = patch.weather {
        cfg.weather = w;
    }
    if let Some(gh) = patch.github {
        cfg.github.username = gh
            .username
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(env_name) = gh.token_env {
            let trimmed = env_name.trim();
            cfg.github.token_env = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }
        match gh.token_action.as_deref().unwrap_or("keep") {
            "clear" => cfg.github.token = None,
            "set" => {
                cfg.github.token = gh
                    .new_token
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
            }
            _ => {}
        }
    }
    if let Some(ai) = patch.ai {
        cfg.ai = ai;
    }
    if let Some(shortcuts) = patch.shortcuts {
        cfg.shortcuts = shortcuts;
    }

    match apply_and_persist_config(&state, cfg).await {
        Ok(saved) => (StatusCode::OK, Json(build_admin_config_view(&state, &saved))).into_response(),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err),
    }
}

async fn put_admin_config_raw_handler(
    State(state): State<AppState>,
    Json(body): Json<RawConfigBody>,
) -> Response {
    let current = state.config_live.read().unwrap().clone();
    let mut parsed: AppConfig = match serde_json::from_str(&body.raw_json) {
        Ok(c) => c,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, format!("JSON inválido: {err}")),
    };

    // Never allow locking out the active server port/token via raw edit
    parsed.port = current.port;
    if parsed.token.trim().is_empty() || parsed.token == REDACTED_SECRET {
        parsed.token = current.token.clone();
    }
    if parsed.github.token.as_deref() == Some(REDACTED_SECRET) {
        parsed.github.token = current.github.token.clone();
    }

    match apply_and_persist_config(&state, parsed).await {
        Ok(saved) => (StatusCode::OK, Json(build_admin_config_view(&state, &saved))).into_response(),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err),
    }
}

async fn admin_open_phone_handler(State(state): State<AppState>) -> Response {
    let Some(adb_path) = crate::monitors::adb::find_adb_path() else {
        return json_error(StatusCode::BAD_REQUEST, "adb.exe não encontrado no PC");
    };
    let port = state.port;
    let token = state.token.clone();
    let res = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let mut cmd = std::process::Command::new(&adb_path);
        cmd.arg("devices");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(crate::monitors::CREATE_NO_WINDOW);
        }
        let out = cmd.output().map_err(|e| format!("falha ao executar adb: {e}"))?;
        let devices = crate::monitors::adb::parse_adb_devices(&String::from_utf8_lossy(&out.stdout));
        let serial = devices
            .first()
            .cloned()
            .ok_or_else(|| "nenhum celular Android conectado via USB".to_string())?;
        let spec = format!("tcp:{port}");
        let mut rev = std::process::Command::new(&adb_path);
        rev.args(["-s", &serial, "reverse", &spec, &spec]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            rev.creation_flags(crate::monitors::CREATE_NO_WINDOW);
        }
        let _ = rev.output();

        let url = format!("http://127.0.0.1:{port}/#token={token}");
        let mut open = std::process::Command::new(&adb_path);
        open.args([
            "-s",
            &serial,
            "shell",
            "am",
            "start",
            "-a",
            "android.intent.action.VIEW",
            "-d",
            &url,
        ]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            open.creation_flags(crate::monitors::CREATE_NO_WINDOW);
        }
        let open_out = open
            .output()
            .map_err(|e| format!("falha ao abrir no celular: {e}"))?;
        if !open_out.status.success() {
            return Err(String::from_utf8_lossy(&open_out.stderr).trim().to_string());
        }
        Ok(serial)
    })
    .await;

    match res {
        Ok(Ok(device)) => (StatusCode::OK, Json(json!({ "ok": true, "device": device }))).into_response(),
        Ok(Err(err)) => json_error(StatusCode::BAD_REQUEST, err),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
}

async fn admin_restart_adb_handler() -> Response {
    let Some(adb_path) = crate::monitors::adb::find_adb_path() else {
        return json_error(StatusCode::BAD_REQUEST, "adb.exe não encontrado no PC");
    };
    let res = tokio::task::spawn_blocking(move || -> Result<(), String> {
        for sub in ["kill-server", "start-server"] {
            let mut cmd = std::process::Command::new(&adb_path);
            cmd.arg(sub);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(crate::monitors::CREATE_NO_WINDOW);
            }
            let _ = cmd.output();
            if sub == "kill-server" {
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        Ok(())
    })
    .await;

    match res {
        Ok(Ok(())) => (StatusCode::OK, Json(json!({ "ok": true }))).into_response(),
        Ok(Err(err)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
}

async fn admin_startup_handler(Json(body): Json<StartupToggleBody>) -> Response {
    let res = if body.enabled {
        crate::config::install_startup()
    } else {
        crate::config::uninstall_startup()
    };
    match res {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "ok": true, "startup_enabled": crate::config::startup_status() })),
        )
            .into_response(),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err),
    }
}

pub fn build_router(state: AppState) -> Router {
    let sec_state = SecurityState {
        port: state.port,
        token: state.token.clone(),
    };

    Router::new()
        .route("/", get(index_handler))
        .route("/admin", get(admin_handler))
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
        .route(
            "/api/config",
            get(get_admin_config_handler).put(put_admin_config_handler),
        )
        .route("/api/config/raw", put(put_admin_config_raw_handler))
        .route("/api/admin/open-phone", post(admin_open_phone_handler))
        .route("/api/admin/restart-adb", post(admin_restart_adb_handler))
        .route("/api/admin/startup", post(admin_startup_handler))
        .route("/api/system/lock", post(system_lock_handler))
        .route("/api/shortcuts/{id}/trigger", post(trigger_shortcut_handler))
        .route("/api/network/speedtest", post(trigger_speedtest_handler))
        .route("/assets/{*path}", get(assets_wildcard_handler))
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
            rx.clone(),
        ));
        let agenda = Arc::new(AgendaStore::new(
            dir.path().join("agenda_push.json"),
            None,
        ));
        let todos = Arc::new(TodoStore::open(dir.path().join("todos.json")).expect("todos"));
        let mock_media = Arc::new(MockMediaBackend::new());
        let mut initial_cfg = if dir.path().join("config.json").exists() {
            crate::config::load_or_create(&dir.path().join("config.json")).unwrap()
        } else {
            crate::config::AppConfig::default()
        };
        initial_cfg.github.token = Some("ghp_SUPER_SECRET_TEST_TOKEN_12345".to_string());
        if initial_cfg.shortcuts.is_empty() {
            initial_cfg.shortcuts.push(crate::config::ShortcutConfig {
                id: "mute_mic".to_string(),
                label: "Mute Mic".to_string(),
                icon: "mic".to_string(),
                color: "#000".to_string(),
                action: crate::config::ShortcutAction::ToggleMic,
            });
        }
        
        let config_live = Arc::new(std::sync::RwLock::new(initial_cfg.clone()));
        
        let network = Arc::new(crate::monitors::network::NetworkMonitor::spawn(dir.path().join("net.json"), rx.clone()));
        let weather = Arc::new(crate::monitors::weather::WeatherMonitor::spawn(initial_cfg.weather.clone(), rx.clone()));
        let github = Arc::new(crate::monitors::github::GithubMonitor::spawn(initial_cfg.github.clone(), rx.clone()));
        let shortcuts = Arc::new(crate::shortcuts::ShortcutRunner::new(initial_cfg.shortcuts.clone()));

        let state = AppState {
            token: "test-token-123".to_string(),
            port: 8765,
            config_path: dir.path().join("config.json"),
            events,
            approvals,
            system,
            adb,
            media: mock_media.clone(),
            antigravity,
            agenda,
            todos,
            remote_approval_enabled: Arc::new(std::sync::atomic::AtomicBool::new(initial_cfg.remote_approval_enabled)),
            calendar_ics_url: Arc::new(StdMutex::new(initial_cfg.calendar_ics_url.clone())),
            network,
            weather,
            github,
            shortcuts,
            config_live,
            shutdown: rx,
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

    #[tokio::test]
    async fn test_v2_api_state_includes_all_appliance_sections_and_never_leaks_github_token() {
        let (state, _, _, _dir) = make_test_state();
        let app = build_router(state);
        
        let req = Request::builder()
            .method("GET")
            .uri("/api/state")
            .header("Host", "127.0.0.1:8765")
            .header("X-DeskPulse-Token", "test-token-123")
            .body(Body::empty())
            .unwrap();
            
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        
        assert!(json.get("network").is_some());
        assert!(json.get("ai").is_some());
        assert!(json.get("weather").is_some());
        assert!(json.get("github").is_some());
        assert!(json.get("shortcuts").is_some());
        assert!(json.get("home_widgets").is_some());
        assert!(json.get("night_mode").is_some());
        
        let body_str = String::from_utf8_lossy(&body);
        assert!(!body_str.contains("ghp_SUPER_SECRET_TEST_TOKEN_12345"));
    }

    #[tokio::test]
    async fn test_shortcut_trigger_and_speedtest_endpoints_require_token_and_handle_unknown_id() {
        let (state, _, _, _dir) = make_test_state();
        let app = build_router(state);
        
        // 1. Unauthenticated -> 401
        let req = Request::builder()
            .method("POST")
            .uri("/api/shortcuts/mute_mic/trigger")
            .header("Host", "127.0.0.1:8765")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        
        let req = Request::builder()
            .method("POST")
            .uri("/api/network/speedtest")
            .header("Host", "127.0.0.1:8765")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        
        // 2. Unknown shortcut -> 404
        let req = Request::builder()
            .method("POST")
            .uri("/api/shortcuts/unknown/trigger")
            .header("Host", "127.0.0.1:8765")
            .header("X-DeskPulse-Token", "test-token-123")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        
        // 3. Valid shortcut (mute_mic) -> 200 (assume it exists in setup)
        let req = Request::builder()
            .method("POST")
            .uri("/api/shortcuts/mute_mic/trigger")
            .header("Host", "127.0.0.1:8765")
            .header("X-DeskPulse-Token", "test-token-123")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_modular_web_assets_served_with_correct_mime_and_guard() {
        let (state, _, _, _dir) = make_test_state();
        let app = build_router(state);
        
        // 1. Valid local request -> 200 OK + correct MIME
        let req = Request::builder()
            .method("GET")
            .uri("/assets/js/app.js")
            .header("Host", "127.0.0.1:8765")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers().get("content-type").unwrap(), "application/javascript; charset=utf-8");
        
        let req = Request::builder()
            .method("GET")
            .uri("/assets/css/tokens.css")
            .header("Host", "127.0.0.1:8765")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers().get("content-type").unwrap(), "text/css; charset=utf-8");
        
        // 2. Nonexistent -> 404
        let req = Request::builder()
            .method("GET")
            .uri("/assets/nonexistent.js")
            .header("Host", "127.0.0.1:8765")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        
        // 3. Foreign Host -> 403 Forbidden
        let req = Request::builder()
            .method("GET")
            .uri("/assets/js/app.js")
            .header("Host", "evil.com:8765")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_frontend_shell_contains_9_tabs_and_screensaver_zindex_hierarchy() {
        let index_html = include_str!("../web/index.html");
        assert!(index_html.contains("id=\"view-home\""));
        assert!(index_html.contains("id=\"view-pc\""));
        assert!(index_html.contains("id=\"view-ai\""));
        assert!(index_html.contains("id=\"view-agenda\""));
        assert!(index_html.contains("id=\"view-weather\""));
        assert!(index_html.contains("id=\"view-media\""));
        assert!(index_html.contains("id=\"view-github\""));
        assert!(index_html.contains("id=\"view-shortcuts\""));
        assert!(index_html.contains("id=\"view-timer\""));
        assert!(index_html.contains("id=\"screensaverOverlay\""));
        assert!(index_html.contains("id=\"approvalOverlay\""));

        let layout_css = include_str!("../web/css/layout.css");

        // Simple validation: Ensure approval has a higher z-index than screensaver
        // It's a crude regex but works for the test. We will parse the exact integers if needed.
        let screensaver_z = extract_z_index(layout_css, ".screensaver-overlay");
        let approval_z = extract_z_index(layout_css, ".approval-overlay");
        
        assert!(approval_z > screensaver_z, "approval z-index {} must be greater than screensaver z-index {}", approval_z, screensaver_z);
    }

    fn extract_z_index(css: &str, selector: &str) -> i32 {
        if let Some(idx) = css.find(selector) {
            let block_start = css[idx..].find('{').map(|i| idx + i).unwrap_or(idx);
            let block_end = css[block_start..].find('}').map(|i| block_start + i).unwrap_or(css.len());
            let block = &css[block_start..block_end];
            if let Some(z_idx) = block.find("z-index:") {
                let rest = &block[z_idx + 8..];
                let end = rest.find(';').unwrap_or(rest.len());
                let val_str = rest[..end].trim();
                return val_str.parse().unwrap_or(0);
            }
        }
        0
    }

    #[test]
    fn test_all_9_view_modules_export_render_function() {
        let view_names = [
            "home",
            "pc",
            "ai",
            "agenda",
            "weather",
            "media",
            "github",
            "shortcuts",
            "timer",
        ];

        for name in view_names {
            let path = format!("js/views/{name}.js");
            let (mime, bytes) = crate::web_assets::serve_web_asset(&path)
                .unwrap_or_else(|| panic!("embedded web asset missing: {path}"));
            assert!(
                mime.contains("javascript"),
                "{path} should have javascript MIME type, got {mime}"
            );
            assert!(
                bytes.len() > 200,
                "{path} must be a complete view module (> 200 bytes), got {} bytes",
                bytes.len()
            );
            let src = std::str::from_utf8(bytes)
                .unwrap_or_else(|e| panic!("{path} is not valid UTF-8: {e}"));
            assert!(
                src.contains("export function render"),
                "{path} must export function render"
            );
        }
    }

    #[tokio::test]
    async fn test_sse_stream_terminates_on_shutdown() {
        let (state, _media, tx, _dir) = make_test_state();
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/stream")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let mut body = resp.into_body();
        let first_frame = tokio::time::timeout(Duration::from_secs(2), body.frame())
            .await
            .expect("timed out waiting for initial SSE frame");
        let first_bytes = first_frame
            .expect("stream ended before initial frame")
            .expect("frame error")
            .into_data()
            .expect("expected data frame");
        let first_text = String::from_utf8_lossy(&first_bytes);
        assert!(
            first_text.contains("event: state"),
            "expected initial SSE state event, got: {first_text}"
        );

        let _ = tx.send(true);

        let drained = tokio::time::timeout(Duration::from_secs(2), async {
            while let Some(frame) = body.frame().await {
                frame.expect("unexpected body error during shutdown");
            }
        })
        .await;
        assert!(
            drained.is_ok(),
            "SSE stream did not terminate promptly after shutdown signal"
        );
    }

    #[tokio::test]
    async fn test_admin_page_and_config_endpoints_mask_secrets_and_persist() {
        let (state, _media, tx, _dir) = make_test_state();
        let expected_cfg_token = {
            let mut cfg = state.config_live.write().unwrap();
            cfg.token = "test-token-123".to_string();
            cfg.github.token = Some("ghp_super_secret_token_9876".to_string());
            save_config(&state.config_path, &cfg).unwrap();
            cfg.token.clone()
        };
        let app = build_router(state.clone());

        // GET /admin serves HTML without leaking token
        let html_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .header("Host", "127.0.0.1:8765")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(html_resp.status(), StatusCode::OK);
        let html_body = String::from_utf8(
            html_resp.into_body().collect().await.unwrap().to_bytes().to_vec(),
        )
        .unwrap();
        assert!(html_body.contains("DeskPulse Admin"));
        assert!(!html_body.contains("test-token-123"));
        assert!(!html_body.contains("ghp_super_secret_token_9876"));

        // GET /api/config requires token
        let unauth = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/config")
                    .header("Host", "127.0.0.1:8765")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

        // GET /api/config returns masked secret preview and redacted raw_json
        let cfg_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/config")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cfg_resp.status(), StatusCode::OK);
        let cfg_bytes = cfg_resp.into_body().collect().await.unwrap().to_bytes();
        let cfg_str = String::from_utf8_lossy(&cfg_bytes);
        assert!(!cfg_str.contains("ghp_super_secret_token_9876"));
        assert!(!cfg_str.contains("test-token-123"));
        let cfg_val: Value = serde_json::from_slice(&cfg_bytes).unwrap();
        assert_eq!(cfg_val["github"]["token_set"], true);
        assert_eq!(cfg_val["github"]["token_preview"], "ghp_••••9876");

        // PUT /api/config updates theme & city while preserving github token
        let put_body = json!({
            "theme": "amber",
            "auto_open_on_usb": false,
            "weather": { "city": "Fortaleza", "latitude": -3.73, "longitude": -38.52, "unit": "c" },
            "github": { "username": "devuser", "token_env": "GITHUB_TOKEN", "token_action": "keep", "new_token": null }
        });
        let put_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/config")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .header("Content-Type", "application/json")
                    .body(Body::from(put_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_resp.status(), StatusCode::OK);
        let saved = crate::config::load_or_create(&state.config_path).unwrap();
        assert_eq!(saved.theme, "amber");
        assert!(!saved.auto_open_on_usb);
        assert_eq!(saved.weather.city, "Fortaleza");
        assert_eq!(
            saved.github.token.as_deref(),
            Some("ghp_super_secret_token_9876"),
            "token_action=keep must preserve existing github token"
        );

        // PUT /api/config/raw preserves redacted secrets & rejects invalid JSON
        let bad_raw = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/config/raw")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .header("Content-Type", "application/json")
                    .body(Body::from(json!({ "raw_json": "{broken" }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad_raw.status(), StatusCode::BAD_REQUEST);

        let raw_text = cfg_val["raw_json"].as_str().unwrap().replace("midnight", "nord");
        let ok_raw = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/config/raw")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", "test-token-123")
                    .header("Content-Type", "application/json")
                    .body(Body::from(json!({ "raw_json": raw_text }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ok_raw.status(), StatusCode::OK);
        let saved2 = crate::config::load_or_create(&state.config_path).unwrap();
        assert_eq!(saved2.theme, "nord");
        assert_eq!(saved2.token, expected_cfg_token);
        assert_eq!(saved2.github.token.as_deref(), Some("ghp_super_secret_token_9876"));

        let _ = tx.send(true);
    }
}
