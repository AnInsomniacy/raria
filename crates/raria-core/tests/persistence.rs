use axum::{
    Router,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use raria_core::{
    ControlFile, DownloadEngine, RariaConfig, RpcCall, RpcEngine, RpcValue, read_control_file,
    write_control_file_atomic,
};
use tokio::{fs, net::TcpListener};

#[tokio::test]
async fn writes_and_reads_versioned_raria_control_file_atomically() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("file.txt.raria");
    fs::write(&path, b"previous")
        .await
        .expect("previous control file");

    let control = ControlFile::new_http(
        "0000000000000001",
        temp.path(),
        "file.txt",
        Some(16),
        6,
        vec!["http://example.invalid/file.txt".to_string()],
    );
    write_control_file_atomic(&path, &control)
        .await
        .expect("write control file");

    let read = read_control_file(&path).await.expect("read control file");
    assert_eq!(read, Some(control));
    assert!(
        fs::metadata(temp.path().join("file.txt.raria.tmp"))
            .await
            .is_err(),
        "atomic temp file is cleaned up"
    );
}

#[tokio::test]
async fn resumes_http_download_from_versioned_raria_control_file_after_restart() {
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
    let control = ControlFile::new_http(
        "0000000000000001",
        temp.path(),
        "range.txt",
        Some(16),
        6,
        vec![format!("http://{addr}/range.txt")],
    );
    write_control_file_atomic(&temp.path().join("range.txt.raria"), &control)
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

async fn range_file(headers: HeaderMap) -> Response {
    let bytes = b"hello from raria";
    let Some(range) = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("bytes="))
    else {
        return StatusCode::RANGE_NOT_SATISFIABLE.into_response();
    };
    let start = range
        .trim_end_matches('-')
        .parse::<usize>()
        .expect("range start");
    let end = bytes.len() - 1;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        header::CONTENT_RANGE,
        format!("bytes {start}-{end}/{}", bytes.len())
            .parse()
            .expect("content range"),
    );
    (
        StatusCode::PARTIAL_CONTENT,
        response_headers,
        bytes[start..=end].to_vec(),
    )
        .into_response()
}
