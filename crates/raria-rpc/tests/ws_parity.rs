// WebSocket notification parity tests.
//
// These tests verify that raria's WebSocket notifications match aria2 1.37.0's
// exact wire format. The aria2 manual specifies:
//
// - Notifications are JSON-RPC 2.0 objects WITHOUT an "id" field
// - params is an array containing a single struct: [{"gid": "..."}]
// - Method names: aria2.onDownloadStart, aria2.onDownloadPause,
//   aria2.onDownloadStop, aria2.onDownloadComplete, aria2.onDownloadError,
//   aria2.onBtDownloadComplete
//
// Reference: https://aria2.github.io/manual/en/html/aria2c.html#notifications

#[cfg(test)]
mod tests {
    use raria_core::job::Gid;
    use raria_core::progress::DownloadEvent;
    use raria_rpc::events::event_to_notification;

    /// aria2 notification params structure is [{"gid":"..."}], NOT [[{"gid":"..."}]].
    /// This is the most critical format check — wrong nesting breaks all aria2 clients.
    #[test]
    fn notification_params_is_flat_array_of_one_object() {
        let notif = event_to_notification(&DownloadEvent::Started {
            gid: Gid::from_raw(0x2089b05ecca3d829),
        })
        .unwrap();

        let json = serde_json::to_value(&notif).unwrap();

        // aria2 wire format: "params":[{"gid":"2089b05ecca3d829"}]
        let params = json["params"].as_array().expect("params must be an array");
        assert_eq!(params.len(), 1, "params must contain exactly one element");
        assert!(
            params[0].is_object(),
            "params[0] must be an object, not an array. \
             aria2 sends [{{gid}}], not [[{{gid}}]]"
        );
        assert_eq!(params[0]["gid"], "2089b05ecca3d829");
    }

    /// aria2 notifications MUST NOT contain an "id" field.
    /// JSON-RPC 2.0 spec: a notification is a request without "id".
    #[test]
    fn notification_has_no_id_field() {
        let notif = event_to_notification(&DownloadEvent::Complete {
            gid: Gid::from_raw(1),
        })
        .unwrap();

        let json = serde_json::to_value(&notif).unwrap();
        let obj = json.as_object().unwrap();

        assert!(
            !obj.contains_key("id"),
            "aria2 notifications must NOT contain an 'id' field. \
             Per JSON-RPC 2.0: a notification is a request object without an 'id' member."
        );
    }

    /// Verify the exact JSON structure matches aria2's documented format.
    #[test]
    fn notification_exact_json_wire_format() {
        let notif = event_to_notification(&DownloadEvent::Started {
            gid: Gid::from_raw(0x0a0b0c0d0e0f0001),
        })
        .unwrap();

        let json = serde_json::to_value(&notif).unwrap();

        // Must have exactly these top-level keys: jsonrpc, method, params
        let obj = json.as_object().unwrap();
        assert_eq!(
            obj.len(),
            3,
            "notification must have exactly 3 fields: jsonrpc, method, params"
        );
        assert_eq!(obj["jsonrpc"], "2.0");
        assert_eq!(obj["method"], "aria2.onDownloadStart");

        // params[0] must have exactly one key: "gid"
        let gid_obj = obj["params"][0].as_object().unwrap();
        assert_eq!(
            gid_obj.len(),
            1,
            "params[0] must have exactly one key: 'gid'"
        );
        assert_eq!(gid_obj["gid"], "0a0b0c0d0e0f0001");
    }

    /// GID must be formatted as 16-character lowercase hex, zero-padded.
    #[test]
    fn notification_gid_is_16_char_lowercase_hex() {
        let notif = event_to_notification(&DownloadEvent::Started {
            gid: Gid::from_raw(255),
        })
        .unwrap();

        let json = serde_json::to_value(&notif).unwrap();
        let gid = json["params"][0]["gid"].as_str().unwrap();

        assert_eq!(gid.len(), 16, "GID must be exactly 16 characters");
        assert_eq!(
            gid, "00000000000000ff",
            "GID must be zero-padded lowercase hex"
        );
        assert!(
            gid.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
            "GID must be lowercase hex only"
        );
    }

    /// All six aria2 notification methods must be supported.
    #[test]
    fn all_aria2_notification_methods_are_mapped() {
        let test_cases = vec![
            (
                DownloadEvent::Started {
                    gid: Gid::from_raw(1),
                },
                "aria2.onDownloadStart",
            ),
            (
                DownloadEvent::Paused {
                    gid: Gid::from_raw(2),
                },
                "aria2.onDownloadPause",
            ),
            (
                DownloadEvent::Stopped {
                    gid: Gid::from_raw(3),
                },
                "aria2.onDownloadStop",
            ),
            (
                DownloadEvent::Complete {
                    gid: Gid::from_raw(4),
                },
                "aria2.onDownloadComplete",
            ),
            (
                DownloadEvent::BtDownloadComplete {
                    gid: Gid::from_raw(6),
                },
                "aria2.onBtDownloadComplete",
            ),
            (
                DownloadEvent::Error {
                    gid: Gid::from_raw(5),
                    message: "connection refused".into(),
                },
                "aria2.onDownloadError",
            ),
        ];

        for (event, expected_method) in &test_cases {
            let notif = event_to_notification(event)
                .unwrap_or_else(|| panic!("event {:?} should produce a notification", event));
            assert_eq!(
                notif.method, *expected_method,
                "wrong method for event {:?}",
                event
            );
        }
    }

    /// Internal events (Progress, StatusChanged) must NOT produce notifications.
    #[test]
    fn internal_events_produce_no_notification() {
        use raria_core::job::Status;

        let events = vec![
            DownloadEvent::Progress {
                gid: Gid::from_raw(1),
                downloaded: 1024,
                total: Some(4096),
                speed: 512,
            },
            DownloadEvent::StatusChanged {
                gid: Gid::from_raw(1),
                old_status: Status::Waiting,
                new_status: Status::Active,
            },
        ];

        for event in &events {
            assert!(
                event_to_notification(event).is_none(),
                "internal event {:?} should not produce a notification",
                event
            );
        }
    }

    /// Notification must serialize to valid JSON that can be sent over WS as text.
    #[test]
    fn notification_serializes_to_valid_json_string() {
        let notif = event_to_notification(&DownloadEvent::Started {
            gid: Gid::from_raw(42),
        })
        .unwrap();

        let json_str = serde_json::to_string(&notif).unwrap();

        // Must parse back successfully
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert!(parsed.is_object());

        // Must not contain null bytes or control characters that break WS
        assert!(!json_str.contains('\0'));
    }
}
