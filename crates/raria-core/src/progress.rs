// raria-core: Event bus for progress and status change notifications.
//
// Uses tokio broadcast channels to fan-out events to multiple subscribers
// (RPC WebSocket push, progress bars, logging, etc.).

use crate::job::{Gid, Status};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Events emitted by the download engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DownloadEvent {
    /// A job started downloading.
    Started { gid: Gid },
    /// A job was paused.
    Paused { gid: Gid },
    /// A job was stopped (removed or error).
    Stopped { gid: Gid },
    /// A job completed successfully.
    Complete { gid: Gid },
    /// A job encountered an error.
    Error { gid: Gid, message: String },
    /// A job's status changed.
    StatusChanged {
        gid: Gid,
        old_status: Status,
        new_status: Status,
    },
    /// Progress update for a job.
    Progress {
        gid: Gid,
        downloaded: u64,
        total: Option<u64>,
        speed: u64,
    },
}

/// Fan-out event bus for download engine events.
#[derive(Debug, Clone)]
pub struct EventBus {
    sender: broadcast::Sender<DownloadEvent>,
}

impl EventBus {
    /// Create a new event bus with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Publish an event to all subscribers.
    ///
    /// Returns the number of receivers that received the event.
    /// If there are no subscribers, the event is silently dropped.
    pub fn publish(&self, event: DownloadEvent) -> usize {
        // send() returns Err if no receivers, which is fine.
        self.sender.send(event).unwrap_or(0)
    }

    /// Subscribe to events. Returns a receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<DownloadEvent> {
        self.sender.subscribe()
    }

    /// Number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::Gid;

    #[test]
    fn new_bus_has_no_subscribers() {
        let bus = EventBus::default();
        assert_eq!(bus.subscriber_count(), 0);
    }

    #[test]
    fn publish_with_no_subscribers_does_not_panic() {
        let bus = EventBus::default();
        let count = bus.publish(DownloadEvent::Started {
            gid: Gid::from_raw(1),
        });
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn subscriber_receives_events() {
        let bus = EventBus::default();
        let mut rx = bus.subscribe();

        bus.publish(DownloadEvent::Started {
            gid: Gid::from_raw(1),
        });

        let event = rx.recv().await.unwrap();
        match event {
            DownloadEvent::Started { gid } => assert_eq!(gid, Gid::from_raw(1)),
            _ => panic!("unexpected event"),
        }
    }

    #[tokio::test]
    async fn multiple_subscribers_receive_same_event() {
        let bus = EventBus::default();
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        assert_eq!(bus.subscriber_count(), 2);

        bus.publish(DownloadEvent::Complete {
            gid: Gid::from_raw(5),
        });

        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();

        match (e1, e2) {
            (DownloadEvent::Complete { gid: g1 }, DownloadEvent::Complete { gid: g2 }) => {
                assert_eq!(g1, Gid::from_raw(5));
                assert_eq!(g2, Gid::from_raw(5));
            }
            _ => panic!("unexpected events"),
        }
    }

    #[tokio::test]
    async fn progress_event_carries_data() {
        let bus = EventBus::default();
        let mut rx = bus.subscribe();

        bus.publish(DownloadEvent::Progress {
            gid: Gid::from_raw(1),
            downloaded: 5000,
            total: Some(10000),
            speed: 1024,
        });

        let event = rx.recv().await.unwrap();
        match event {
            DownloadEvent::Progress {
                downloaded,
                total,
                speed,
                ..
            } => {
                assert_eq!(downloaded, 5000);
                assert_eq!(total, Some(10000));
                assert_eq!(speed, 1024);
            }
            _ => panic!("unexpected event"),
        }
    }

    #[test]
    fn event_serde_roundtrips() {
        let event = DownloadEvent::Error {
            gid: Gid::from_raw(42),
            message: "connection refused".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let recovered: DownloadEvent = serde_json::from_str(&json).unwrap();
        match recovered {
            DownloadEvent::Error { gid, message } => {
                assert_eq!(gid, Gid::from_raw(42));
                assert_eq!(message, "connection refused");
            }
            _ => panic!("wrong variant"),
        }
    }
}
