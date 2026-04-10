use std::process::Command;
use std::{fs, io::Write};
use tempfile::tempdir;
use wiremock::matchers::{header, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn cargo_bin(name: &str) -> String {
    std::env::var(format!("CARGO_BIN_EXE_{name}")).expect("cargo should provide binary path")
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
