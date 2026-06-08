use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use raria_core::{DownloadEngine, RariaConfig, RpcCall, RpcEngine, RpcValue};
use tokio::{fs, net::TcpListener};

#[tokio::test]
async fn downloads_single_http_uri_to_configured_directory() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let app = Router::new().route("/file.txt", get(test_file));
        axum::serve(listener, app).await.expect("fixture server");
    });

    let temp = tempfile::tempdir().expect("tempdir");
    let mut rpc = RpcEngine::default();
    let gid = rpc
        .call(RpcCall::new(
            "aria2.addUri",
            RpcValue::array([
                RpcValue::array([RpcValue::string(format!("http://{addr}/file.txt"))]),
                RpcValue::object([("out", RpcValue::string("file.txt"))]),
            ]),
        ))
        .expect("addUri")
        .as_str()
        .expect("gid")
        .to_owned();

    let config = RariaConfig {
        download_dir: temp.path().to_path_buf(),
        ..RariaConfig::default()
    };
    DownloadEngine::new(config)
        .run_once(&mut rpc)
        .await
        .expect("download");

    let bytes = fs::read(temp.path().join("file.txt"))
        .await
        .expect("downloaded file");
    assert_eq!(bytes, b"hello from raria");

    let status = rpc
        .call(RpcCall::new(
            "aria2.tellStatus",
            RpcValue::array([RpcValue::string(gid)]),
        ))
        .expect("status");
    assert_eq!(
        status.get("status").and_then(RpcValue::as_str),
        Some("complete")
    );
    assert_eq!(
        status.get("completedLength").and_then(RpcValue::as_str),
        Some("16")
    );
}

#[tokio::test]
async fn resumes_http_download_from_raria_control_file() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let app = Router::new().route("/range.txt", get(range_file));
        axum::serve(listener, app).await.expect("fixture server");
    });

    let temp = tempfile::tempdir().expect("tempdir");
    fs::write(temp.path().join("range.txt"), b"hello ")
        .await
        .expect("partial file");
    fs::write(
        temp.path().join("range.txt.raria"),
        br#"{"completedLength":6}"#,
    )
    .await
    .expect("control file");

    let mut rpc = RpcEngine::default();
    let gid = rpc
        .call(RpcCall::new(
            "aria2.addUri",
            RpcValue::array([
                RpcValue::array([RpcValue::string(format!("http://{addr}/range.txt"))]),
                RpcValue::object([("out", RpcValue::string("range.txt"))]),
            ]),
        ))
        .expect("addUri")
        .as_str()
        .expect("gid")
        .to_owned();

    let config = RariaConfig {
        download_dir: temp.path().to_path_buf(),
        ..RariaConfig::default()
    };
    DownloadEngine::new(config)
        .run_once(&mut rpc)
        .await
        .expect("download");

    let bytes = fs::read(temp.path().join("range.txt"))
        .await
        .expect("downloaded file");
    assert_eq!(bytes, b"hello from raria");
    assert!(
        fs::metadata(temp.path().join("range.txt.raria"))
            .await
            .is_err(),
        "control file is removed after completion"
    );

    let status = rpc
        .call(RpcCall::new(
            "aria2.tellStatus",
            RpcValue::array([RpcValue::string(gid)]),
        ))
        .expect("status");
    assert_eq!(
        status.get("completedLength").and_then(RpcValue::as_str),
        Some("16")
    );
}

async fn test_file() -> impl IntoResponse {
    "hello from raria"
}

async fn range_file(headers: HeaderMap) -> Response {
    let bytes = b"hello from raria";
    match headers.get("range").and_then(|value| value.to_str().ok()) {
        Some("bytes=6-") => (StatusCode::PARTIAL_CONTENT, &bytes[6..]).into_response(),
        _ => (StatusCode::OK, &bytes[..]).into_response(),
    }
}
