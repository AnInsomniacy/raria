use std::process::Command;
use std::{fs, io::Write};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{RootCertStore, ServerConfig};
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use wiremock::matchers::{header, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn cargo_bin(name: &str) -> String {
    std::env::var(format!("CARGO_BIN_EXE_{name}")).expect("cargo should provide binary path")
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
