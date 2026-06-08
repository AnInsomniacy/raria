use libunftp::ServerBuilder;
use raria_core::{
    ControlFile, DownloadEngine, RariaConfig, RpcCall, RpcEngine, RpcValue,
    write_control_file_atomic,
};
use tokio::{fs, net::TcpListener};
use unftp_sbe_fs::Filesystem;

#[tokio::test]
async fn downloads_single_ftp_uri_to_configured_directory() {
    let root = tempfile::tempdir().expect("ftp root");
    fs::write(root.path().join("file.txt"), b"hello from ftp")
        .await
        .expect("fixture file");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let addr = listener.local_addr().expect("local addr");
    drop(listener);
    let root_path = root.path().to_path_buf();
    tokio::spawn(async move {
        let server = ServerBuilder::new(Box::new(move || {
            Filesystem::new(root_path.clone()).expect("ftp filesystem")
        }))
        .build()
        .expect("ftp server");
        server.listen(addr.to_string()).await.expect("ftp listen");
    });

    let temp = tempfile::tempdir().expect("download dir");
    let mut rpc = RpcEngine::default();
    let gid = rpc
        .call(RpcCall::new(
            "aria2.addUri",
            RpcValue::array([
                RpcValue::array([RpcValue::string(format!("ftp://{addr}/file.txt"))]),
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
    assert_eq!(bytes, b"hello from ftp");

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
        Some("14")
    );
}

#[tokio::test]
async fn resumes_ftp_download_from_raria_control_file() {
    let root = tempfile::tempdir().expect("ftp root");
    fs::write(root.path().join("file.txt"), b"hello from ftp")
        .await
        .expect("fixture file");
    let addr = spawn_ftp_server(root.path().to_path_buf()).await;

    let temp = tempfile::tempdir().expect("download dir");
    fs::write(temp.path().join("file.txt"), b"hello ")
        .await
        .expect("partial file");
    write_control_file_atomic(
        &temp.path().join("file.txt.raria"),
        &ControlFile::new_http(
            "0000000000000001",
            temp.path(),
            "file.txt",
            Some(14),
            6,
            vec![format!("ftp://{addr}/file.txt")],
        ),
    )
    .await
    .expect("control file");
    let mut rpc = RpcEngine::default();
    let gid = rpc
        .call(RpcCall::new(
            "aria2.addUri",
            RpcValue::array([
                RpcValue::array([RpcValue::string(format!("ftp://{addr}/file.txt"))]),
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
    assert_eq!(bytes, b"hello from ftp");
    assert!(
        fs::metadata(temp.path().join("file.txt.raria"))
            .await
            .is_err()
    );

    let status = rpc
        .call(RpcCall::new(
            "aria2.tellStatus",
            RpcValue::array([RpcValue::string(gid)]),
        ))
        .expect("status");
    assert_eq!(
        status.get("completedLength").and_then(RpcValue::as_str),
        Some("14")
    );
}

#[tokio::test]
async fn uses_ftp_user_and_passwd_options_for_ftp_auth() {
    let root = tempfile::tempdir().expect("ftp root");
    fs::write(root.path().join("file.txt"), b"hello from ftp")
        .await
        .expect("fixture file");
    let addr = spawn_ftp_server(root.path().to_path_buf()).await;

    let temp = tempfile::tempdir().expect("download dir");
    let mut rpc = RpcEngine::default();
    rpc.call(RpcCall::new(
        "aria2.addUri",
        RpcValue::array([
            RpcValue::array([RpcValue::string(format!("ftp://{addr}/file.txt"))]),
            RpcValue::object([
                ("out", RpcValue::string("file.txt")),
                ("ftp-user", RpcValue::string("anonymous")),
                ("ftp-passwd", RpcValue::string("anonymous@")),
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

    let bytes = fs::read(temp.path().join("file.txt"))
        .await
        .expect("downloaded file");
    assert_eq!(bytes, b"hello from ftp");
}

async fn spawn_ftp_server(root_path: std::path::PathBuf) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let addr = listener.local_addr().expect("local addr");
    drop(listener);
    tokio::spawn(async move {
        let server = ServerBuilder::new(Box::new(move || {
            Filesystem::new(root_path.clone()).expect("ftp filesystem")
        }))
        .build()
        .expect("ftp server");
        server.listen(addr.to_string()).await.expect("ftp listen");
    });
    addr
}
