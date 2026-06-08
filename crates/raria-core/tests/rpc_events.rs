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
