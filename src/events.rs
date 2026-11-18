use crate::approvals::ApprovalRequest;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NotificationEvent {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub level: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum StreamEvent {
    Notify(NotificationEvent),
    ApprovalPending(ApprovalRequest),
    ApprovalDone { id: String },
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub struct EventBus {
    tx: broadcast::Sender<StreamEvent>,
    notifications: Mutex<VecDeque<NotificationEvent>>,
    last_client_ms: AtomicU64,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(64);
        Self {
            tx,
            notifications: Mutex::new(VecDeque::with_capacity(30)),
            last_client_ms: AtomicU64::new(0),
        }
    }

    pub fn touch_client(&self) {
        self.last_client_ms.store(now_ms(), Ordering::Relaxed);
    }

    pub fn is_client_active(&self, window_ms: u64) -> bool {
        let last = self.last_client_ms.load(Ordering::Relaxed);
        last > 0 && now_ms().saturating_sub(last) <= window_ms
    }

    pub fn emit(&self, ev: StreamEvent) {
        if let StreamEvent::Notify(ref note) = ev {
            if let Ok(mut queue) = self.notifications.lock() {
                if queue.len() >= 30 {
                    queue.pop_front();
                }
                queue.push_back(note.clone());
            }
        }
        let _ = self.tx.send(ev);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StreamEvent> {
        self.tx.subscribe()
    }

    pub fn recent_notifications(&self) -> Vec<NotificationEvent> {
        self.notifications
            .lock()
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recent_notifications_caps_at_30_and_tracks_client_liveness() {
        let bus = EventBus::new();
        assert!(!bus.is_client_active(8_000));
        bus.touch_client();
        assert!(bus.is_client_active(8_000));

        for i in 0..35 {
            bus.emit(StreamEvent::Notify(NotificationEvent {
                id: format!("n-{i}"),
                kind: "stop".to_string(),
                title: format!("Title {i}"),
                body: "Body".to_string(),
                level: "info".to_string(),
                created_at_ms: i as u64,
            }));
        }

        let recent = bus.recent_notifications();
        assert_eq!(recent.len(), 30);
        assert_eq!(recent.first().unwrap().id, "n-5");
        assert_eq!(recent.last().unwrap().id, "n-34");
    }
}
