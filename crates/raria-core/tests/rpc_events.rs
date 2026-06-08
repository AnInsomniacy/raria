use raria_core::{RpcCall, RpcEngine, RpcErrorCode, RpcValue};

#[test]
fn add_uri_then_poll_pause_unpause_remove() {
    let mut engine = RpcEngine::default();

    let gid = engine
        .call(RpcCall::new(
            "aria2.addUri",
            RpcValue::array([RpcValue::array([RpcValue::string(
                "https://example.test/file.iso",
            )])]),
        ))
        .expect("addUri should work");

    let gid = gid.as_str().expect("addUri returns gid").to_owned();

    let status = engine
        .call(RpcCall::new(
            "aria2.tellStatus",
            RpcValue::array([RpcValue::string(&gid)]),
        ))
        .expect("tellStatus should work");
    assert_eq!(
        status.get("status").and_then(RpcValue::as_str),
        Some("waiting")
    );

    assert_eq!(
        engine
            .call(RpcCall::new(
                "aria2.pause",
                RpcValue::array([RpcValue::string(&gid)])
            ))
            .expect("pause should work")
            .as_str(),
        Some(gid.as_str())
    );

    let status = engine
        .call(RpcCall::new(
            "aria2.tellStatus",
            RpcValue::array([RpcValue::string(&gid)]),
        ))
        .expect("tellStatus should work");
    assert_eq!(
        status.get("status").and_then(RpcValue::as_str),
        Some("paused")
    );

    engine
        .call(RpcCall::new(
            "aria2.unpause",
            RpcValue::array([RpcValue::string(&gid)]),
        ))
        .expect("unpause should work");

    engine
        .call(RpcCall::new(
            "aria2.remove",
            RpcValue::array([RpcValue::string(&gid)]),
        ))
        .expect("remove should work");

    let status = engine
        .call(RpcCall::new(
            "aria2.tellStatus",
            RpcValue::array([RpcValue::string(&gid)]),
        ))
        .expect("tellStatus should work");
    assert_eq!(
        status.get("status").and_then(RpcValue::as_str),
        Some("removed")
    );
}

#[test]
fn multicall_wraps_each_result_like_aria2() {
    let mut engine = RpcEngine::default();

    let result = engine
        .call(RpcCall::new(
            "system.multicall",
            RpcValue::array([RpcValue::array([
                RpcValue::object([
                    ("methodName", RpcValue::string("aria2.addUri")),
                    (
                        "params",
                        RpcValue::array([RpcValue::array([RpcValue::string(
                            "https://example.test/a.iso",
                        )])]),
                    ),
                ]),
                RpcValue::object([("methodName", RpcValue::string("aria2.getGlobalStat"))]),
            ])]),
        ))
        .expect("multicall should work");

    let items = result.as_array().expect("multicall returns array");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].as_array().expect("wrapped result").len(), 1);
    assert_eq!(
        items[1].as_array().expect("wrapped result")[0]
            .get("numWaiting")
            .and_then(RpcValue::as_str),
        Some("1")
    );
}

#[test]
fn common_query_methods_support_new_session_polling() {
    let mut engine = RpcEngine::default();

    let gid = engine
        .call(RpcCall::new(
            "aria2.addUri",
            RpcValue::array([
                RpcValue::string("token:secret"),
                RpcValue::array([RpcValue::string("https://example.test/file.iso")]),
            ]),
        ))
        .expect("token addUri should work")
        .as_str()
        .expect("gid")
        .to_owned();

    let uris = engine
        .call(RpcCall::new(
            "aria2.getUris",
            RpcValue::array([RpcValue::string(&gid)]),
        ))
        .expect("getUris");
    assert_eq!(
        uris.as_array()
            .and_then(|items| items.first())
            .and_then(|item| item.get("uri"))
            .and_then(RpcValue::as_str),
        Some("https://example.test/file.iso")
    );

    let waiting = engine
        .call(RpcCall::new(
            "aria2.tellWaiting",
            RpcValue::array([
                RpcValue::string("0"),
                RpcValue::string("10"),
                RpcValue::array([]),
            ]),
        ))
        .expect("tellWaiting");
    assert_eq!(waiting.as_array().expect("waiting").len(), 1);

    let active = engine
        .call(RpcCall::new("aria2.tellActive", RpcValue::array([])))
        .expect("tellActive");
    assert_eq!(active.as_array().expect("active").len(), 0);

    engine
        .call(RpcCall::new(
            "aria2.remove",
            RpcValue::array([RpcValue::string(&gid)]),
        ))
        .expect("remove");
    let stopped = engine
        .call(RpcCall::new(
            "aria2.tellStopped",
            RpcValue::array([
                RpcValue::string("0"),
                RpcValue::string("10"),
                RpcValue::array([]),
            ]),
        ))
        .expect("tellStopped");
    assert_eq!(stopped.as_array().expect("stopped").len(), 1);
}

#[test]
fn session_metadata_methods_are_available() {
    let mut engine = RpcEngine::default();

    let version = engine
        .call(RpcCall::new("aria2.getVersion", RpcValue::array([])))
        .expect("version");
    assert_eq!(
        version.get("version").and_then(RpcValue::as_str),
        Some("0.1.0")
    );
    assert!(
        version
            .get("enabledFeatures")
            .and_then(RpcValue::as_array)
            .expect("features")
            .iter()
            .any(|feature| feature.as_str() == Some("BitTorrent"))
    );

    let session = engine
        .call(RpcCall::new("aria2.getSessionInfo", RpcValue::array([])))
        .expect("session info");
    assert_eq!(
        session.get("sessionId").and_then(RpcValue::as_str),
        Some("raria-new-session")
    );

    let saved = engine
        .call(RpcCall::new("aria2.saveSession", RpcValue::array([])))
        .expect("save session");
    assert_eq!(saved.as_str(), Some("OK"));
}

#[test]
fn events_follow_aria2_notification_shape() {
    let mut engine = RpcEngine::default();
    let gid = engine
        .call(RpcCall::new(
            "aria2.addUri",
            RpcValue::array([RpcValue::array([RpcValue::string(
                "https://example.test/file.iso",
            )])]),
        ))
        .expect("addUri should work")
        .as_str()
        .expect("gid")
        .to_owned();

    let events = engine.drain_events();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].method, "aria2.onDownloadStart");
    assert_eq!(
        events[0].params[0].get("gid").and_then(RpcValue::as_str),
        Some(gid.as_str())
    );
}

#[test]
fn ed2k_methods_return_explicit_unsupported_error() {
    let mut engine = RpcEngine::default();

    let error = engine
        .call(RpcCall::new("aria2.ed2kSearch", RpcValue::array([])))
        .expect_err("ED2K is not implemented in phase one");

    assert_eq!(error.code, RpcErrorCode::UnsupportedPhaseOne);
}

#[test]
fn add_torrent_exposes_bittorrent_status_fields() {
    let torrent = "ZDg6YW5ub3VuY2UzMTpodHRwOi8vdHJhY2tlci5leGFtcGxlL2Fubm91bmNlNDppbmZvZDY6bGVuZ3RoaTE0ZTQ6bmFtZTg6ZmlsZS50eHQxMjpwaWVjZSBsZW5ndGhpMTYzODRlNjpwaWVjZXMyMDoxMjM0NTY3ODkwMTIzNDU2Nzg5MGVl";
    let mut engine = RpcEngine::default();

    let gid = engine
        .call(RpcCall::new(
            "aria2.addTorrent",
            RpcValue::array([RpcValue::string(torrent)]),
        ))
        .expect("addTorrent should work")
        .as_str()
        .expect("gid")
        .to_owned();

    let status = engine
        .call(RpcCall::new(
            "aria2.tellStatus",
            RpcValue::array([RpcValue::string(gid)]),
        ))
        .expect("tellStatus should work");

    assert_eq!(
        status
            .get("bittorrent")
            .and_then(|value| value.get("info"))
            .and_then(|value| value.get("name"))
            .and_then(RpcValue::as_str),
        Some("file.txt")
    );
    assert_eq!(
        status.get("infoHash").and_then(RpcValue::as_str),
        Some("9d8cd776fc2f80d08eee2de831b139010d4b033f")
    );
    assert_eq!(
        status.get("totalLength").and_then(RpcValue::as_str),
        Some("14")
    );
}

#[test]
fn add_torrent_maps_selected_files_into_file_status() {
    let torrent = "ZDg6YW5ub3VuY2UzMTpodHRwOi8vdHJhY2tlci5leGFtcGxlL2Fubm91bmNlNDppbmZvZDU6ZmlsZXNsZDY6bGVuZ3RoaTVlNDpwYXRobDE6YWVlZDY6bGVuZ3RoaTdlNDpwYXRobDE6YmVlZTQ6bmFtZTY6YnVuZGxlMTI6cGllY2UgbGVuZ3RoaTE2Mzg0ZTY6cGllY2VzMjA6MTIzNDU2Nzg5MDEyMzQ1Njc4OTBlZQ==";
    let mut engine = RpcEngine::default();

    let gid = engine
        .call(RpcCall::new(
            "aria2.addTorrent",
            RpcValue::array([
                RpcValue::string(torrent),
                RpcValue::array([]),
                RpcValue::object([("select-file", RpcValue::string("2"))]),
            ]),
        ))
        .expect("addTorrent should work")
        .as_str()
        .expect("gid")
        .to_owned();

    let status = engine
        .call(RpcCall::new(
            "aria2.tellStatus",
            RpcValue::array([RpcValue::string(gid)]),
        ))
        .expect("tellStatus should work");
    let files = status
        .get("files")
        .and_then(RpcValue::as_array)
        .expect("files");

    assert_eq!(
        files[0].get("selected").and_then(RpcValue::as_str),
        Some("false")
    );
    assert_eq!(
        files[1].get("selected").and_then(RpcValue::as_str),
        Some("true")
    );
}

#[test]
fn get_files_returns_bittorrent_file_list() {
    let torrent = "ZDg6YW5ub3VuY2UzMTpodHRwOi8vdHJhY2tlci5leGFtcGxlL2Fubm91bmNlNDppbmZvZDU6ZmlsZXNsZDY6bGVuZ3RoaTVlNDpwYXRobDE6YWVlZDY6bGVuZ3RoaTdlNDpwYXRobDE6YmVlZTQ6bmFtZTY6YnVuZGxlMTI6cGllY2UgbGVuZ3RoaTE2Mzg0ZTY6cGllY2VzMjA6MTIzNDU2Nzg5MDEyMzQ1Njc4OTBlZQ==";
    let mut engine = RpcEngine::default();
    let gid = engine
        .call(RpcCall::new(
            "aria2.addTorrent",
            RpcValue::array([RpcValue::string(torrent)]),
        ))
        .expect("addTorrent")
        .as_str()
        .expect("gid")
        .to_owned();

    let files = engine
        .call(RpcCall::new(
            "aria2.getFiles",
            RpcValue::array([RpcValue::string(gid)]),
        ))
        .expect("getFiles");
    let files = files.as_array().expect("files");

    assert_eq!(files.len(), 2);
    assert_eq!(files[0].get("path").and_then(RpcValue::as_str), Some("a"));
    assert_eq!(files[1].get("length").and_then(RpcValue::as_str), Some("7"));
}

#[test]
fn add_uri_accepts_magnet_as_bittorrent_metadata_task() {
    let mut engine = RpcEngine::default();

    let gid = engine
        .call(RpcCall::new(
            "aria2.addUri",
            RpcValue::array([RpcValue::array([RpcValue::string(
                "magnet:?xt=urn:btih:9d8cd776fc2f80d08eee2de831b139010d4b033f&dn=file.txt",
            )])]),
        ))
        .expect("addUri should work")
        .as_str()
        .expect("gid")
        .to_owned();

    let status = engine
        .call(RpcCall::new(
            "aria2.tellStatus",
            RpcValue::array([RpcValue::string(gid)]),
        ))
        .expect("tellStatus should work");

    assert_eq!(
        status
            .get("bittorrent")
            .and_then(|value| value.get("info"))
            .and_then(|value| value.get("name"))
            .and_then(RpcValue::as_str),
        Some("file.txt")
    );
    assert_eq!(
        status.get("infoHash").and_then(RpcValue::as_str),
        Some("9d8cd776fc2f80d08eee2de831b139010d4b033f")
    );
}

#[test]
fn add_uri_maps_magnet_selected_files_into_file_status() {
    let mut engine = RpcEngine::default();

    let gid = engine
        .call(RpcCall::new(
            "aria2.addUri",
            RpcValue::array([
                RpcValue::array([RpcValue::string(
                    "magnet:?xt=urn:btih:9d8cd776fc2f80d08eee2de831b139010d4b033f&dn=file.txt",
                )]),
                RpcValue::object([("select-file", RpcValue::string("2-3"))]),
            ]),
        ))
        .expect("addUri should work")
        .as_str()
        .expect("gid")
        .to_owned();

    let task = engine
        .pending_bittorrent_tasks()
        .into_iter()
        .find(|task| task.gid == gid)
        .expect("pending bittorrent task");

    assert_eq!(task.selected_files, Some(vec![2, 3]));
}

#[test]
fn lists_methods_and_notifications_for_client_discovery() {
    let mut engine = RpcEngine::default();

    let methods = engine
        .call(RpcCall::new("system.listMethods", RpcValue::array([])))
        .expect("listMethods should work");
    let notifications = engine
        .call(RpcCall::new(
            "system.listNotifications",
            RpcValue::array([]),
        ))
        .expect("listNotifications should work");

    assert!(
        methods
            .as_array()
            .expect("methods")
            .iter()
            .any(|method| method.as_str() == Some("aria2.addUri"))
    );
    assert!(
        notifications
            .as_array()
            .expect("notifications")
            .iter()
            .any(|method| method.as_str() == Some("aria2.onDownloadStart"))
    );
}
