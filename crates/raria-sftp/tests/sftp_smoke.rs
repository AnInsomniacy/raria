use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use raria_range::backend::{ByteSourceBackend, OpenContext, ProbeContext};
use raria_sftp::backend::{SftpBackend, SftpBackendConfig};
use russh::keys::ssh_key::{LineEnding, rand_core::OsRng};
use russh::server::{Auth, Msg, Server as _, Session};
use russh::{Channel, ChannelId};
use russh_sftp::protocol::{
    Attrs, Data, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

#[derive(Clone)]
struct TestServer {
    file_path: String,
    file_data: Arc<Vec<u8>>,
}

impl russh::server::Server for TestServer {
    type Handler = SshSession;

    fn new_client(&mut self, _: Option<SocketAddr>) -> Self::Handler {
        SshSession {
            file_path: self.file_path.clone(),
            file_data: Arc::clone(&self.file_data),
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

struct SshSession {
    file_path: String,
    file_data: Arc<Vec<u8>>,
    clients: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
}

impl SshSession {
    async fn get_channel(&mut self, channel_id: ChannelId) -> Channel<Msg> {
        let mut clients = self.clients.lock().await;
        clients.remove(&channel_id).unwrap()
    }
}

impl russh::server::Handler for SshSession {
    type Error = anyhow::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        if user == "test-user" && password == "test-pass" {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            })
        }
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        _public_key: &russh::keys::PublicKey,
    ) -> Result<Auth, Self::Error> {
        if user == "test-user" {
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
        let mut clients = self.clients.lock().await;
        clients.insert(channel.id(), channel);
        Ok(true)
    }

    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if name == "sftp" {
            let channel = self.get_channel(channel_id).await;
            let sftp = SftpSession {
                file_path: self.file_path.clone(),
                file_data: Arc::clone(&self.file_data),
            };
            session.channel_success(channel_id)?;
            russh_sftp::server::run(channel.into_stream(), sftp).await;
        } else {
            session.channel_failure(channel_id)?;
        }

        Ok(())
    }
}

struct SftpSession {
    file_path: String,
    file_data: Arc<Vec<u8>>,
}

impl russh_sftp::server::Handler for SftpSession {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(
        &mut self,
        _version: u32,
        _extensions: HashMap<String, String>,
    ) -> Result<Version, Self::Error> {
        Ok(Version::new())
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let resolved = if path == "." { "/".to_string() } else { path };
        Ok(Name {
            id,
            files: vec![russh_sftp::protocol::File::dummy(resolved)],
        })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        if path != self.file_path {
            return Err(StatusCode::NoSuchFile);
        }
        let mut attrs = FileAttributes::empty();
        attrs.size = Some(self.file_data.len() as u64);
        attrs.permissions = Some(0o644);
        Ok(Attrs { id, attrs })
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        _pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        if filename != self.file_path {
            return Err(StatusCode::NoSuchFile);
        }
        Ok(Handle {
            id,
            handle: filename,
        })
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        if handle != self.file_path {
            return Err(StatusCode::NoSuchFile);
        }
        let start = offset as usize;
        if start >= self.file_data.len() {
            return Err(StatusCode::Eof);
        }
        let end = (start + len as usize).min(self.file_data.len());
        Ok(Data {
            id,
            data: self.file_data[start..end].to_vec(),
        })
    }

    async fn close(&mut self, id: u32, _handle: String) -> Result<Status, Self::Error> {
        Ok(Status {
            id,
            status_code: StatusCode::Ok,
            error_message: "ok".into(),
            language_tag: "en-US".into(),
        })
    }
}

struct SftpServerFixture {
    port: u16,
    host_public_key: String,
}

async fn spawn_sftp_server(file_path: &str, contents: &'static [u8]) -> SftpServerFixture {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);

    let host_key =
        russh::keys::PrivateKey::random(&mut OsRng, russh::keys::ssh_key::Algorithm::Ed25519)
            .unwrap();
    let host_public_key = host_key
        .public_key()
        .to_openssh()
        .expect("public key openssh");

    let config = russh::server::Config {
        auth_rejection_time: Duration::from_secs(1),
        auth_rejection_time_initial: Some(Duration::from_secs(0)),
        keys: vec![host_key],
        ..Default::default()
    };

    let mut server = TestServer {
        file_path: file_path.to_string(),
        file_data: Arc::new(contents.to_vec()),
    };

    tokio::spawn(async move {
        server
            .run_on_address(Arc::new(config), ("127.0.0.1", port))
            .await
            .expect("run sftp server");
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "sftp test server did not become ready in time"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    SftpServerFixture {
        port,
        host_public_key,
    }
}

async fn spawn_socks5_proxy() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind socks5");
    let addr = listener.local_addr().expect("socks5 addr");
    tokio::spawn(async move {
        loop {
            let Ok((mut downstream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut greeting = [0u8; 2];
                downstream
                    .read_exact(&mut greeting)
                    .await
                    .expect("read socks greeting");
                let methods_len = greeting[1] as usize;
                let mut methods = vec![0u8; methods_len];
                downstream
                    .read_exact(&mut methods)
                    .await
                    .expect("read socks methods");
                downstream
                    .write_all(&[0x05, 0x00])
                    .await
                    .expect("write socks method select");

                let mut req = [0u8; 4];
                downstream
                    .read_exact(&mut req)
                    .await
                    .expect("read socks request header");
                let target = match req[3] {
                    0x01 => {
                        let mut ipv4 = [0u8; 4];
                        downstream.read_exact(&mut ipv4).await.expect("read ipv4");
                        let mut port = [0u8; 2];
                        downstream.read_exact(&mut port).await.expect("read port");
                        format!(
                            "{}.{}.{}.{}:{}",
                            ipv4[0],
                            ipv4[1],
                            ipv4[2],
                            ipv4[3],
                            u16::from_be_bytes(port)
                        )
                    }
                    0x03 => {
                        let mut len = [0u8; 1];
                        downstream
                            .read_exact(&mut len)
                            .await
                            .expect("read host len");
                        let mut host = vec![0u8; len[0] as usize];
                        downstream.read_exact(&mut host).await.expect("read host");
                        let mut port = [0u8; 2];
                        downstream.read_exact(&mut port).await.expect("read port");
                        format!(
                            "{}:{}",
                            String::from_utf8(host).expect("utf8 host"),
                            u16::from_be_bytes(port)
                        )
                    }
                    other => panic!("unsupported socks atyp: {other}"),
                };

                let mut upstream = TcpStream::connect(&target).await.expect("connect upstream");
                downstream
                    .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                    .await
                    .expect("write socks connect response");
                tokio::io::copy_bidirectional(&mut downstream, &mut upstream)
                    .await
                    .expect("proxy relay");
            });
        }
    });
    format!("socks5://{}", addr)
}

#[tokio::test]
async fn sftp_backend_downloads_file_with_password_auth() {
    let fixture = spawn_sftp_server("/remote/file.txt", b"hello-sftp").await;
    let backend = SftpBackend::with_config(SftpBackendConfig::default());
    let url = format!(
        "sftp://test-user:test-pass@127.0.0.1:{}/remote/file.txt",
        fixture.port
    )
    .parse()
    .expect("url");

    let probe = backend
        .probe(&url, &ProbeContext::default())
        .await
        .expect("probe should succeed");
    assert_eq!(probe.size, Some(10));

    let mut stream = backend
        .open_from(&url, 5, &OpenContext::default())
        .await
        .expect("open_from should succeed");
    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .await
        .expect("read sftp stream");
    assert_eq!(buf, b"-sftp");
}

#[tokio::test]
async fn sftp_backend_downloads_file_with_private_key_auth() {
    let fixture = spawn_sftp_server("/remote/key-auth.txt", b"hello-key").await;

    let dir = tempfile::tempdir().expect("tempdir");
    let key_path = dir.path().join("id_ed25519");
    let key = russh::keys::PrivateKey::random(&mut OsRng, russh::keys::ssh_key::Algorithm::Ed25519)
        .unwrap();
    key.write_openssh_file(&key_path, LineEnding::LF)
        .expect("write key");

    let backend = SftpBackend::with_config(SftpBackendConfig {
        private_key_path: Some(key_path),
        ..Default::default()
    });
    let url = format!(
        "sftp://test-user@127.0.0.1:{}/remote/key-auth.txt",
        fixture.port
    )
    .parse()
    .expect("url");

    let probe = backend
        .probe(&url, &ProbeContext::default())
        .await
        .expect("probe should succeed with key auth");
    assert_eq!(probe.size, Some(9));

    let mut stream = backend
        .open_from(&url, 6, &OpenContext::default())
        .await
        .expect("open_from should succeed with key auth");
    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .await
        .expect("read sftp stream");
    assert_eq!(buf, b"key");
}

#[tokio::test]
async fn sftp_backend_honors_known_hosts_when_strict_check_is_enabled() {
    let fixture = spawn_sftp_server("/remote/known-hosts.txt", b"hello-known-hosts").await;

    let dir = tempfile::tempdir().expect("tempdir");
    let known_hosts = dir.path().join("known_hosts");
    std::fs::write(
        &known_hosts,
        format!("[127.0.0.1]:{} {}\n", fixture.port, fixture.host_public_key),
    )
    .expect("write known_hosts");

    let backend = SftpBackend::with_config(SftpBackendConfig {
        strict_host_key_check: true,
        known_hosts_path: Some(known_hosts),
        ..Default::default()
    });
    let url = format!(
        "sftp://test-user:test-pass@127.0.0.1:{}/remote/known-hosts.txt",
        fixture.port
    )
    .parse()
    .expect("url");

    let probe = backend
        .probe(&url, &ProbeContext::default())
        .await
        .expect("probe should succeed with matching known_hosts");
    assert_eq!(probe.size, Some(17));
}

#[tokio::test]
async fn sftp_backend_downloads_file_through_socks5_proxy() {
    let fixture = spawn_sftp_server("/remote/proxy.txt", b"hello-proxy").await;
    let proxy = spawn_socks5_proxy().await;

    let backend = SftpBackend::with_config(SftpBackendConfig {
        all_proxy: Some(proxy),
        ..Default::default()
    });
    let url = format!(
        "sftp://test-user:test-pass@127.0.0.1:{}/remote/proxy.txt",
        fixture.port
    )
    .parse()
    .expect("url");

    let probe = backend
        .probe(&url, &ProbeContext::default())
        .await
        .expect("probe should succeed through socks5 proxy");
    assert_eq!(probe.size, Some(11));
}
