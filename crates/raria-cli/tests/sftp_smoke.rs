use std::collections::HashMap;
use std::net::SocketAddr;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use russh::keys::ssh_key::rand_core::OsRng;
use russh::server::{Auth, Msg, Server as _, Session};
use russh::{Channel, ChannelId};
use russh_sftp::protocol::{
    Attrs, Data, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version,
};
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

fn cargo_bin(name: &str) -> String {
    std::env::var(format!("CARGO_BIN_EXE_{name}")).expect("cargo should provide binary path")
}

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
            Ok(Auth::reject())
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
            Ok(Auth::reject())
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
            let sftp = TestSftpSession {
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

struct TestSftpSession {
    file_path: String,
    file_data: Arc<Vec<u8>>,
}

impl russh_sftp::server::Handler for TestSftpSession {
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
    url: String,
    known_hosts: std::path::PathBuf,
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
                let mut methods = vec![0u8; greeting[1] as usize];
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

async fn spawn_sftp_server(temp: &std::path::Path, payload: &'static [u8]) -> SftpServerFixture {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);

    let host_key = russh::keys::PrivateKey::random(&mut OsRng, russh::keys::Algorithm::Ed25519)
        .expect("generate ssh host key");
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
        file_path: "/downloads/fixture.bin".to_string(),
        file_data: Arc::new(payload.to_vec()),
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

    let known_hosts = temp.join("known_hosts");
    std::fs::write(
        &known_hosts,
        format!("[127.0.0.1]:{} {}\n", port, host_public_key),
    )
    .expect("write known_hosts");

    SftpServerFixture {
        url: format!(
            "sftp://test-user:test-pass@127.0.0.1:{}/downloads/fixture.bin",
            port
        ),
        known_hosts,
    }
}

#[tokio::test]
async fn single_download_fetches_from_sftp_with_strict_known_hosts() {
    let temp = tempdir().expect("tempdir");
    let output_dir = temp.path().join("out");
    std::fs::create_dir_all(&output_dir).expect("create output dir");

    let fixture = spawn_sftp_server(temp.path(), b"sftp smoke payload").await;
    let output_dir_for_cmd = output_dir.clone();
    let url = fixture.url.clone();
    let known_hosts = fixture.known_hosts.clone();

    let output = tokio::task::spawn_blocking(move || {
        Command::new(cargo_bin("raria"))
            .arg("download")
            .arg(&url)
            .arg("-d")
            .arg(&output_dir_for_cmd)
            .arg("-o")
            .arg("downloaded.bin")
            .arg("-x")
            .arg("1")
            .arg("--sftp-strict-host-key-check")
            .arg("--sftp-known-hosts")
            .arg(&known_hosts)
            .output()
            .expect("run raria")
    })
    .await
    .expect("join blocking command");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(output_dir.join("downloaded.bin")).expect("read downloaded file"),
        b"sftp smoke payload"
    );
}

#[tokio::test]
async fn single_download_fetches_from_sftp_with_private_key_auth() {
    let temp = tempdir().expect("tempdir");
    let output_dir = temp.path().join("out-key");
    std::fs::create_dir_all(&output_dir).expect("create output dir");

    let fixture = spawn_sftp_server(temp.path(), b"sftp key payload").await;
    let private_key_path = temp.path().join("id_ed25519");
    let private_key = russh::keys::PrivateKey::random(&mut OsRng, russh::keys::Algorithm::Ed25519)
        .expect("generate client key");
    private_key
        .write_openssh_file(&private_key_path, russh::keys::ssh_key::LineEnding::LF)
        .expect("write private key");

    let output_dir_for_cmd = output_dir.clone();
    let url = fixture.url.clone();
    let key_path = private_key_path.clone();

    let output = tokio::task::spawn_blocking(move || {
        Command::new(cargo_bin("raria"))
            .arg("download")
            .arg(&url)
            .arg("-d")
            .arg(&output_dir_for_cmd)
            .arg("-o")
            .arg("downloaded-key.bin")
            .arg("-x")
            .arg("1")
            .arg("--sftp-private-key")
            .arg(&key_path)
            .output()
            .expect("run raria")
    })
    .await
    .expect("join blocking command");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(output_dir.join("downloaded-key.bin")).expect("read downloaded file"),
        b"sftp key payload"
    );
}

#[tokio::test]
async fn single_download_fetches_from_sftp_through_socks5_proxy() {
    let temp = tempdir().expect("tempdir");
    let output_dir = temp.path().join("out-proxy");
    std::fs::create_dir_all(&output_dir).expect("create output dir");

    let fixture = spawn_sftp_server(temp.path(), b"sftp proxy payload").await;
    let proxy = spawn_socks5_proxy().await;
    let output_dir_for_cmd = output_dir.clone();
    let url = fixture.url.clone();
    let proxy_url = proxy.clone();

    let output = tokio::task::spawn_blocking(move || {
        Command::new(cargo_bin("raria"))
            .arg("download")
            .arg(&url)
            .arg("-d")
            .arg(&output_dir_for_cmd)
            .arg("-o")
            .arg("downloaded-proxy.bin")
            .arg("-x")
            .arg("1")
            .arg("--all-proxy")
            .arg(&proxy_url)
            .output()
            .expect("run raria")
    })
    .await
    .expect("join blocking command");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(output_dir.join("downloaded-proxy.bin")).expect("read downloaded file"),
        b"sftp proxy payload"
    );
}
