use std::sync::Arc;

use raria_ftp::backend::{FtpBackend, FtpBackendConfig};
use raria_range::backend::{ByteSourceBackend, OpenContext, ProbeContext};
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::ServerConfig;
use tempfile::NamedTempFile;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, server::TlsStream};

fn is_disconnect(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionAborted
    )
}

struct FtpsFixture {
    port: u16,
    ca_pem: NamedTempFile,
}

async fn spawn_explicit_ftps_server(
    expected_user: &'static str,
    expected_password: &'static str,
    file_path: &'static str,
    file_data: &'static [u8],
) -> FtpsFixture {
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

    let ca_pem = NamedTempFile::new().expect("ca pem");
    std::fs::write(ca_pem.path(), ca_cert.pem()).expect("write ca pem");

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![server_cert.der().clone()],
            PrivateKeyDer::from(PrivatePkcs8KeyDer::from(server_key.serialize_der())),
        )
        .expect("server config");
    let acceptor = TlsAcceptor::from(Arc::new(server_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ftps listener");
    let port = listener.local_addr().expect("ftps addr").port();

    tokio::spawn(async move {
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

    FtpsFixture { port, ca_pem }
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
    write_reply(reader.get_mut(), 220, "raria explicit ftps test server")
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
            write_reply(reader.get_mut(), 234, "AUTH TLS accepted")
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
            write_reply(reader.get_mut(), 221, "goodbye")
                .await
                .expect("write quit");
        }
        other => panic!("unexpected plain FTPS command before AUTH TLS: {other} {arg}"),
    }
}

async fn handle_ftps_tls_session(
    stream: TlsStream<TcpStream>,
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
                write_reply(reader.get_mut(), 331, "password required")
                    .await
                    .expect("write user reply");
            }
            "PASS" => {
                authenticated = seen_user.as_deref() == Some(expected_user) && arg == expected_password;
                let code = if authenticated { 230 } else { 530 };
                write_reply(
                    reader.get_mut(),
                    code,
                    if authenticated { "login successful" } else { "login incorrect" },
                )
                .await
                .expect("write pass reply");
            }
            "PBSZ" => {
                assert_eq!(arg, "0");
                write_reply(reader.get_mut(), 200, "pbsz=0")
                    .await
                    .expect("write pbsz");
            }
            "PROT" => {
                assert_eq!(arg, "P");
                protect_private = true;
                write_reply(reader.get_mut(), 200, "protection private")
                    .await
                    .expect("write prot");
            }
            "TYPE" => {
                assert!(authenticated, "TYPE before authentication");
                write_reply(reader.get_mut(), 200, "type set to I")
                    .await
                    .expect("write type");
            }
            "SIZE" => {
                assert!(authenticated, "SIZE before authentication");
                assert_eq!(arg, file_path);
                write_reply(reader.get_mut(), 213, &file_data.len().to_string())
                    .await
                    .expect("write size");
            }
            "REST" => {
                pending_offset = arg.parse::<usize>().expect("parse rest");
                write_reply(reader.get_mut(), 350, "restart position accepted")
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
                write_reply(
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
                write_reply(reader.get_mut(), 150, "opening protected data connection")
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
                write_reply(reader.get_mut(), 226, "transfer complete")
                    .await
                    .expect("write complete");
            }
            "QUIT" => {
                write_reply(reader.get_mut(), 221, "goodbye")
                    .await
                    .expect("write quit");
                break;
            }
            other => panic!("unexpected FTPS command: {other} {arg}"),
        }
    }
}

async fn write_reply<W>(writer: &mut W, code: u16, message: &str) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer
        .write_all(format!("{code} {message}\r\n").as_bytes())
        .await?;
    writer.flush().await
}

#[tokio::test]
async fn ftps_backend_supports_explicit_tls_with_custom_ca() {
    let fixture = spawn_explicit_ftps_server(
        "ftps-user",
        "ftps-pass",
        "/secure/file.bin",
        b"hello-ftps-world",
    )
    .await;

    let backend = FtpBackend::with_config(FtpBackendConfig {
        ca_certificate: Some(fixture.ca_pem.path().to_path_buf()),
        ..Default::default()
    });
    let url = format!("ftps://ftps-user:ftps-pass@127.0.0.1:{}/secure/file.bin", fixture.port)
        .parse()
        .expect("ftps url");

    let probe = backend
        .probe(&url, &ProbeContext::default())
        .await
        .expect("probe should succeed over explicit FTPS");
    assert_eq!(probe.size, Some(16));

    let mut stream = backend
        .open_from(&url, 6, &OpenContext::default())
        .await
        .expect("open_from should succeed over explicit FTPS");
    let mut body = Vec::new();
    stream.read_to_end(&mut body).await.expect("read ftps body");
    assert_eq!(body, b"ftps-world");
}
