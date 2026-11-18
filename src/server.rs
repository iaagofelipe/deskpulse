use crate::approvals::{ApprovalOutcome, ApprovalRequest, ApprovalStore, Decision};
use crate::events::{now_ms, EventBus, NotificationEvent, StreamEvent};
use crate::hooks::{
    format_pre_tool_output, parse_pre_tool, parse_stop, PreToolAction,
};
use crate::monitors::adb::{AdbBridge, AdbStatus};
use crate::monitors::media::{parse_media_command, MediaBackend, MediaSnapshot};
use crate::monitors::system::{PcSnapshot, SystemMonitor};
use crate::security::{guard_middleware, SecurityState};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

static INDEX_HTML: &str = include_str!("../web/index.html");

#[derive(Clone)]
pub struct AppState {
    pub token: String,
    pub port: u16,
    pub events: Arc<EventBus>,
    pub approvals: Arc<ApprovalStore>,
    pub system: Arc<SystemMonitor>,
    pub adb: Arc<AdbBridge>,
    pub media: Arc<dyn MediaBackend>,
}

#[derive(Debug, Serialize)]
pub struct DashboardState {
    pub pc: Option<PcSnapshot>,
    pub media: MediaSnapshot,
    pub adb: AdbStatus,
    pub notifications: Vec<NotificationEvent>,
    pub approvals: Vec<ApprovalRequest>,
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

async fn collect_dashboard_state(state: &AppState) -> DashboardState {
    DashboardState {
        pc: state.system.snapshot(),
        media: state.media.snapshot().await,
        adb: state.adb.status(),
        notifications: state.events.recent_notifications(),
        approvals: state.approvals.snapshot(),
    }
}

async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
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
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": "approval not found or expired" })),
        )
            .into_response()
    }
}

pub fn build_router(state: AppState) -> Router {
    let sec_state = SecurityState {
        port: state.port,
        token: state.token.clone(),
    };

    Router::new()
        .route("/", get(index_handler))
        .route("/api/state", get(state_handler))
        .route("/api/stream", get(stream_handler))
        .route("/api/media/{action}", post(media_action_handler))
        .route("/api/hooks/pre-tool", post(pre_tool_hook_handler))
        .route("/api/hooks/stop", post(stop_hook_handler))
        .route("/api/approvals/{id}", post(approval_decide_handler))
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

    fn make_test_state() -> (AppState, Arc<MockMediaBackend>, watch::Sender<bool>) {
        let (tx, rx) = watch::channel(false);
        let events = Arc::new(EventBus::new());
        let approvals = Arc::new(ApprovalStore::new(
            Arc::clone(&events),
            Duration::from_secs(5),
        ));
        let system = Arc::new(SystemMonitor::spawn(rx.clone()));
        let adb = Arc::new(AdbBridge::spawn(8765, rx));
        let mock_media = Arc::new(MockMediaBackend::new());
        let state = AppState {
            token: "test-token-123".to_string(),
            port: 8765,
            events,
            approvals,
            system,
            adb,
            media: mock_media.clone(),
        };
        (state, mock_media, tx)
    }

    #[tokio::test]
    async fn test_api_state_requires_token_and_returns_pc_media_adb_payload() {
        let (state, _media, _tx) = make_test_state();
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
        assert!(val.get("notifications").is_some());
        assert!(val.get("approvals").is_some());
    }

    #[tokio::test]
    async fn test_media_endpoint_executes_command_and_rejects_unknown_action() {
        let (state, media, _tx) = make_test_state();
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
        let (state, _media, _tx) = make_test_state();
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
        let (state, _media, _tx) = make_test_state();
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
        let (state, _media, _tx) = make_test_state();
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
}
