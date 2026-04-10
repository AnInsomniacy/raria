use raria_http::backend::{HttpBackend, HttpBackendConfig};
use raria_range::backend::{ByteSourceBackend, OpenContext, ProbeContext};
use std::io::Write;
use tempfile::NamedTempFile;
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
