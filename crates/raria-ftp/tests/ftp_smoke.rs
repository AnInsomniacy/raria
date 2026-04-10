use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use raria_ftp::backend::{FtpBackend, FtpBackendConfig};
use raria_range::backend::{ByteSourceBackend, OpenContext, ProbeContext};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

struct FtpFixture {
    port: u16,
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
    let port = listener.local_addr().expect("control addr").port();

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

    write_reply(reader.get_mut(), 220, "raria ftp test server")
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
                write_reply(reader.get_mut(), 331, "password required")
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
                write_reply(reader.get_mut(), code, message)
                    .await
                    .expect("write pass reply");
            }
            "TYPE" => {
                assert!(authenticated, "TYPE before authentication");
                write_reply(reader.get_mut(), 200, "type set to I")
                    .await
                    .expect("write type reply");
            }
            "SIZE" => {
                assert!(authenticated, "SIZE before authentication");
                assert_eq!(arg, file_path, "unexpected SIZE path");
                write_reply(reader.get_mut(), 213, &file_data.len().to_string())
                    .await
                    .expect("write size reply");
            }
            "REST" => {
                assert!(authenticated, "REST before authentication");
                pending_offset = arg.parse::<usize>().expect("parse REST offset");
                write_reply(reader.get_mut(), 350, "restart position accepted")
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
                let port_hi = addr.port() / 256;
                let port_lo = addr.port() % 256;
                let reply = format!(
                    "Entering Passive Mode ({},{},{},{},{},{})",
                    octets[0], octets[1], octets[2], octets[3], port_hi, port_lo
                );
                data_listener = Some(listener);
                write_reply(reader.get_mut(), 227, &reply)
                    .await
                    .expect("write pasv reply");
            }
            "RETR" => {
                assert!(authenticated, "RETR before authentication");
                assert_eq!(arg, file_path, "unexpected RETR path");
                let listener = data_listener.take().expect("RETR without PASV");
                write_reply(reader.get_mut(), 150, "opening data connection")
                    .await
                    .expect("write retr start");
                let (mut data_stream, _) = listener.accept().await.expect("accept data conn");
                data_stream
                    .write_all(&file_data[pending_offset.min(file_data.len())..])
                    .await
                    .expect("write data");
                data_stream.shutdown().await.expect("shutdown data");
                pending_offset = 0;
                write_reply(reader.get_mut(), 226, "transfer complete")
                    .await
                    .expect("write retr done");
            }
            "QUIT" => {
                write_reply(reader.get_mut(), 221, "goodbye")
                    .await
                    .expect("write quit reply");
                break;
            }
            "PBSZ" => {
                write_reply(reader.get_mut(), 200, "pbsz=0")
                    .await
                    .expect("write pbsz reply");
            }
            "PROT" => {
                write_reply(reader.get_mut(), 200, "protection level set")
                    .await
                    .expect("write prot reply");
            }
            other => panic!("unexpected FTP command: {other} {arg}"),
        }
    }
}

async fn write_reply(stream: &mut TcpStream, code: u16, message: &str) -> std::io::Result<()> {
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

#[tokio::test]
async fn ftp_backend_probes_size_and_resumes_with_rest() {
    let fixture = spawn_ftp_server("ftp-user", "ftp-pass", "/pub/file.bin", b"hello-ftp-world").await;
    let backend = FtpBackend::new();
    let url = format!("ftp://ftp-user:ftp-pass@127.0.0.1:{}/pub/file.bin", fixture.port)
        .parse()
        .expect("ftp url");

    let probe = backend
        .probe(&url, &ProbeContext::default())
        .await
        .expect("probe should succeed");
    assert_eq!(probe.size, Some(15));
    assert!(probe.supports_range);

    let mut stream = backend
        .open_from(&url, 6, &OpenContext::default())
        .await
        .expect("open_from should succeed");
    let mut body = Vec::new();
    stream.read_to_end(&mut body).await.expect("read ftp stream");
    assert_eq!(body, b"ftp-world");
}

#[tokio::test]
async fn ftp_backend_uses_socks5_proxy_when_configured() {
    let fixture = spawn_ftp_server("proxy-user", "proxy-pass", "/pub/proxy.bin", b"proxy-body").await;
    let connect_count = Arc::new(AtomicUsize::new(0));
    let proxy = spawn_socks5_proxy(Arc::clone(&connect_count)).await;

    let backend = FtpBackend::with_config(FtpBackendConfig {
        all_proxy: Some(proxy),
        no_proxy: None,
        ..Default::default()
    });
    let url = format!(
        "ftp://proxy-user:proxy-pass@127.0.0.1:{}/pub/proxy.bin",
        fixture.port
    )
    .parse()
    .expect("ftp url");

    let probe = backend
        .probe(&url, &ProbeContext::default())
        .await
        .expect("probe should succeed through socks5");
    assert_eq!(probe.size, Some(10));

    let mut stream = backend
        .open_from(&url, 0, &OpenContext::default())
        .await
        .expect("open_from should succeed through socks5");
    let mut body = Vec::new();
    stream.read_to_end(&mut body).await.expect("read ftp stream");
    assert_eq!(body, b"proxy-body");
    assert!(
        connect_count.load(Ordering::SeqCst) >= 2,
        "expected control/data traffic to traverse the proxy at least twice"
    );
}
