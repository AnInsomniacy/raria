use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::StreamExt;
use raria_core::{RpcEngine, build_rpc_router};
use serde_json::json;
use tokio::{net::TcpListener, sync::Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tower::ServiceExt;

#[tokio::test]
async fn jsonrpc_http_post_adds_and_polls_download() {
    let app = test_router();

    let add_response = post_json(
        &app,
        json!({
            "jsonrpc": "2.0",
            "id": "add",
            "method": "aria2.addUri",
            "params": [["https://example.test/file.iso"]]
        }),
    )
    .await;

    assert_eq!(add_response["jsonrpc"], "2.0");
    assert_eq!(add_response["id"], "add");
    let gid = add_response["result"].as_str().expect("gid").to_owned();

    let status_response = post_json(
        &app,
        json!({
            "jsonrpc": "2.0",
            "id": "status",
            "method": "aria2.tellStatus",
            "params": [gid]
        }),
    )
    .await;

    assert_eq!(status_response["result"]["status"], "waiting");
}

#[tokio::test]
async fn jsonrpc_http_post_returns_aria2_shaped_unsupported_error() {
    let app = test_router();

    let response = post_json(
        &app,
        json!({
            "jsonrpc": "2.0",
            "id": "ed2k",
            "method": "aria2.ed2kSearch",
            "params": []
        }),
    )
    .await;

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "ed2k");
    assert_eq!(response["error"]["code"], 1);
    assert!(
        response["error"]["message"]
            .as_str()
            .expect("message")
            .contains("phase one")
    );
}

#[tokio::test]
async fn websocket_receives_download_start_notification() {
    let engine = Arc::new(Mutex::new(RpcEngine::default()));
    let app = build_rpc_router(engine);
    let http_app = app.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server");
    });

    let (mut ws, _) = connect_async(format!("ws://{addr}/jsonrpc"))
        .await
        .expect("websocket");

    let response = post_json(
        &http_app,
        json!({
            "jsonrpc": "2.0",
            "id": "add",
            "method": "aria2.addUri",
            "params": [["https://example.test/file.iso"]]
        }),
    )
    .await;
    let gid = response["result"].as_str().expect("gid");

    let message = ws.next().await.expect("message").expect("ws message");
    let Message::Text(text) = message else {
        panic!("expected text message");
    };
    let event: serde_json::Value = serde_json::from_str(&text).expect("event json");

    assert_eq!(event["jsonrpc"], "2.0");
    assert_eq!(event["method"], "aria2.onDownloadStart");
    assert_eq!(event["params"][0]["gid"], gid);
}

fn test_router() -> Router {
    build_rpc_router(Arc::new(Mutex::new(RpcEngine::default())))
}

async fn post_json(app: &Router, body: serde_json::Value) -> serde_json::Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jsonrpc")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    serde_json::from_slice(&bytes).expect("json response")
}
