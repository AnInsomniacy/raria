use raria_http::backend::{HttpBackend, HttpBackendConfig};
use raria_range::backend::{ByteSourceBackend, OpenContext, ProbeContext};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{RootCertStore, ServerConfig};
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::{NamedTempFile, tempdir};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn http_backend_loads_cookie_file_into_requests() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/cookie"))
        .and(header("cookie", "session_id=abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"cookie-ok"))
        .mount(&server)
        .await;

    let mut cookie_file = NamedTempFile::new().expect("cookie file");
    writeln!(
        cookie_file,
        "127.0.0.1\tFALSE\t/\tFALSE\t0\tsession_id\tabc123"
    )
    .expect("write cookie file");

    let backend = HttpBackend::with_config(&HttpBackendConfig {
        cookie_file: Some(cookie_file.path().to_path_buf()),
        check_certificate: true,
        ..Default::default()
    })
    .expect("backend");

    let url = format!("{}/cookie", server.uri()).parse().expect("url");
    let mut stream = backend
        .open_from(&url, 0, &OpenContext::default())
        .await
        .expect("open stream");

    let mut body = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut body)
        .await
        .expect("read body");
    assert_eq!(body, b"cookie-ok");
}

#[tokio::test]
async fn http_backend_bypasses_invalid_proxy_for_no_proxy_host() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/probe"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "5")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    let backend = HttpBackend::with_config(&HttpBackendConfig {
        all_proxy: Some("http://127.0.0.1:9".into()),
        no_proxy: Some("127.0.0.1,localhost".into()),
        check_certificate: true,
        ..Default::default()
    })
    .expect("backend");

    let url = format!("{}/probe", server.uri()).parse().expect("url");
    let probe = backend
        .probe(&url, &ProbeContext::default())
        .await
        .expect("probe should bypass invalid proxy");

    assert_eq!(probe.size, Some(5));
}

#[tokio::test]
async fn http_backend_honors_redirect_limit() {
    let target = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/final"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&target)
        .await;

    let redirector = MockServer::start().await;
    let location = target.uri().to_string() + "/final";
    Mock::given(method("HEAD"))
        .and(path("/redirect"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", &location))
        .mount(&redirector)
        .await;

    let backend = HttpBackend::with_config(&HttpBackendConfig {
        max_redirects: Some(0),
        check_certificate: true,
        ..Default::default()
    })
    .expect("backend");

    let url = format!("{}/redirect", redirector.uri()).parse().expect("url");
    let error = backend
        .probe(&url, &ProbeContext::default())
        .await
        .expect_err("redirect should fail when redirects are disabled");
    assert!(
        error.to_string().contains("HTTP HEAD request failed")
            || error.to_string().contains("redirect"),
        "unexpected redirect failure: {error}"
    );
}

async fn spawn_socks5_proxy(counter: Arc<AtomicUsize>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind socks5");
    let addr = listener.local_addr().expect("socks5 addr");
    tokio::spawn(async move {
        loop {
            let Ok((mut downstream, _)) = listener.accept().await else {
                break;
            };
            let counter = Arc::clone(&counter);
            tokio::spawn(async move {
                counter.fetch_add(1, Ordering::SeqCst);

                let mut greeting = [0u8; 2];
                downstream.read_exact(&mut greeting).await.expect("read socks greeting");
                let methods_len = greeting[1] as usize;
                let mut methods = vec![0u8; methods_len];
                downstream.read_exact(&mut methods).await.expect("read socks methods");
                downstream
                    .write_all(&[0x05, 0x00])
                    .await
                    .expect("write socks method select");

                let mut req = [0u8; 4];
                downstream.read_exact(&mut req).await.expect("read socks request header");
                assert_eq!(req[0], 0x05, "socks version");
                assert_eq!(req[1], 0x01, "socks command should be CONNECT");

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

async fn spawn_connect_proxy(counter: Arc<AtomicUsize>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind connect proxy");
    let addr = listener.local_addr().expect("connect proxy addr");
    tokio::spawn(async move {
        loop {
            let Ok((mut downstream, _)) = listener.accept().await else {
                break;
            };
            let counter = Arc::clone(&counter);
            tokio::spawn(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 4096];
                let n = downstream.read(&mut buf).await.expect("read connect request");
                let req = String::from_utf8_lossy(&buf[..n]);
                let target = req
                    .lines()
                    .next()
                    .and_then(|line| line.strip_prefix("CONNECT "))
                    .and_then(|rest| rest.split_whitespace().next())
                    .expect("connect target");
                let mut upstream = TcpStream::connect(target).await.expect("connect upstream");
                downstream
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await
                    .expect("write connect response");
                tokio::io::copy_bidirectional(&mut downstream, &mut upstream)
                    .await
                    .expect("proxy relay");
            });
        }
    });

    format!("http://{}", addr)
}

async fn spawn_http_proxy(counter: Arc<AtomicUsize>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind http proxy");
    let addr = listener.local_addr().expect("http proxy addr");
    tokio::spawn(async move {
        loop {
            let Ok((mut downstream, _)) = listener.accept().await else {
                break;
            };
            let counter = Arc::clone(&counter);
            tokio::spawn(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 4096];
                let n = downstream.read(&mut buf).await.expect("read proxy request");
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                assert!(
                    req.starts_with("HEAD http://"),
                    "expected absolute-form HEAD request via http proxy, got: {req}"
                );
                downstream
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 5\r\naccept-ranges: bytes\r\nconnection: close\r\n\r\n")
                    .await
                    .expect("write proxy response");
                downstream.flush().await.expect("flush downstream");
            });
        }
    });

    format!("http://{}", addr)
}

struct MtlsFixture {
    url: url::Url,
    ca_pem: NamedTempFile,
    client_cert_pem: NamedTempFile,
    client_key_pem: NamedTempFile,
}

async fn spawn_mtls_server() -> MtlsFixture {
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
    let mut server_params = CertificateParams::new(vec!["localhost".into()]).expect("server params");
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

    let temp = tempdir().expect("tempdir");
    let ca_pem_path = temp.path().join("ca.pem");
    let client_cert_path = temp.path().join("client.pem");
    let client_key_path = temp.path().join("client.key");
    std::fs::write(&ca_pem_path, ca_cert.pem()).expect("write ca pem");
    std::fs::write(&client_cert_path, client_cert.pem()).expect("write client cert");
    std::fs::write(&client_key_path, client_key.serialize_pem()).expect("write client key");

    let mut roots = RootCertStore::empty();
    roots.add(ca_cert.der().clone()).expect("add ca to roots");
    let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .build()
        .expect("client verifier");

    let server_config = ServerConfig::builder_with_provider(provider.clone())
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
        let (stream, _) = listener.accept().await.expect("accept tcp");
        let mut tls = acceptor.accept(stream).await.expect("accept tls");
        let mut buf = [0u8; 2048];
        let n = tls.read(&mut buf).await.expect("read request");
        let req = String::from_utf8_lossy(&buf[..n]);
        let response = if req.starts_with("HEAD /mtls ") {
            "HTTP/1.1 200 OK\r\ncontent-length: 5\r\naccept-ranges: bytes\r\n\r\n".to_string()
        } else if req.starts_with("GET /mtls ") {
            "HTTP/1.1 200 OK\r\ncontent-length: 5\r\n\r\nhello".to_string()
        } else {
            "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n".to_string()
        };
        tls.write_all(response.as_bytes()).await.expect("write response");
        tls.flush().await.expect("flush response");
    });

    let mut ca_pem = NamedTempFile::new().expect("ca temp");
    let mut client_cert_pem = NamedTempFile::new().expect("client cert temp");
    let mut client_key_pem = NamedTempFile::new().expect("client key temp");
    std::io::Write::write_all(&mut ca_pem, ca_cert.pem().as_bytes()).expect("copy ca pem");
    std::io::Write::write_all(&mut client_cert_pem, client_cert.pem().as_bytes()).expect("copy client cert");
    std::io::Write::write_all(&mut client_key_pem, client_key.serialize_pem().as_bytes()).expect("copy client key");

    MtlsFixture {
        url: format!("https://localhost:{}/mtls", addr.port()).parse().expect("mtls url"),
        ca_pem,
        client_cert_pem,
        client_key_pem,
    }
}

#[tokio::test]
async fn http_backend_presents_client_identity_for_mtls() {
    let fixture = spawn_mtls_server().await;

    let backend = HttpBackend::with_config(&HttpBackendConfig {
        check_certificate: true,
        ca_certificate: Some(fixture.ca_pem.path().to_path_buf()),
        client_certificate: Some(fixture.client_cert_pem.path().to_path_buf()),
        client_private_key: Some(fixture.client_key_pem.path().to_path_buf()),
        ..Default::default()
    })
    .expect("backend");

    let probe = backend
        .probe(&fixture.url, &ProbeContext::default())
        .await
        .expect("mtls probe should succeed");
    assert_eq!(probe.size, Some(5));
}

#[tokio::test]
async fn http_backend_can_disable_certificate_verification() {
    let fixture = spawn_mtls_server().await;

    let backend = HttpBackend::with_config(&HttpBackendConfig {
        check_certificate: false,
        client_certificate: Some(fixture.client_cert_pem.path().to_path_buf()),
        client_private_key: Some(fixture.client_key_pem.path().to_path_buf()),
        ..Default::default()
    })
    .expect("backend");

    let probe = backend
        .probe(&fixture.url, &ProbeContext::default())
        .await
        .expect("probe should succeed when certificate verification is disabled");
    assert_eq!(probe.size, Some(5));
}

#[tokio::test]
async fn http_backend_routes_requests_through_socks5_proxy() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/probe"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "5")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    let counter = Arc::new(AtomicUsize::new(0));
    let proxy = spawn_socks5_proxy(Arc::clone(&counter)).await;

    let backend = HttpBackend::with_config(&HttpBackendConfig {
        all_proxy: Some(proxy),
        check_certificate: true,
        ..Default::default()
    })
    .expect("backend");

    let url = format!("{}/probe", server.uri()).parse().expect("url");
    let probe = backend
        .probe(&url, &ProbeContext::default())
        .await
        .expect("probe through socks5");

    assert_eq!(probe.size, Some(5));
    assert!(
        counter.load(Ordering::SeqCst) > 0,
        "expected at least one proxied connection"
    );
}

#[tokio::test]
async fn http_backend_routes_requests_through_http_proxy() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/probe"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "5")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    let counter = Arc::new(AtomicUsize::new(0));
    let proxy = spawn_http_proxy(Arc::clone(&counter)).await;

    let backend = HttpBackend::with_config(&HttpBackendConfig {
        http_proxy: Some(proxy),
        check_certificate: true,
        ..Default::default()
    })
    .expect("backend");

    let url = format!("{}/probe", server.uri()).parse().expect("url");
    let probe = backend
        .probe(&url, &ProbeContext::default())
        .await
        .expect("probe through http proxy");

    assert_eq!(probe.size, Some(5));
    assert!(
        counter.load(Ordering::SeqCst) > 0,
        "expected at least one HTTP proxy connection"
    );
}

#[tokio::test]
async fn http_backend_routes_https_requests_through_connect_proxy() {
    let fixture = spawn_mtls_server().await;
    let counter = Arc::new(AtomicUsize::new(0));
    let proxy = spawn_connect_proxy(Arc::clone(&counter)).await;

    let backend = HttpBackend::with_config(&HttpBackendConfig {
        https_proxy: Some(proxy),
        check_certificate: true,
        ca_certificate: Some(fixture.ca_pem.path().to_path_buf()),
        client_certificate: Some(fixture.client_cert_pem.path().to_path_buf()),
        client_private_key: Some(fixture.client_key_pem.path().to_path_buf()),
        ..Default::default()
    })
    .expect("backend");

    let probe = backend
        .probe(&fixture.url, &ProbeContext::default())
        .await
        .expect("probe through https proxy");
    assert_eq!(probe.size, Some(5));
    assert!(
        counter.load(Ordering::SeqCst) > 0,
        "expected at least one CONNECT proxy connection"
    );
}

#[tokio::test]
async fn http_backend_retries_with_digest_auth_when_challenged() {
    let digest_challenge = r#"Digest realm="raria", qop="auth", algorithm=MD5, nonce="abcdef123456", opaque="opaque-token""#;
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind digest listener");
    let addr = listener.local_addr().expect("digest addr");
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept digest");
        for _ in 0..2 {
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).await.expect("read digest request");
            let req = String::from_utf8_lossy(&buf[..n]);
            let req_lower = req.to_ascii_lowercase();
            let response = if req_lower.contains("authorization: digest ") {
                "HTTP/1.1 200 OK\r\ncontent-length: 5\r\naccept-ranges: bytes\r\nconnection: close\r\n\r\n".to_string()
            } else {
                format!(
                    "HTTP/1.1 401 Unauthorized\r\nwww-authenticate: {digest_challenge}\r\ncontent-length: 0\r\nconnection: keep-alive\r\n\r\n"
                )
            };
            stream.write_all(response.as_bytes()).await.expect("write digest response");
            stream.flush().await.expect("flush digest response");
            if req_lower.contains("authorization: digest ") {
                break;
            }
        }
    });

    let backend = HttpBackend::with_config(&HttpBackendConfig {
        check_certificate: true,
        ..Default::default()
    })
    .expect("backend");

    let url = format!("http://127.0.0.1:{}/digest-head", addr.port()).parse().expect("url");
    let probe = backend
        .probe(
            &url,
            &ProbeContext {
                auth: Some(raria_range::backend::Credentials {
                    username: "user".into(),
                    password: "pass".into(),
                }),
                ..Default::default()
            },
        )
        .await
        .expect("digest probe should succeed");
    assert_eq!(probe.size, Some(5));
}
