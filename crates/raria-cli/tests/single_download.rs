use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::{fs, io::Write};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{RootCertStore, ServerConfig};
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use wiremock::matchers::{header, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn cargo_bin(name: &str) -> String {
    std::env::var(format!("CARGO_BIN_EXE_{name}")).expect("cargo should provide binary path")
}

struct FtpFixture {
    port: u16,
}

struct FtpsFixture {
    port: u16,
    ca_path: std::path::PathBuf,
}

fn is_disconnect(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionAborted
    )
}

async fn spawn_ftp_server(
    expected_user: &'static str,
    expected_password: &'static str,
    file_path: &'static str,
    file_data: &'static [u8],
) -> FtpFixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ftp listener");
    let port = listener.local_addr().expect("ftp addr").port();

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(handle_ftp_client(
                stream,
                expected_user,
                expected_password,
                file_path,
                file_data,
            ));
        }
    });

    FtpFixture { port }
}

async fn spawn_explicit_ftps_server(
    expected_user: &'static str,
    expected_password: &'static str,
    file_path: &'static str,
    file_data: &'static [u8],
) -> FtpsFixture {
    use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair};

    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let _ = provider.clone().install_default();

    let ca_key = KeyPair::generate().expect("generate ca key");
    let mut ca_params = CertificateParams::new(vec!["raria-ftps-test-ca".into()]).expect("ca params");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.distinguished_name.push(DnType::CommonName, "raria-ftps-test-ca");
    let ca_cert = ca_params.self_signed(&ca_key).expect("ca cert");

    let server_key = KeyPair::generate().expect("generate server key");
    let mut server_params =
        CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()]).expect("server params");
    server_params.distinguished_name.push(DnType::CommonName, "127.0.0.1");
    let server_cert = server_params
        .signed_by(&server_key, &ca_cert, &ca_key)
        .expect("server cert");

    let cert_dir = tempdir().expect("ftps cert tempdir");
    let ca_path = cert_dir.path().join("ftps-ca.pem");
    fs::write(&ca_path, ca_cert.pem()).expect("write ftps ca pem");
    let ca_path_for_return = ca_path.clone();

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![server_cert.der().clone()],
            PrivateKeyDer::from(PrivatePkcs8KeyDer::from(server_key.serialize_der())),
        )
        .expect("ftps server config");
    let acceptor = TlsAcceptor::from(Arc::new(server_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ftps listener");
    let port = listener.local_addr().expect("ftps addr").port();

    tokio::spawn(async move {
        let _keep_dir_alive = cert_dir;
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(handle_ftps_client(
                stream,
                acceptor,
                expected_user,
                expected_password,
                file_path,
                file_data,
            ));
        }
    });

    FtpsFixture {
        port,
        ca_path: ca_path_for_return,
    }
}

async fn handle_ftp_client(
    stream: TcpStream,
    expected_user: &str,
    expected_password: &str,
    file_path: &str,
    file_data: &[u8],
) {
    let mut reader = BufReader::new(stream);
    let mut pending_offset = 0usize;
    let mut data_listener: Option<TcpListener> = None;
    let mut authenticated = false;
    let mut seen_user: Option<String> = None;

    write_ftp_reply(reader.get_mut(), 220, "raria ftp test server")
        .await
        .expect("write greeting");

    loop {
        let mut line = String::new();
        let n = match reader.read_line(&mut line).await {
            Ok(n) => n,
            Err(error) if is_disconnect(&error) => break,
            Err(error) => panic!("read ftp command: {error}"),
        };
        if n == 0 {
            break;
        }

        let line = line.trim_end_matches(['\r', '\n']);
        let (command, arg) = line
            .split_once(' ')
            .map(|(cmd, rest)| (cmd.to_ascii_uppercase(), rest))
            .unwrap_or_else(|| (line.to_ascii_uppercase(), ""));

        match command.as_str() {
            "USER" => {
                seen_user = Some(arg.to_string());
                write_ftp_reply(reader.get_mut(), 331, "password required")
                    .await
                    .expect("write user reply");
            }
            "PASS" => {
                authenticated = seen_user.as_deref() == Some(expected_user) && arg == expected_password;
                let code = if authenticated { 230 } else { 530 };
                let message = if authenticated {
                    "login successful"
                } else {
                    "login incorrect"
                };
                write_ftp_reply(reader.get_mut(), code, message)
                    .await
                    .expect("write pass reply");
            }
            "TYPE" => {
                assert!(authenticated, "TYPE before authentication");
                write_ftp_reply(reader.get_mut(), 200, "type set")
                    .await
                    .expect("write type reply");
            }
            "SIZE" => {
                assert!(authenticated, "SIZE before authentication");
                assert_eq!(arg, file_path, "unexpected SIZE path");
                write_ftp_reply(reader.get_mut(), 213, &file_data.len().to_string())
                    .await
                    .expect("write size reply");
            }
            "REST" => {
                assert!(authenticated, "REST before authentication");
                pending_offset = arg.parse::<usize>().expect("parse rest offset");
                write_ftp_reply(reader.get_mut(), 350, "restart position accepted")
                    .await
                    .expect("write rest reply");
            }
            "PASV" => {
                assert!(authenticated, "PASV before authentication");
                let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind data listener");
                let addr = listener.local_addr().expect("data addr");
                let octets = match addr.ip() {
                    std::net::IpAddr::V4(ip) => ip.octets(),
                    std::net::IpAddr::V6(_) => panic!("expected ipv4 listener"),
                };
                let reply = format!(
                    "Entering Passive Mode ({},{},{},{},{},{})",
                    octets[0],
                    octets[1],
                    octets[2],
                    octets[3],
                    addr.port() / 256,
                    addr.port() % 256
                );
                data_listener = Some(listener);
                write_ftp_reply(reader.get_mut(), 227, &reply)
                    .await
                    .expect("write pasv reply");
            }
            "RETR" => {
                assert!(authenticated, "RETR before authentication");
                assert_eq!(arg, file_path, "unexpected RETR path");
                let listener = data_listener.take().expect("RETR without PASV");
                write_ftp_reply(reader.get_mut(), 150, "opening data connection")
                    .await
                    .expect("write retr start");
                let (mut data_stream, _) = listener.accept().await.expect("accept data");
                data_stream
                    .write_all(&file_data[pending_offset.min(file_data.len())..])
                    .await
                    .expect("write data payload");
                data_stream.shutdown().await.expect("shutdown data stream");
                pending_offset = 0;
                write_ftp_reply(reader.get_mut(), 226, "transfer complete")
                    .await
                    .expect("write retr done");
            }
            "QUIT" => {
                write_ftp_reply(reader.get_mut(), 221, "goodbye")
                    .await
                    .expect("write quit reply");
                break;
            }
            other => panic!("unexpected FTP command: {other} {arg}"),
        }
    }
}

async fn handle_ftps_client(
    stream: TcpStream,
    acceptor: TlsAcceptor,
    expected_user: &str,
    expected_password: &str,
    file_path: &str,
    file_data: &[u8],
) {
    let mut reader = BufReader::new(stream);
    write_ftp_reply(reader.get_mut(), 220, "raria explicit ftps test server")
        .await
        .expect("write greeting");

    let mut line = String::new();
    let n = match reader.read_line(&mut line).await {
        Ok(n) => n,
        Err(error) if is_disconnect(&error) => return,
        Err(error) => panic!("read plain ftps command: {error}"),
    };
    if n == 0 {
        return;
    }
    let line = line.trim_end_matches(['\r', '\n']);
    let (command, arg) = line
        .split_once(' ')
        .map(|(cmd, rest)| (cmd.to_ascii_uppercase(), rest))
        .unwrap_or_else(|| (line.to_ascii_uppercase(), ""));

    match command.as_str() {
        "AUTH" => {
            assert_eq!(arg, "TLS", "unexpected AUTH mode");
            write_ftp_reply(reader.get_mut(), 234, "AUTH TLS accepted")
                .await
                .expect("write auth reply");
            let tls_stream = acceptor.accept(reader.into_inner()).await.expect("upgrade control tls");
            handle_ftps_tls_session(
                tls_stream,
                acceptor,
                expected_user,
                expected_password,
                file_path,
                file_data,
            )
            .await;
        }
        "QUIT" => {
            write_ftp_reply(reader.get_mut(), 221, "goodbye")
                .await
                .expect("write quit");
        }
        other => panic!("unexpected plain FTPS command before AUTH TLS: {other} {arg}"),
    }
}

async fn handle_ftps_tls_session(
    stream: tokio_rustls::server::TlsStream<TcpStream>,
    acceptor: TlsAcceptor,
    expected_user: &str,
    expected_password: &str,
    file_path: &str,
    file_data: &[u8],
) {
    let mut reader = BufReader::new(stream);
    let mut pending_offset = 0usize;
    let mut data_listener: Option<TcpListener> = None;
    let mut authenticated = false;
    let mut seen_user: Option<String> = None;
    let mut protect_private = false;

    loop {
        let mut line = String::new();
        let n = match reader.read_line(&mut line).await {
            Ok(n) => n,
            Err(error) if is_disconnect(&error) => break,
            Err(error) => panic!("read tls ftps command: {error}"),
        };
        if n == 0 {
            break;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        let (command, arg) = line
            .split_once(' ')
            .map(|(cmd, rest)| (cmd.to_ascii_uppercase(), rest))
            .unwrap_or_else(|| (line.to_ascii_uppercase(), ""));

        match command.as_str() {
            "USER" => {
                seen_user = Some(arg.to_string());
                write_ftp_reply(reader.get_mut(), 331, "password required")
                    .await
                    .expect("write user reply");
            }
            "PASS" => {
                authenticated = seen_user.as_deref() == Some(expected_user) && arg == expected_password;
                write_ftp_reply(
                    reader.get_mut(),
                    if authenticated { 230 } else { 530 },
                    if authenticated { "login successful" } else { "login incorrect" },
                )
                .await
                .expect("write pass reply");
            }
            "PBSZ" => {
                assert_eq!(arg, "0");
                write_ftp_reply(reader.get_mut(), 200, "pbsz=0")
                    .await
                    .expect("write pbsz");
            }
            "PROT" => {
                assert_eq!(arg, "P");
                protect_private = true;
                write_ftp_reply(reader.get_mut(), 200, "protection private")
                    .await
                    .expect("write prot");
            }
            "TYPE" => {
                assert!(authenticated, "TYPE before authentication");
                write_ftp_reply(reader.get_mut(), 200, "type set to I")
                    .await
                    .expect("write type");
            }
            "SIZE" => {
                assert!(authenticated, "SIZE before authentication");
                assert_eq!(arg, file_path);
                write_ftp_reply(reader.get_mut(), 213, &file_data.len().to_string())
                    .await
                    .expect("write size");
            }
            "REST" => {
                pending_offset = arg.parse::<usize>().expect("parse rest");
                write_ftp_reply(reader.get_mut(), 350, "restart position accepted")
                    .await
                    .expect("write rest");
            }
            "PASV" => {
                let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ftps data listener");
                let addr = listener.local_addr().expect("data addr");
                let octets = match addr.ip() {
                    std::net::IpAddr::V4(ip) => ip.octets(),
                    std::net::IpAddr::V6(_) => panic!("expected ipv4 listener"),
                };
                data_listener = Some(listener);
                write_ftp_reply(
                    reader.get_mut(),
                    227,
                    &format!(
                        "Entering Passive Mode ({},{},{},{},{},{})",
                        octets[0],
                        octets[1],
                        octets[2],
                        octets[3],
                        addr.port() / 256,
                        addr.port() % 256
                    ),
                )
                .await
                .expect("write pasv");
            }
            "RETR" => {
                assert_eq!(arg, file_path);
                let listener = data_listener.take().expect("RETR without PASV");
                write_ftp_reply(reader.get_mut(), 150, "opening protected data connection")
                    .await
                    .expect("write retr");
                let (data_stream, _) = listener.accept().await.expect("accept data");
                if protect_private {
                    let mut data_stream = acceptor.accept(data_stream).await.expect("upgrade data tls");
                    data_stream
                        .write_all(&file_data[pending_offset.min(file_data.len())..])
                        .await
                        .expect("write tls data");
                    data_stream.shutdown().await.expect("shutdown tls data");
                } else {
                    let mut data_stream = data_stream;
                    data_stream
                        .write_all(&file_data[pending_offset.min(file_data.len())..])
                        .await
                        .expect("write plain data");
                    data_stream.shutdown().await.expect("shutdown plain data");
                }
                pending_offset = 0;
                write_ftp_reply(reader.get_mut(), 226, "transfer complete")
                    .await
                    .expect("write complete");
            }
            "QUIT" => {
                write_ftp_reply(reader.get_mut(), 221, "goodbye")
                    .await
                    .expect("write quit");
                break;
            }
            other => panic!("unexpected FTPS command: {other} {arg}"),
        }
    }
}

async fn write_ftp_reply<W>(stream: &mut W, code: u16, message: &str) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    stream
        .write_all(format!("{code} {message}\r\n").as_bytes())
        .await?;
    stream.flush().await
}

async fn spawn_socks5_proxy(connect_count: Arc<AtomicUsize>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind socks5 proxy");
    let addr = listener.local_addr().expect("socks5 addr");

    tokio::spawn(async move {
        loop {
            let Ok((mut downstream, _)) = listener.accept().await else {
                break;
            };
            let connect_count = Arc::clone(&connect_count);
            tokio::spawn(async move {
                let mut greeting = [0u8; 2];
                downstream.read_exact(&mut greeting).await.expect("read greeting");
                let mut methods = vec![0u8; greeting[1] as usize];
                downstream.read_exact(&mut methods).await.expect("read methods");
                downstream
                    .write_all(&[0x05, 0x00])
                    .await
                    .expect("write method select");

                let mut req = [0u8; 4];
                downstream.read_exact(&mut req).await.expect("read request header");
                let target = match req[3] {
                    0x01 => {
                        let mut ipv4 = [0u8; 4];
                        downstream.read_exact(&mut ipv4).await.expect("read ipv4");
                        let mut port = [0u8; 2];
                        downstream.read_exact(&mut port).await.expect("read port");
                        format!(
                            "{}.{}.{}.{}:{}",
                            ipv4[0], ipv4[1], ipv4[2], ipv4[3], u16::from_be_bytes(port)
                        )
                    }
                    0x03 => {
                        let mut len = [0u8; 1];
                        downstream.read_exact(&mut len).await.expect("read host len");
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

                connect_count.fetch_add(1, Ordering::SeqCst);
                let mut upstream = TcpStream::connect(target).await.expect("connect upstream");
                downstream
                    .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                    .await
                    .expect("write connect reply");
                if let Err(error) = tokio::io::copy_bidirectional(&mut downstream, &mut upstream).await {
                    assert!(is_disconnect(&error), "relay socks5: {error}");
                }
            });
        }
    });

    format!("socks5://{}", addr)
}

struct MtlsFixture {
    url: String,
    ca_path: std::path::PathBuf,
    client_cert_path: std::path::PathBuf,
    client_key_path: std::path::PathBuf,
}

async fn spawn_mtls_server(temp: &std::path::Path) -> MtlsFixture {
    use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair};
    use rustls::server::WebPkiClientVerifier;
    use std::sync::Arc;

    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let _ = provider.clone().install_default();
    let provider = Arc::new(provider);

    let ca_key = KeyPair::generate().expect("generate ca key");
    let mut ca_params = CertificateParams::new(vec!["raria-test-ca".into()]).expect("ca params");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.distinguished_name.push(DnType::CommonName, "raria-test-ca");
    let ca_cert = ca_params.self_signed(&ca_key).expect("ca cert");

    let server_key = KeyPair::generate().expect("generate server key");
    let mut server_params =
        CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()]).expect("server params");
    server_params.distinguished_name.push(DnType::CommonName, "localhost");
    let server_cert = server_params
        .signed_by(&server_key, &ca_cert, &ca_key)
        .expect("server cert");

    let client_key = KeyPair::generate().expect("generate client key");
    let mut client_params = CertificateParams::new(vec!["raria-client".into()]).expect("client params");
    client_params.distinguished_name.push(DnType::CommonName, "raria-client");
    let client_cert = client_params
        .signed_by(&client_key, &ca_cert, &ca_key)
        .expect("client cert");

    let ca_path = temp.join("ca.pem");
    let client_cert_path = temp.join("client.pem");
    let client_key_path = temp.join("client.key");
    fs::write(&ca_path, ca_cert.pem()).expect("write ca pem");
    fs::write(&client_cert_path, client_cert.pem()).expect("write client cert");
    fs::write(&client_key_path, client_key.serialize_pem()).expect("write client key");

    let mut roots = RootCertStore::empty();
    roots.add(ca_cert.der().clone()).expect("add ca");
    let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .build()
        .expect("client verifier");

    let server_config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("tls13")
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            vec![server_cert.der().clone()],
            PrivateKeyDer::from(PrivatePkcs8KeyDer::from(server_key.serialize_der())),
        )
        .expect("server config");

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    let acceptor = TlsAcceptor::from(Arc::new(server_config));

    tokio::spawn(async move {
        for _ in 0..2 {
            let (stream, _) = listener.accept().await.expect("accept tcp");
            let mut tls = acceptor.accept(stream).await.expect("accept tls");
            let mut buf = [0u8; 4096];
            let n = tls.read(&mut buf).await.expect("read request");
            let req = String::from_utf8_lossy(&buf[..n]);
            let response = if req.starts_with("HEAD /mtls.bin ") {
                "HTTP/1.1 200 OK\r\ncontent-length: 5\r\naccept-ranges: bytes\r\n\r\n".to_string()
            } else if req.starts_with("GET /mtls.bin ") {
                "HTTP/1.1 200 OK\r\ncontent-length: 5\r\n\r\nhello".to_string()
            } else {
                "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n".to_string()
            };
            tls.write_all(response.as_bytes()).await.expect("write response");
            tls.flush().await.expect("flush response");
        }
    });

    MtlsFixture {
        url: format!("https://127.0.0.1:{}/mtls.bin", addr.port()),
        ca_path,
        client_cert_path,
        client_key_path,
    }
}

#[tokio::test]
async fn single_download_uses_suggested_filename_when_out_is_not_provided() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/download"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "11")
                .insert_header("accept-ranges", "bytes")
                .insert_header("content-disposition", "attachment; filename=server-name.bin"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello world"))
        .mount(&server)
        .await;

    let tmp = tempdir().expect("tempdir");
    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg(server.uri().to_string() + "/download")
        .arg("-d")
        .arg(tmp.path())
        .output()
        .expect("run raria");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let expected = tmp.path().join("server-name.bin");
    assert!(
        expected.is_file(),
        "expected downloaded file at {}",
        expected.display()
    );
    assert_eq!(std::fs::read(&expected).expect("read downloaded file"), b"hello world");
}

#[tokio::test]
async fn single_download_keeps_explicit_out_over_suggested_filename() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/download"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "11")
                .insert_header("accept-ranges", "bytes")
                .insert_header("content-disposition", "attachment; filename=server-name.bin"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello world"))
        .mount(&server)
        .await;

    let tmp = tempdir().expect("tempdir");
    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg(server.uri().to_string() + "/download")
        .arg("-d")
        .arg(tmp.path())
        .arg("-o")
        .arg("explicit-name.bin")
        .output()
        .expect("run raria");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(tmp.path().join("explicit-name.bin").is_file());
    assert!(!tmp.path().join("server-name.bin").exists());
}

#[tokio::test]
async fn single_download_sends_configured_user_agent() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/ua"))
        .and(header("user-agent", "phase1-test-agent/1.0"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "2")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/ua"))
        .and(header("user-agent", "phase1-test-agent/1.0"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"ok"))
        .mount(&server)
        .await;

    let tmp = tempdir().expect("tempdir");
    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg(server.uri().to_string() + "/ua")
        .arg("-d")
        .arg(tmp.path())
        .arg("--user-agent")
        .arg("phase1-test-agent/1.0")
        .output()
        .expect("run raria");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn single_download_sends_basic_auth_from_cli_flags() {
    let server = MockServer::start().await;
    let auth_value = format!(
        "Basic {}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"cli-user:cli-pass")
    );

    Mock::given(method("HEAD"))
        .and(path("/cli-auth.bin"))
        .and(header("authorization", auth_value.as_str()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/cli-auth.bin"))
        .and(header("authorization", auth_value.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"auth"))
        .mount(&server)
        .await;

    let tmp = tempdir().expect("tempdir");
    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg(server.uri().to_string() + "/cli-auth.bin")
        .arg("-d")
        .arg(tmp.path())
        .arg("--http-user")
        .arg("cli-user")
        .arg("--http-passwd")
        .arg("cli-pass")
        .output()
        .expect("run raria");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn single_download_presents_client_identity_for_mtls() {
    let tmp = tempdir().expect("tempdir");
    let fixture = spawn_mtls_server(tmp.path()).await;

    let url = fixture.url.clone();
    let ca_path = fixture.ca_path.clone();
    let client_cert_path = fixture.client_cert_path.clone();
    let client_key_path = fixture.client_key_path.clone();
    let out_dir = tmp.path().to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(cargo_bin("raria"))
            .arg("download")
            .arg(&url)
            .arg("-d")
            .arg(&out_dir)
            .arg("--ca-certificate")
            .arg(&ca_path)
            .arg("--certificate")
            .arg(&client_cert_path)
            .arg("--private-key")
            .arg(&client_key_path)
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
        fs::read(tmp.path().join("mtls.bin")).expect("read downloaded file"),
        b"hello"
    );
}

#[tokio::test]
async fn single_download_writes_save_cookies_file() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/cookie.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/cookie.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("set-cookie", "session_id=abc123; Path=/")
                .set_body_bytes(b"done"),
        )
        .mount(&server)
        .await;

    let tmp = tempdir().expect("tempdir");
    let cookies_path = tmp.path().join("cookies.txt");

    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg(server.uri().to_string() + "/cookie.bin")
        .arg("-d")
        .arg(tmp.path())
        .arg("--save-cookies")
        .arg(&cookies_path)
        .output()
        .expect("run raria");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let content = fs::read_to_string(&cookies_path).expect("read saved cookie file");
    assert!(
        content.contains("session_id\tabc123"),
        "expected cookie contents to include session_id, got:\n{content}"
    );
}

#[tokio::test]
async fn single_download_quiet_suppresses_user_facing_output() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/quiet.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/quiet.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"done"))
        .mount(&server)
        .await;

    let tmp = tempdir().expect("tempdir");
    let output = Command::new(cargo_bin("raria"))
        .arg("--quiet")
        .arg("download")
        .arg(server.uri().to_string() + "/quiet.bin")
        .arg("-d")
        .arg(tmp.path())
        .output()
        .expect("run raria");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

#[tokio::test]
async fn single_download_auto_renames_when_target_file_exists() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/file.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/file.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"new!"))
        .mount(&server)
        .await;

    let tmp = tempdir().expect("tempdir");
    let original = tmp.path().join("file.bin");
    fs::write(&original, b"old!").expect("write existing file");

    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg(server.uri().to_string() + "/file.bin")
        .arg("-d")
        .arg(tmp.path())
        .output()
        .expect("run raria");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(fs::read(&original).expect("read original"), b"old!");
    let renamed = tmp.path().join("file.bin.1");
    assert!(renamed.is_file(), "expected auto-renamed file at {}", renamed.display());
    assert_eq!(fs::read(&renamed).expect("read renamed"), b"new!");
}

#[tokio::test]
async fn single_download_uses_netrc_credentials_for_http_auth() {
    let server = MockServer::start().await;
    let auth_value = format!(
        "Basic {}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"netrc-user:netrc-pass")
    );

    Mock::given(method("HEAD"))
        .and(path("/auth.bin"))
        .and(header("authorization", auth_value.as_str()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/auth.bin"))
        .and(header("authorization", auth_value.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"auth"))
        .mount(&server)
        .await;

    let tmp = tempdir().expect("tempdir");
    let netrc_path = tmp.path().join("test.netrc");
    let host = server.address().ip().to_string();
    let mut netrc = fs::File::create(&netrc_path).expect("create netrc");
    writeln!(
        netrc,
        "machine {host}\nlogin netrc-user\npassword netrc-pass\n"
    )
    .expect("write netrc");

    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg(server.uri().to_string() + "/auth.bin")
        .arg("-d")
        .arg(tmp.path())
        .arg("--netrc-path")
        .arg(&netrc_path)
        .output()
        .expect("run raria");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn single_download_no_netrc_disables_netrc_credentials() {
    let server = MockServer::start().await;
    let auth_value = format!(
        "Basic {}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"netrc-user:netrc-pass")
    );

    Mock::given(method("HEAD"))
        .and(path("/auth-disabled.bin"))
        .and(header("authorization", auth_value.as_str()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    let tmp = tempdir().expect("tempdir");
    let netrc_path = tmp.path().join("test.netrc");
    let host = server.address().ip().to_string();
    let mut netrc = fs::File::create(&netrc_path).expect("create netrc");
    writeln!(
        netrc,
        "machine {host}\nlogin netrc-user\npassword netrc-pass\n"
    )
    .expect("write netrc");

    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg(server.uri().to_string() + "/auth-disabled.bin")
        .arg("-d")
        .arg(tmp.path())
        .arg("--netrc-path")
        .arg(&netrc_path)
        .arg("--no-netrc")
        .output()
        .expect("run raria");

    assert!(
        !output.status.success(),
        "download unexpectedly succeeded with --no-netrc\nstdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unexpected argument '--no-netrc'"),
        "no-netrc failure came from missing CLI wiring instead of auth suppression:\n{stderr}"
    );
}

#[tokio::test]
async fn single_download_respects_max_redirect_zero() {
    let target = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/final.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&target)
        .await;

    Mock::given(method("GET"))
        .and(path("/final.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"done"))
        .mount(&target)
        .await;

    let redirector = MockServer::start().await;
    let location = target.uri().to_string() + "/final.bin";
    Mock::given(method("HEAD"))
        .and(path("/redir.bin"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", &location))
        .mount(&redirector)
        .await;
    Mock::given(method("GET"))
        .and(path("/redir.bin"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", &location))
        .mount(&redirector)
        .await;

    let tmp = tempdir().expect("tempdir");
    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg(redirector.uri().to_string() + "/redir.bin")
        .arg("-d")
        .arg(tmp.path())
        .arg("--max-redirect")
        .arg("0")
        .output()
        .expect("run raria");

    assert!(
        !output.status.success(),
        "download unexpectedly succeeded with redirects disabled\nstdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unexpected argument '--max-redirect'"),
        "redirect-limit failure came from missing CLI wiring instead of actual redirect enforcement:\n{stderr}"
    );
}

#[tokio::test]
async fn single_download_sends_custom_header_from_cli() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/header.bin"))
        .and(header("x-phase2-header", "from-cli"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "6")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/header.bin"))
        .and(header("x-phase2-header", "from-cli"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"header"))
        .mount(&server)
        .await;

    let tmp = tempdir().expect("tempdir");
    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg(server.uri().to_string() + "/header.bin")
        .arg("-d")
        .arg(tmp.path())
        .arg("--header")
        .arg("X-Phase2-Header: from-cli")
        .output()
        .expect("run raria");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn single_download_honors_request_timeout() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/slow-timeout.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(2))
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    let tmp = tempdir().expect("tempdir");
    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg(server.uri().to_string() + "/slow-timeout.bin")
        .arg("-d")
        .arg(tmp.path())
        .arg("--timeout")
        .arg("1")
        .output()
        .expect("run raria");

    assert!(
        !output.status.success(),
        "download unexpectedly succeeded despite a 1s timeout\nstdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unexpected argument '--timeout'"),
        "timeout failure came from missing CLI wiring instead of real request timeout behavior:\n{stderr}"
    );
}

#[tokio::test]
async fn single_download_honors_connect_timeout_flag() {
    let tmp = tempdir().expect("tempdir");
    let start = std::time::Instant::now();
    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg("http://10.255.255.1:81/connect-timeout.bin")
        .arg("-d")
        .arg(tmp.path())
        .arg("--connect-timeout")
        .arg("1")
        .output()
        .expect("run raria");

    assert!(
        !output.status.success(),
        "download unexpectedly succeeded against an unroutable address\nstdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unexpected argument '--connect-timeout'"),
        "connect-timeout failure came from missing CLI wiring instead of real connect-timeout behavior:\n{stderr}"
    );

    assert!(
        start.elapsed() < std::time::Duration::from_secs(8),
        "connect-timeout path took too long, elapsed {:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn single_download_conditional_get_skips_download_when_server_reports_not_modified() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/cached.bin"))
        .and(header_exists("if-modified-since"))
        .respond_with(ResponseTemplate::new(304))
        .mount(&server)
        .await;

    Mock::given(method("HEAD"))
        .and(path("/cached.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "8")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/cached.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"download"))
        .mount(&server)
        .await;

    let tmp = tempdir().expect("tempdir");
    let cached = tmp.path().join("cached.bin");
    fs::write(&cached, b"existing").expect("write cached file");

    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg(server.uri().to_string() + "/cached.bin")
        .arg("-d")
        .arg(tmp.path())
        .arg("--conditional-get")
        .arg("--allow-overwrite")
        .output()
        .expect("run raria");

    assert!(
        output.status.success(),
        "conditional-get should treat 304 as success\nstdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(&cached).expect("read cached file"), b"existing");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unexpected argument '--conditional-get'")
            && !stderr.contains("unexpected argument '--allow-overwrite'"),
        "conditional-get path failed from missing CLI wiring instead of 304 handling:\n{stderr}"
    );
}

#[tokio::test]
async fn single_download_conditional_get_is_ignored_without_allow_overwrite() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/fresh.bin"))
        .and(header_exists("if-modified-since"))
        .respond_with(ResponseTemplate::new(304))
        .mount(&server)
        .await;

    Mock::given(method("HEAD"))
        .and(path("/fresh.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/fresh.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"new!"))
        .mount(&server)
        .await;

    let tmp = tempdir().expect("tempdir");
    let out = tmp.path().join("fresh.bin");
    fs::write(&out, b"old!").expect("write old file");

    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg(server.uri().to_string() + "/fresh.bin")
        .arg("-d")
        .arg(tmp.path())
        .arg("--conditional-get")
        .output()
        .expect("run raria");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(&out).expect("read original file"), b"old!");
    assert_eq!(
        fs::read(tmp.path().join("fresh.bin.1")).expect("read auto-renamed file"),
        b"new!"
    );
}

#[tokio::test]
async fn single_download_conditional_get_is_ignored_when_control_file_exists() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/resume.bin"))
        .and(header_exists("if-modified-since"))
        .respond_with(ResponseTemplate::new(304))
        .mount(&server)
        .await;

    Mock::given(method("HEAD"))
        .and(path("/resume.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/resume.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"next"))
        .mount(&server)
        .await;

    let tmp = tempdir().expect("tempdir");
    let out = tmp.path().join("resume.bin");
    fs::write(&out, b"old!").expect("write old file");
    fs::write(tmp.path().join("resume.bin.aria2"), b"control").expect("write control file");

    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg(server.uri().to_string() + "/resume.bin")
        .arg("-d")
        .arg(tmp.path())
        .arg("--conditional-get")
        .arg("--allow-overwrite")
        .output()
        .expect("run raria");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(&out).expect("read downloaded file"), b"next");
}

#[tokio::test]
async fn single_download_allow_overwrite_replaces_existing_file_without_tail_bytes() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/overwrite.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/overwrite.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"new!"))
        .mount(&server)
        .await;

    let tmp = tempdir().expect("tempdir");
    let out = tmp.path().join("overwrite.bin");
    fs::write(&out, b"old-contents").expect("write existing file");

    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg(server.uri().to_string() + "/overwrite.bin")
        .arg("-d")
        .arg(tmp.path())
        .arg("--allow-overwrite")
        .output()
        .expect("run raria");

    assert!(
        output.status.success(),
        "allow-overwrite should succeed\nstdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(&out).expect("read overwritten file"), b"new!");
}

#[tokio::test]
async fn single_download_continue_resumes_from_existing_file_length() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/continue.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "8")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/continue.bin"))
        .and(header("range", "bytes=4-"))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(b"5678"))
        .mount(&server)
        .await;

    let tmp = tempdir().expect("tempdir");
    let out = tmp.path().join("continue.bin");
    fs::write(&out, b"1234").expect("write partial file");

    let output = Command::new(cargo_bin("raria"))
        .arg("download")
        .arg(server.uri().to_string() + "/continue.bin")
        .arg("-d")
        .arg(tmp.path())
        .arg("-x")
        .arg("1")
        .arg("--continue")
        .output()
        .expect("run raria");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(&out).expect("read resumed file"), b"12345678");
}

#[tokio::test]
async fn single_download_supports_plain_ftp_urls() {
    let fixture = spawn_ftp_server("cli-user", "cli-pass", "/pub/file.bin", b"hello-from-ftp").await;
    let tmp = tempdir().expect("tempdir");
    let download_dir = tmp.path().to_path_buf();
    let url = format!(
        "ftp://cli-user:cli-pass@127.0.0.1:{}/pub/file.bin",
        fixture.port
    );

    let output = tokio::task::spawn_blocking(move || {
        Command::new(cargo_bin("raria"))
            .arg("download")
            .arg(url)
            .arg("-d")
            .arg(&download_dir)
            .output()
            .expect("run raria")
    })
    .await
    .expect("join blocking download");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(tmp.path().join("file.bin")).expect("read ftp download"),
        b"hello-from-ftp"
    );
}

#[tokio::test]
async fn single_download_supports_plain_ftp_urls_through_socks5_proxy() {
    let fixture = spawn_ftp_server("proxy-user", "proxy-pass", "/pub/proxy.bin", b"ftp-via-proxy").await;
    let connect_count = Arc::new(AtomicUsize::new(0));
    let proxy = spawn_socks5_proxy(Arc::clone(&connect_count)).await;
    let tmp = tempdir().expect("tempdir");
    let download_dir = tmp.path().to_path_buf();
    let url = format!(
        "ftp://proxy-user:proxy-pass@127.0.0.1:{}/pub/proxy.bin",
        fixture.port
    );

    let output = tokio::task::spawn_blocking(move || {
        Command::new(cargo_bin("raria"))
            .arg("download")
            .arg(url)
            .arg("-d")
            .arg(&download_dir)
            .arg("--all-proxy")
            .arg(&proxy)
            .output()
            .expect("run raria")
    })
    .await
    .expect("join blocking proxied download");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(tmp.path().join("proxy.bin")).expect("read proxied ftp download"),
        b"ftp-via-proxy"
    );
    assert!(
        connect_count.load(Ordering::SeqCst) >= 2,
        "expected proxied FTP control/data traffic"
    );
}

#[tokio::test]
async fn single_download_supports_explicit_ftps_with_custom_ca() {
    let fixture = spawn_explicit_ftps_server(
        "ftps-user",
        "ftps-pass",
        "/secure/file.bin",
        b"hello-ftps-world",
    )
    .await;
    let tmp = tempdir().expect("tempdir");
    let download_dir = tmp.path().to_path_buf();
    let url = format!(
        "ftps://ftps-user:ftps-pass@127.0.0.1:{}/secure/file.bin",
        fixture.port
    );
    let ca_path = fixture.ca_path.clone();

    let output = tokio::task::spawn_blocking(move || {
        Command::new(cargo_bin("raria"))
            .arg("download")
            .arg(url)
            .arg("-d")
            .arg(&download_dir)
            .arg("--ca-certificate")
            .arg(&ca_path)
            .output()
            .expect("run raria")
    })
    .await
    .expect("join blocking ftps download");

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(tmp.path().join("file.bin")).expect("read ftps download"),
        b"hello-ftps-world"
    );
}
