use axum::{Router, routing::get};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use raria_core::{DownloadEngine, RariaConfig, RpcCall, RpcEngine, RpcValue, parse_metalink_bytes};
use tokio::{fs, net::TcpListener};

#[test]
fn parses_metalink_v4_file_resources_and_hash() {
    let metalink = br#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="file.txt">
    <size>16</size>
    <hash type="sha-256">0b57d9df97670d2b0118fcc4189998e8d93c339e5e257a86f1e69bd2801134cf</hash>
    <url priority="1">http://example.invalid/file.txt</url>
  </file>
</metalink>"#;

    let doc = parse_metalink_bytes(metalink).expect("metalink");

    assert_eq!(doc.files.len(), 1);
    assert_eq!(doc.files[0].name, "file.txt");
    assert_eq!(doc.files[0].size, Some(16));
    assert_eq!(
        doc.files[0].checksum.as_deref(),
        Some("sha-256=0b57d9df97670d2b0118fcc4189998e8d93c339e5e257a86f1e69bd2801134cf")
    );
    assert_eq!(
        doc.files[0].resources,
        vec!["http://example.invalid/file.txt"]
    );
}

#[test]
fn parses_metalink_v3_file_resources_and_hash() {
    let metalink = br#"<?xml version="1.0" encoding="UTF-8"?>
<metalink version="3.0" xmlns="http://www.metalinker.org/">
  <files>
    <file name="archive.tar">
      <size>12</size>
      <verification>
        <hash type="sha-256">5d7b5313d81195e4caf90aa52719f240eb93d50d2b38288a85ef6724af80c97a</hash>
      </verification>
      <resources>
        <url type="http">http://example.invalid/archive.tar</url>
      </resources>
    </file>
  </files>
</metalink>"#;

    let doc = parse_metalink_bytes(metalink).expect("metalink");

    assert_eq!(doc.files.len(), 1);
    assert_eq!(doc.files[0].name, "archive.tar");
    assert_eq!(doc.files[0].size, Some(12));
    assert_eq!(
        doc.files[0].checksum.as_deref(),
        Some("sha-256=5d7b5313d81195e4caf90aa52719f240eb93d50d2b38288a85ef6724af80c97a")
    );
    assert_eq!(
        doc.files[0].resources,
        vec!["http://example.invalid/archive.tar"]
    );
}

#[tokio::test]
async fn add_metalink_creates_http_download_task() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let app = Router::new().route("/file.txt", get(|| async { "hello from raria" }));
        axum::serve(listener, app).await.expect("fixture server");
    });
    let metalink = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="file.txt">
    <size>16</size>
    <hash type="sha-256">0b57d9df97670d2b0118fcc4189998e8d93c339e5e257a86f1e69bd2801134cf</hash>
    <url>http://{addr}/file.txt</url>
  </file>
</metalink>"#
    );

    let temp = tempfile::tempdir().expect("tempdir");
    let mut rpc = RpcEngine::default();
    let result = rpc
        .call(RpcCall::new(
            "aria2.addMetalink",
            RpcValue::array([RpcValue::string(STANDARD.encode(metalink.as_bytes()))]),
        ))
        .expect("addMetalink");
    let gids = result.as_array().expect("gids");
    let gid = gids
        .first()
        .and_then(RpcValue::as_str)
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
}
