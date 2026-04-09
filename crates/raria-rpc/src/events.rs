// raria-rpc: WebSocket event emitter.
//
// Maps raria-core's DownloadEvent to aria2-compatible notification format
// and provides a subscription task that pushes events to WebSocket clients.
//
// aria2 notifications:
// - aria2.onDownloadStart
// - aria2.onDownloadPause
// - aria2.onDownloadStop
// - aria2.onDownloadComplete
// - aria2.onDownloadError

use raria_core::job::Gid;
use raria_core::progress::DownloadEvent;
use serde::{Deserialize, Serialize};

/// An aria2-compatible WebSocket notification.
///
/// Wire format per aria2 manual:
/// ```json
/// {"jsonrpc":"2.0","method":"aria2.onDownloadStart","params":[{"gid":"..."}]}
/// ```
///
/// Note: No `id` field — this is a JSON-RPC 2.0 notification.
/// Note: `params` is `[{"gid":"..."}]`, NOT `[[{"gid":"..."}]]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Aria2Notification {
    /// JSON-RPC version.
    pub jsonrpc: String,
    /// Notification method name (e.g., "aria2.onDownloadStart").
    pub method: String,
    /// Params: [{"gid": "..."}] — a single-element array containing a GID struct.
    pub params: Vec<GidParam>,
}

/// GID parameter in a notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GidParam {
    pub gid: String,
}

impl Aria2Notification {
    /// Create a new notification for a given method and GID.
    fn new(method: &str, gid: Gid) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params: vec![GidParam {
                gid: format!("{gid}"),
            }],
        }
    }
}

/// Convert a DownloadEvent to an aria2-compatible notification.
///
/// Returns `None` for events that don't have an aria2 equivalent (e.g. Progress).
pub fn event_to_notification(event: &DownloadEvent) -> Option<Aria2Notification> {
    match event {
        DownloadEvent::Started { gid } => {
            Some(Aria2Notification::new("aria2.onDownloadStart", *gid))
        }
        DownloadEvent::Paused { gid } => {
            Some(Aria2Notification::new("aria2.onDownloadPause", *gid))
        }
        DownloadEvent::Stopped { gid } => {
            Some(Aria2Notification::new("aria2.onDownloadStop", *gid))
        }
        DownloadEvent::Complete { gid } => {
            Some(Aria2Notification::new("aria2.onDownloadComplete", *gid))
        }
        DownloadEvent::Error { gid, .. } => {
            Some(Aria2Notification::new("aria2.onDownloadError", *gid))
        }
        // StatusChanged and Progress don't map to aria2 notifications.
        DownloadEvent::StatusChanged { .. } | DownloadEvent::Progress { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raria_core::job::Gid;
    use raria_core::progress::DownloadEvent;

    #[test]
    fn started_event_maps_to_on_download_start() {
        let event = DownloadEvent::Started {
            gid: Gid::from_raw(1),
        };
        let notif = event_to_notification(&event).unwrap();
        assert_eq!(notif.method, "aria2.onDownloadStart");
        assert_eq!(notif.jsonrpc, "2.0");
        assert_eq!(notif.params[0].gid, "0000000000000001");
    }

    #[test]
    fn paused_event_maps_to_on_download_pause() {
        let event = DownloadEvent::Paused {
            gid: Gid::from_raw(2),
        };
        let notif = event_to_notification(&event).unwrap();
        assert_eq!(notif.method, "aria2.onDownloadPause");
    }

    #[test]
    fn stopped_event_maps_to_on_download_stop() {
        let event = DownloadEvent::Stopped {
            gid: Gid::from_raw(3),
        };
        let notif = event_to_notification(&event).unwrap();
        assert_eq!(notif.method, "aria2.onDownloadStop");
    }

    #[test]
    fn complete_event_maps_to_on_download_complete() {
        let event = DownloadEvent::Complete {
            gid: Gid::from_raw(4),
        };
        let notif = event_to_notification(&event).unwrap();
        assert_eq!(notif.method, "aria2.onDownloadComplete");
    }

    #[test]
    fn error_event_maps_to_on_download_error() {
        let event = DownloadEvent::Error {
            gid: Gid::from_raw(5),
            message: "timeout".into(),
        };
        let notif = event_to_notification(&event).unwrap();
        assert_eq!(notif.method, "aria2.onDownloadError");
    }

    #[test]
    fn progress_event_returns_none() {
        let event = DownloadEvent::Progress {
            gid: Gid::from_raw(6),
            downloaded: 1000,
            total: Some(2000),
            speed: 500,
        };
        assert!(event_to_notification(&event).is_none());
    }

    #[test]
    fn notification_serde_roundtrips() {
        let notif = Aria2Notification::new("aria2.onDownloadStart", Gid::from_raw(1));
        let json = serde_json::to_string(&notif).unwrap();
        let recovered: Aria2Notification = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.method, "aria2.onDownloadStart");
        assert_eq!(recovered.params[0].gid, "0000000000000001");
    }

    #[test]
    fn notification_json_format_matches_aria2() {
        let notif = Aria2Notification::new("aria2.onDownloadComplete", Gid::from_raw(255));
        let json = serde_json::to_value(&notif).unwrap();

        // aria2 format: {"jsonrpc":"2.0","method":"...","params":[{"gid":"..."}]}
        assert_eq!(json["jsonrpc"], "2.0");
        assert!(json["params"].is_array());
        assert!(json["params"][0].is_object());
        assert_eq!(json["params"][0]["gid"], "00000000000000ff");
    }
}
