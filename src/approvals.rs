use crate::events::{now_ms, EventBus, StreamEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalInput {
    pub tool_name: String,
    pub title: String,
    pub summary: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub tool_name: String,
    pub title: String,
    pub summary: String,
    pub cwd: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ApprovalOutcome {
    pub decision: Decision,
    pub reason: Option<String>,
}

struct PendingEntry {
    request: ApprovalRequest,
    tx: oneshot::Sender<ApprovalOutcome>,
}

static NEXT_APPROVAL_ID: AtomicU64 = AtomicU64::new(1);

const DEFAULT_CLIENT_LIVENESS_MS: u64 = 8_000;

struct PendingGuard {
    id: String,
    pending: Arc<Mutex<HashMap<String, PendingEntry>>>,
    events: Arc<EventBus>,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        let removed = self
            .pending
            .lock()
            .ok()
            .and_then(|mut map| map.remove(&self.id))
            .is_some();
        if removed {
            self.events
                .emit(StreamEvent::ApprovalDone { id: self.id.clone() });
        }
    }
}

pub struct ApprovalStore {
    events: Arc<EventBus>,
    timeout: Duration,
    pending: Arc<Mutex<HashMap<String, PendingEntry>>>,
}

impl ApprovalStore {
    pub fn new(events: Arc<EventBus>, timeout: Duration) -> Self {
        Self {
            events,
            timeout,
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn ask(&self, input: ApprovalInput) -> ApprovalOutcome {
        if !self.events.is_client_active(DEFAULT_CLIENT_LIVENESS_MS) {
            return ApprovalOutcome {
                decision: Decision::Ask,
                reason: None,
            };
        }

        let seq = NEXT_APPROVAL_ID.fetch_add(1, Ordering::Relaxed);
        let created_at_ms = now_ms();
        let expires_at_ms = created_at_ms.saturating_add(self.timeout.as_millis() as u64);
        let id = format!("apr-{created_at_ms}-{seq}");

        let request = ApprovalRequest {
            id: id.clone(),
            tool_name: input.tool_name,
            title: input.title,
            summary: input.summary,
            cwd: input.cwd,
            created_at_ms,
            expires_at_ms,
        };

        let (tx, rx) = oneshot::channel();
        if let Ok(mut map) = self.pending.lock() {
            map.insert(
                id.clone(),
                PendingEntry {
                    request: request.clone(),
                    tx,
                },
            );
        }

        let _guard = PendingGuard {
            id,
            pending: Arc::clone(&self.pending),
            events: Arc::clone(&self.events),
        };

        self.events.emit(StreamEvent::ApprovalPending(request));

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(outcome)) => outcome,
            _ => ApprovalOutcome {
                decision: Decision::Ask,
                reason: None,
            },
        }
    }

    pub fn decide(&self, id: &str, outcome: ApprovalOutcome) -> bool {
        let entry = self
            .pending
            .lock()
            .ok()
            .and_then(|mut map| map.remove(id));
        let Some(entry) = entry else {
            return false;
        };
        let _ = entry.tx.send(outcome);
        self.events
            .emit(StreamEvent::ApprovalDone { id: id.to_string() });
        true
    }

    pub fn snapshot(&self) -> Vec<ApprovalRequest> {
        let mut items: Vec<ApprovalRequest> = self
            .pending
            .lock()
            .map(|map| map.values().map(|e| e.request.clone()).collect())
            .unwrap_or_default();
        items.sort_by_key(|r| r.created_at_ms);
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::StreamEvent;

    fn sample_input() -> ApprovalInput {
        ApprovalInput {
            tool_name: "run_command".to_string(),
            title: "Executar comando".to_string(),
            summary: "cargo test".to_string(),
            cwd: "D:\\Projetos\\deskpulse".to_string(),
        }
    }

    #[tokio::test]
    async fn test_returns_ask_immediately_when_no_client_active() {
        let events = Arc::new(EventBus::new());
        let store = ApprovalStore::new(events, Duration::from_secs(5));
        let start = tokio::time::Instant::now();
        let outcome = store.ask(sample_input()).await;
        assert_eq!(outcome.decision, Decision::Ask);
        assert!(start.elapsed() < Duration::from_millis(200));
        assert!(store.snapshot().is_empty());
    }

    #[tokio::test]
    async fn test_resolves_allow_when_client_decides_before_timeout() {
        let events = Arc::new(EventBus::new());
        events.touch_client();
        let mut rx = events.subscribe();
        let store = Arc::new(ApprovalStore::new(events, Duration::from_secs(5)));

        let store_clone = store.clone();
        let handle = tokio::spawn(async move { store_clone.ask(sample_input()).await });

        let req = tokio::time::timeout(Duration::from_millis(300), async {
            loop {
                if let Ok(StreamEvent::ApprovalPending(req)) = rx.recv().await {
                    break req;
                }
            }
        })
        .await
        .expect("timed out waiting for ApprovalPending");
        assert_eq!(store.snapshot().len(), 1);
        assert!(store.decide(
            &req.id,
            ApprovalOutcome {
                decision: Decision::Allow,
                reason: Some("ok".to_string()),
            }
        ));

        let outcome = handle.await.unwrap();
        assert_eq!(outcome.decision, Decision::Allow);
        assert_eq!(outcome.reason.as_deref(), Some("ok"));
        assert!(store.snapshot().is_empty());

        let done = rx.recv().await.unwrap();
        assert!(matches!(done, StreamEvent::ApprovalDone { id } if id == req.id));
    }

    #[tokio::test]
    async fn test_returns_ask_when_timeout_expires_and_emits_approval_done() {
        let events = Arc::new(EventBus::new());
        events.touch_client();
        let mut rx = events.subscribe();
        let store = ApprovalStore::new(events, Duration::from_millis(50));

        let outcome = store.ask(sample_input()).await;
        assert_eq!(outcome.decision, Decision::Ask);
        assert!(store.snapshot().is_empty());

        let mut saw_pending = None;
        let mut saw_done = None;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                StreamEvent::ApprovalPending(req) => saw_pending = Some(req.id),
                StreamEvent::ApprovalDone { id } => saw_done = Some(id),
                _ => {}
            }
        }
        assert!(saw_pending.is_some());
        assert_eq!(saw_pending, saw_done);
    }

    #[tokio::test]
    async fn test_cleans_up_pending_approval_when_caller_future_is_dropped() {
        let events = Arc::new(EventBus::new());
        events.touch_client();
        let mut rx = events.subscribe();
        let store = Arc::new(ApprovalStore::new(events, Duration::from_secs(5)));

        let store_clone = store.clone();
        let mut task = tokio::spawn(async move { store_clone.ask(sample_input()).await });

        let req_id = tokio::time::timeout(Duration::from_millis(300), async {
            loop {
                if let Ok(StreamEvent::ApprovalPending(req)) = rx.recv().await {
                    break req.id;
                }
            }
        })
        .await
        .expect("timed out waiting for ApprovalPending");
        assert_eq!(store.snapshot().len(), 1);

        task.abort();
        let _ = (&mut task).await;
        assert!(store.snapshot().is_empty());

        let done = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("approval_done event")
            .unwrap();
        assert!(matches!(done, StreamEvent::ApprovalDone { id } if id == req_id));
    }
}
