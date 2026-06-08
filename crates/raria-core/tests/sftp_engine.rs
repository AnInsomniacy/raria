use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use raria_core::{
    ControlFile, DownloadEngine, RariaConfig, RpcCall, RpcEngine, RpcValue,
    write_control_file_atomic,
};
use russh::{
    Channel, ChannelId,
    keys::{Algorithm, PrivateKey},
    server::{Auth, Msg, Server as _, Session},
};
use russh_sftp::protocol::{
    Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version,
};
use tokio::{fs, net::TcpListener, sync::Mutex};

#[tokio::test]
async fn downloads_single_sftp_uri_to_configured_directory() {
    let addr = spawn_sftp_server().await;

    let temp = tempfile::tempdir().expect("download dir");
    let mut rpc = RpcEngine::default();
    let gid = rpc
        .call(RpcCall::new(
            "aria2.addUri",
            RpcValue::array([
                RpcValue::array([RpcValue::string(format!(
                    "sftp://raria:secret@{addr}/file.txt"
                ))]),
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
    assert_eq!(bytes, b"hello from sftp");

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
        Some("15")
    );
}

#[tokio::test]
async fn resumes_sftp_download_from_raria_control_file() {
    let addr = spawn_sftp_server().await;

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
            Some(15),
            6,
            vec![format!("sftp://raria:secret@{addr}/file.txt")],
        ),
    )
    .await
    .expect("control file");
    let mut rpc = RpcEngine::default();
    let gid = rpc
        .call(RpcCall::new(
            "aria2.addUri",
            RpcValue::array([
                RpcValue::array([RpcValue::string(format!(
                    "sftp://raria:secret@{addr}/file.txt"
                ))]),
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
    assert_eq!(bytes, b"hello from sftp");
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
        Some("15")
    );
}

#[tokio::test]
async fn uses_ftp_user_and_passwd_options_for_sftp_auth() {
    let addr = spawn_sftp_server().await;

    let temp = tempfile::tempdir().expect("download dir");
    let mut rpc = RpcEngine::default();
    rpc.call(RpcCall::new(
        "aria2.addUri",
        RpcValue::array([
            RpcValue::array([RpcValue::string(format!("sftp://{addr}/file.txt"))]),
            RpcValue::object([
                ("out", RpcValue::string("file.txt")),
                ("ftp-user", RpcValue::string("raria")),
                ("ftp-passwd", RpcValue::string("secret")),
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
    assert_eq!(bytes, b"hello from sftp");
}

async fn spawn_sftp_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let addr = listener.local_addr().expect("local addr");
    drop(listener);
    tokio::spawn(async move {
        let config = russh::server::Config {
            auth_rejection_time: Duration::from_secs(0),
            auth_rejection_time_initial: Some(Duration::from_secs(0)),
            keys: vec![PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("key")],
            ..Default::default()
        };
        let mut server = FixtureSshServer;
        server
            .run_on_address(Arc::new(config), addr)
            .await
            .expect("sftp server");
    });
    addr
}

#[derive(Clone)]
struct FixtureSshServer;

impl russh::server::Server for FixtureSshServer {
    type Handler = FixtureSshSession;

    fn new_client(&mut self, _: Option<SocketAddr>) -> Self::Handler {
        FixtureSshSession::default()
    }
}

struct FixtureSshSession {
    channels: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
}

impl Default for FixtureSshSession {
    fn default() -> Self {
        Self {
            channels: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl russh::server::Handler for FixtureSshSession {
    type Error = anyhow::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        if user == "raria" && password == "secret" {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            })
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        self.channels.lock().await.insert(channel.id(), channel);
        Ok(true)
    }

    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if name == "sftp" {
            let channel = self
                .channels
                .lock()
                .await
                .remove(&channel_id)
                .expect("channel");
            session.channel_success(channel_id)?;
            russh_sftp::server::run(channel.into_stream(), FixtureSftpSession::default()).await;
        } else {
            session.channel_failure(channel_id)?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct FixtureSftpSession {
    version: Option<u32>,
}

impl russh_sftp::server::Handler for FixtureSftpSession {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(
        &mut self,
        version: u32,
        _extensions: HashMap<String, String>,
    ) -> Result<Version, Self::Error> {
        self.version = Some(version);
        Ok(Version::new())
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        _pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        if filename == "/file.txt" || filename == "file.txt" {
            Ok(Handle {
                id,
                handle: filename,
            })
        } else {
            Err(StatusCode::NoSuchFile)
        }
    }

    async fn read(
        &mut self,
        id: u32,
        _handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        let bytes = b"hello from sftp";
        let start = offset as usize;
        if start >= bytes.len() {
            return Err(StatusCode::Eof);
        }
        let end = (start + len as usize).min(bytes.len());
        Ok(Data {
            id,
            data: bytes[start..end].to_vec(),
        })
    }

    async fn close(&mut self, id: u32, _handle: String) -> Result<Status, Self::Error> {
        Ok(Status {
            id,
            status_code: StatusCode::Ok,
            error_message: "Ok".to_string(),
            language_tag: "en-US".to_string(),
        })
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        Ok(Name {
            id,
            files: vec![File::dummy(path)],
        })
    }
}
