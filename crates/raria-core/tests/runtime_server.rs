use std::sync::Arc;

use axum::{Router, routing::get};
use raria_core::{RariaConfig, RpcEngine, RpcServer, build_rpc_router};
use serde_json::json;
use tokio::{fs, net::TcpListener, sync::Mutex};

#[tokio::test]
async fn rpc_server_downloads_added_http_task_in_background() {
    let file_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("file listener");
    let file_addr = file_listener.local_addr().expect("file addr");
    tokio::spawn(async move {
        let app = Router::new().route("/file.txt", get(|| async { "hello from raria" }));
        axum::serve(file_listener, app)
            .await
            .expect("fixture server");
    });

    let temp = tempfile::tempdir().expect("tempdir");
    let config = RariaConfig {
        download_dir: temp.path().to_path_buf(),
        rpc_listen_port: 0,
        ..RariaConfig::default()
    };
    let engine = Arc::new(Mutex::new(RpcEngine::default()));
    let app = build_rpc_router(engine.clone());
    let (server, rpc_addr) = RpcServer::start(config, engine, app)
        .await
        .expect("rpc server");
    let client = reqwest::Client::new();

    let response: serde_json::Value = client
        .post(format!("http://{rpc_addr}/jsonrpc"))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(
            json!({
            "jsonrpc": "2.0",
            "id": "add",
            "method": "aria2.addUri",
            "params": [[format!("http://{file_addr}/file.txt")], {"out": "file.txt"}]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("add request")
        .text()
        .await
        .expect("add response text")
        .parse()
        .expect("add response");
    let gid = response["result"].as_str().expect("gid").to_string();

    for _ in 0..50 {
        let response: serde_json::Value = client
            .post(format!("http://{rpc_addr}/jsonrpc"))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(
                json!({
                "jsonrpc": "2.0",
                "id": "status",
                "method": "aria2.tellStatus",
                "params": [gid]
                })
                .to_string(),
            )
            .send()
            .await
            .expect("status request")
            .text()
            .await
            .expect("status response text")
            .parse()
            .expect("status response");
        if response["result"]["status"] == "complete" {
            let bytes = fs::read(temp.path().join("file.txt"))
                .await
                .expect("downloaded file");
            assert_eq!(bytes, b"hello from raria");
            server.shutdown().await.expect("shutdown");
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    server.shutdown().await.expect("shutdown");
    panic!("download did not complete");
}
