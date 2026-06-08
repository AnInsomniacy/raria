use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use raria_core::{DownloadEngine, RariaConfig, RpcCall, RpcEngine, RpcValue};
use tokio::{fs, net::TcpListener, sync::Mutex, time::Instant};

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

#[tokio::test]
async fn reports_error_when_checksum_does_not_match() {
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
                RpcValue::object([
                    ("out", RpcValue::string("file.txt")),
                    ("checksum", RpcValue::string("sha-256=deadbeef")),
                ]),
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
        .expect("download attempt should finish with task error");

    let status = rpc
        .call(RpcCall::new(
            "aria2.tellStatus",
            RpcValue::array([RpcValue::string(gid)]),
        ))
        .expect("status");
    assert_eq!(
        status.get("status").and_then(RpcValue::as_str),
        Some("error")
    );
    assert!(
        status
            .get("errorMessage")
            .and_then(RpcValue::as_str)
            .expect("error message")
            .contains("checksum")
    );
}

#[tokio::test]
async fn sends_configured_header_and_cookie_file() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let app = Router::new().route("/guarded.txt", get(guarded_file));
        axum::serve(listener, app).await.expect("fixture server");
    });

    let temp = tempfile::tempdir().expect("tempdir");
    let cookie_path = temp.path().join("cookies.txt");
    fs::write(&cookie_path, "session=abc")
        .await
        .expect("cookie file");

    let mut rpc = RpcEngine::default();
    rpc.call(RpcCall::new(
        "aria2.addUri",
        RpcValue::array([
            RpcValue::array([RpcValue::string(format!("http://{addr}/guarded.txt"))]),
            RpcValue::object([
                ("out", RpcValue::string("guarded.txt")),
                ("header", RpcValue::string("X-Raria-Test: yes")),
                (
                    "load-cookies",
                    RpcValue::string(cookie_path.to_string_lossy()),
                ),
            ]),
        ]),
    ))
    .expect("addUri");

    let config = RariaConfig {
        download_dir: temp.path().to_path_buf(),
        ..RariaConfig::default()
    };
    DownloadEngine::new(config)
        .run_once(&mut rpc)
        .await
        .expect("download");

    let bytes = fs::read(temp.path().join("guarded.txt"))
        .await
        .expect("downloaded file");
    assert_eq!(bytes, b"guarded content");
}

#[tokio::test]
async fn applies_task_download_rate_limit() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let app = Router::new().route("/file.txt", get(test_file));
        axum::serve(listener, app).await.expect("fixture server");
    });

    let temp = tempfile::tempdir().expect("tempdir");
    let mut rpc = RpcEngine::default();
    rpc.call(RpcCall::new(
        "aria2.addUri",
        RpcValue::array([
            RpcValue::array([RpcValue::string(format!("http://{addr}/file.txt"))]),
            RpcValue::object([
                ("out", RpcValue::string("file.txt")),
                ("max-download-limit", RpcValue::string("40")),
            ]),
        ]),
    ))
    .expect("addUri");

    let config = RariaConfig {
        download_dir: temp.path().to_path_buf(),
        ..RariaConfig::default()
    };
    let started = Instant::now();
    DownloadEngine::new(config)
        .run_once(&mut rpc)
        .await
        .expect("download");

    assert!(
        started.elapsed().as_millis() >= 300,
        "16 bytes at 40 bytes/sec should be visibly throttled"
    );
}

#[tokio::test]
async fn splits_http_download_into_range_requests() {
    let seen_ranges = Arc::new(Mutex::new(Vec::<String>::new()));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let addr = listener.local_addr().expect("local addr");
    let seen_ranges_for_server = seen_ranges.clone();
    tokio::spawn(async move {
        let app = Router::new()
            .route("/split.txt", get(split_file))
            .with_state(seen_ranges_for_server);
        axum::serve(listener, app).await.expect("fixture server");
    });

    let temp = tempfile::tempdir().expect("tempdir");
    let mut rpc = RpcEngine::default();
    rpc.call(RpcCall::new(
        "aria2.addUri",
        RpcValue::array([
            RpcValue::array([RpcValue::string(format!("http://{addr}/split.txt"))]),
            RpcValue::object([
                ("out", RpcValue::string("split.txt")),
                ("split", RpcValue::string("2")),
            ]),
        ]),
    ))
    .expect("addUri");

    let config = RariaConfig {
        download_dir: temp.path().to_path_buf(),
        ..RariaConfig::default()
    };
    DownloadEngine::new(config)
        .run_once(&mut rpc)
        .await
        .expect("download");

    let bytes = fs::read(temp.path().join("split.txt"))
        .await
        .expect("downloaded file");
    assert_eq!(bytes, b"hello from raria");
    assert_eq!(
        *seen_ranges.lock().await,
        vec![
            "bytes=0-0".to_string(),
            "bytes=0-7".to_string(),
            "bytes=8-".to_string()
        ]
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

async fn guarded_file(headers: HeaderMap) -> Response {
    let has_header = headers
        .get("x-raria-test")
        .and_then(|value| value.to_str().ok())
        == Some("yes");
    let has_cookie = headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("session=abc"));
    if has_header && has_cookie {
        (StatusCode::OK, "guarded content").into_response()
    } else {
        (StatusCode::FORBIDDEN, "missing header or cookie").into_response()
    }
}

async fn split_file(
    State(seen_ranges): State<Arc<Mutex<Vec<String>>>>,
    headers: HeaderMap,
) -> Response {
    let bytes = b"hello from raria";
    match headers.get("range").and_then(|value| value.to_str().ok()) {
        Some(range @ "bytes=0-0") => {
            seen_ranges.lock().await.push(range.to_string());
            (
                StatusCode::PARTIAL_CONTENT,
                [("content-range", "bytes 0-0/16")],
                &bytes[0..1],
            )
                .into_response()
        }
        Some(range @ "bytes=0-7") => {
            seen_ranges.lock().await.push(range.to_string());
            (StatusCode::PARTIAL_CONTENT, &bytes[0..8]).into_response()
        }
        Some(range @ "bytes=8-") => {
            seen_ranges.lock().await.push(range.to_string());
            (StatusCode::PARTIAL_CONTENT, &bytes[8..]).into_response()
        }
        _ => (StatusCode::OK, &bytes[..]).into_response(),
    }
}
