// raria-http: HTTP/HTTPS backend implementing ByteSourceBackend.
//
// Uses reqwest for all HTTP operations. Supports:
// - HEAD probing for size/range/etag detection
// - Range-based offset downloads
// - Cookie persistence via reqwest_cookie_store

use anyhow::{Context, Result};
use async_trait::async_trait;
use digest_auth::{AuthContext, HttpMethod as DigestHttpMethod};
use futures::TryStreamExt;
use netrc::Netrc;
use reqwest::StatusCode;
use raria_range::backend::{
    ByteSourceBackend, ByteStream, Credentials, FileProbe, OpenContext, ProbeContext,
};
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, CONTENT_LENGTH, CONTENT_TYPE, ETAG, LAST_MODIFIED, RANGE,
};
use reqwest::Client;
use reqwest::redirect::Policy;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::io::StreamReader;
use tracing::debug;
use url::Url;

type NetrcAuthMap = Arc<HashMap<String, (String, String)>>;

/// Configuration for the HTTP backend.
///
/// Matches aria2's HTTP-related options: proxy, TLS, user-agent, cookies.
#[derive(Debug, Clone, Default)]
pub struct HttpBackendConfig {
    /// Proxy URL for all protocols.
    pub all_proxy: Option<String>,
    /// Proxy URL for HTTP specifically (overrides all_proxy).
    pub http_proxy: Option<String>,
    /// Proxy URL for HTTPS specifically (overrides all_proxy).
    pub https_proxy: Option<String>,
    /// Comma-separated no-proxy domains.
    pub no_proxy: Option<String>,
    /// Whether to verify TLS certificates (default: true).
    pub check_certificate: bool,
    /// Path to custom CA certificate file.
    pub ca_certificate: Option<std::path::PathBuf>,
    /// Path to client certificate chain for mTLS.
    pub client_certificate: Option<std::path::PathBuf>,
    /// Path to client private key for mTLS.
    pub client_private_key: Option<std::path::PathBuf>,
    /// Custom user-agent string.
    pub user_agent: Option<String>,
    /// Path to Netscape-format cookie file (aria2: --load-cookies).
    pub cookie_file: Option<std::path::PathBuf>,
    /// Path to Netscape-format cookie file for persistence (aria2: --save-cookies).
    pub save_cookie_file: Option<std::path::PathBuf>,
    /// Maximum number of redirects to follow. `Some(0)` disables redirects.
    pub max_redirects: Option<usize>,
    /// Connection establishment timeout in seconds.
    pub connect_timeout: Option<u64>,
    /// Path to a netrc file for host-based credentials.
    pub netrc_path: Option<std::path::PathBuf>,
    /// Disable loading any netrc credentials.
    pub no_netrc: bool,
}

/// HTTP/HTTPS download backend.
#[derive(Debug, Clone)]
pub struct HttpBackend {
    client: Client,
    netrc_auth: Option<NetrcAuthMap>,
    cookie_store: Option<Arc<reqwest_cookie_store::CookieStoreMutex>>,
    save_cookie_file: Option<std::path::PathBuf>,
}

impl HttpBackend {
    /// Create a new HTTP backend with default settings.
    pub fn new() -> Result<Self> {
        Self::with_config(&HttpBackendConfig {
            check_certificate: true,
            ..Default::default()
        })
    }

    /// Create a new HTTP backend with the given configuration.
    pub fn with_config(config: &HttpBackendConfig) -> Result<Self> {
        let _ = rustls::crypto::aws_lc_rs::default_provider()
            .install_default();

        let ua = config
            .user_agent
            .as_deref()
            .unwrap_or(concat!("raria/", env!("CARGO_PKG_VERSION")));

        let mut builder = Client::builder()
            .use_rustls_tls()
            .user_agent(ua)
            .danger_accept_invalid_certs(!config.check_certificate);

        if let Some(limit) = config.max_redirects {
            builder = builder.redirect(Policy::limited(limit));
        }
        if let Some(connect_timeout) = config.connect_timeout {
            builder = builder.connect_timeout(std::time::Duration::from_secs(connect_timeout));
        }

        // Build the no_proxy exclusion list (applied to each proxy).
        let no_proxy_list = config
            .no_proxy
            .as_ref()
            .and_then(|s| reqwest::NoProxy::from_string(s));

        // Configure proxy.
        if let Some(ref proxy_url) = config.all_proxy {
            let mut proxy = reqwest::Proxy::all(proxy_url)
                .context("invalid all-proxy URL")?;
            if let Some(ref np) = no_proxy_list {
                proxy = proxy.no_proxy(Some(np.clone()));
            }
            builder = builder.proxy(proxy);
        }
        if let Some(ref proxy_url) = config.http_proxy {
            let mut proxy = reqwest::Proxy::http(proxy_url)
                .context("invalid http-proxy URL")?;
            if let Some(ref np) = no_proxy_list {
                proxy = proxy.no_proxy(Some(np.clone()));
            }
            builder = builder.proxy(proxy);
        }
        if let Some(ref proxy_url) = config.https_proxy {
            let mut proxy = reqwest::Proxy::https(proxy_url)
                .context("invalid https-proxy URL")?;
            if let Some(ref np) = no_proxy_list {
                proxy = proxy.no_proxy(Some(np.clone()));
            }
            builder = builder.proxy(proxy);
        }

        // Configure custom CA certificate.
        if let Some(ref ca_path) = config.ca_certificate {
            let cert_data = std::fs::read(ca_path)
                .with_context(|| format!("failed to read CA cert: {}", ca_path.display()))?;
            let cert = reqwest::Certificate::from_pem(&cert_data)
                .context("failed to parse CA certificate")?;
            builder = builder.add_root_certificate(cert);
        }

        // Configure client identity for mTLS.
        match (&config.client_certificate, &config.client_private_key) {
            (Some(cert_path), Some(key_path)) => {
                let cert = std::fs::read(cert_path)
                    .with_context(|| format!("failed to read client certificate: {}", cert_path.display()))?;
                let key = std::fs::read(key_path)
                    .with_context(|| format!("failed to read client private key: {}", key_path.display()))?;
                let identity = reqwest::Identity::from_pem(&[cert, key].concat())
                    .context("failed to parse client identity")?;
                builder = builder.identity(identity);
            }
            (Some(_), None) | (None, Some(_)) => {
                anyhow::bail!("both certificate and private-key must be set for client TLS identity");
            }
            (None, None) => {}
        }

        // Load cookies from Netscape cookie file; keep store for persistence.
        let cookie_store = if config.cookie_file.is_some() || config.save_cookie_file.is_some() {
            let store = if let Some(ref cookie_path) = config.cookie_file {
                crate::cookies::load_cookie_store(cookie_path)
                    .with_context(|| format!("failed to load cookies: {}", cookie_path.display()))?
            } else {
                reqwest_cookie_store::CookieStore::default()
            };
            let store = reqwest_cookie_store::CookieStoreMutex::new(store);
            Some(Arc::new(store))
        } else {
            None
        };
        if let Some(ref store) = cookie_store {
            builder = builder.cookie_provider(Arc::clone(store));
        }

        let client = builder.build().context("failed to build reqwest client")?;
        let netrc_auth = if config.no_netrc {
            None
        } else {
            load_netrc_auth(config.netrc_path.as_deref())?
        };

        Ok(Self {
            client,
            netrc_auth,
            cookie_store,
            save_cookie_file: config.save_cookie_file.clone(),
        })
    }

    /// Create a new HTTP backend with a custom client.
    pub fn with_client(client: Client) -> Self {
        Self {
            client,
            netrc_auth: None,
            cookie_store: None,
            save_cookie_file: None,
        }
    }

    fn build_headers(ctx: &ProbeContext) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in &ctx.headers {
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                headers.insert(name, value);
            }
        }
        headers
    }

    fn netrc_credentials_for(&self, uri: &Url) -> Option<(String, String)> {
        let host = uri.host_str()?;
        self.netrc_auth
            .as_ref()
            .and_then(|entries| entries.get(host))
            .cloned()
    }

    async fn send_with_optional_digest<F>(
        &self,
        uri: &Url,
        creds: Option<&Credentials>,
        method: DigestHttpMethod<'static>,
        build: F,
    ) -> Result<reqwest::Response>
    where
        F: Fn(Option<&str>) -> reqwest::RequestBuilder,
    {
        let response = build(None).send().await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return Ok(response);
        }

        let Some(creds) = creds else {
            return Ok(response);
        };

        let challenge = response
            .headers()
            .get(reqwest::header::WWW_AUTHENTICATE)
            .and_then(|v| v.to_str().ok())
            .filter(|header| header.to_ascii_lowercase().starts_with("digest"))
            .map(str::to_owned);

        let Some(challenge) = challenge else {
            return Ok(response);
        };

        let mut prompt = digest_auth::parse(&challenge)
            .context("failed to parse digest auth challenge")?;
        let mut uri_value = uri.path().to_string();
        if let Some(query) = uri.query() {
            uri_value.push('?');
            uri_value.push_str(query);
        }
        let context = AuthContext::new_with_method(
            &creds.username,
            &creds.password,
            uri_value,
            Option::<&[u8]>::None,
            method,
        );
        let header_value = prompt
            .respond(&context)
            .context("failed to compute digest authorization")?
            .to_string();

        build(Some(&header_value))
            .send()
            .await
            .context("HTTP request with digest auth failed")
    }
}

impl Default for HttpBackend {
    fn default() -> Self {
        Self::new().expect("failed to create default HttpBackend")
    }
}

#[async_trait]
impl ByteSourceBackend for HttpBackend {
    async fn probe(&self, uri: &Url, ctx: &ProbeContext) -> Result<FileProbe> {
        debug!(uri = %uri, "probing HTTP resource");

        let headers = Self::build_headers(ctx);
        let creds = ctx.auth.as_ref().cloned().or_else(|| {
            self.netrc_credentials_for(uri).map(|(username, password)| Credentials {
                username,
                password,
            })
        });

        let resp = self
            .send_with_optional_digest(
                uri,
                creds.as_ref(),
                DigestHttpMethod::HEAD,
                |digest_header| {
                    let mut request = self.client.head(uri.as_str()).timeout(ctx.timeout);
                    request = request.headers(headers.clone());
                    if let Some(header) = digest_header {
                        request = request.header(reqwest::header::AUTHORIZATION, header);
                    } else if let Some(ref creds) = creds {
                        request = request.basic_auth(&creds.username, Some(&creds.password));
                    }
                    request
                },
            )
            .await
            .context("HTTP HEAD request failed")?;

        if resp.status() == StatusCode::NOT_MODIFIED {
            return Ok(FileProbe {
                size: None,
                supports_range: false,
                etag: None,
                last_modified: None,
                content_type: None,
                suggested_filename: None,
                not_modified: true,
            });
        }

        let resp = resp
            .error_for_status()
            .context("HTTP HEAD returned error status")?;

        let headers = resp.headers();

        let size = headers
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());

        let supports_range = headers
            .get("accept-ranges")
            .map(|v| v.to_str().unwrap_or("").contains("bytes"))
            .unwrap_or(false);

        let etag = headers
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let last_modified = headers
            .get(LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let content_type = headers
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let suggested_filename = headers
            .get("content-disposition")
            .and_then(|v| v.to_str().ok())
            .and_then(crate::content_disposition::parse_content_disposition);

        Ok(FileProbe {
            size,
            supports_range,
            etag,
            last_modified,
            content_type,
            suggested_filename,
            not_modified: false,
        })
    }

    async fn open_from(&self, uri: &Url, offset: u64, ctx: &OpenContext) -> Result<ByteStream> {
        debug!(uri = %uri, offset, "opening HTTP stream");

        let creds = ctx.auth.as_ref().cloned().or_else(|| {
            self.netrc_credentials_for(uri).map(|(username, password)| Credentials {
                username,
                password,
            })
        });

        let resp = self
            .send_with_optional_digest(
                uri,
                creds.as_ref(),
                DigestHttpMethod::GET,
                |digest_header| {
                    let mut request = self.client.get(uri.as_str()).timeout(ctx.timeout);

                    for (name, value) in &ctx.headers {
                        request = request.header(name, value);
                    }

                    if offset > 0 {
                        request = request.header(RANGE, format!("bytes={offset}-"));
                        if let Some(ref etag) = ctx.etag {
                            request = request.header("If-Range", etag.as_str());
                        }
                    }

                    if let Some(header) = digest_header {
                        request = request.header(reqwest::header::AUTHORIZATION, header);
                    } else if let Some(ref creds) = creds {
                        request = request.basic_auth(&creds.username, Some(&creds.password));
                    }

                    request
                },
            )
            .await
            .context("HTTP GET request failed")?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Err(anyhow::anyhow!("http status 404 not found"));
        }
        let resp = resp
            .error_for_status()
            .context("HTTP GET returned error status")?;

        // Detect resource change: if we requested a range but got 200 OK
        // instead of 206 Partial Content, the resource has changed and
        // our partial data is stale.
        let status_code = resp.status().as_u16();
        if Self::is_resource_changed(status_code, offset > 0) {
            return Err(anyhow::anyhow!(
                "resource changed on server (got {} instead of 206): partial data is stale, \
                 download must restart from the beginning",
                status_code
            ));
        }

        // Convert reqwest's bytes stream into AsyncRead via StreamReader.
        let byte_stream = resp
            .bytes_stream()
            .map_err(std::io::Error::other);

        let reader = StreamReader::new(byte_stream);
        Ok(Box::pin(reader))
    }

    fn name(&self) -> &'static str {
        "http"
    }
}

impl Drop for HttpBackend {
    fn drop(&mut self) {
        let Some(ref path) = self.save_cookie_file else {
            return;
        };
        let Some(ref store) = self.cookie_store else {
            return;
        };
        if let Err(error) = crate::cookies::save_cookie_store(path, store) {
            tracing::warn!(error = %error, path = %path.display(), "failed to save cookies");
        }
    }
}

impl HttpBackend {
    /// Check whether a range request was silently replaced with a full response.
    ///
    /// Returns `true` when we sent a `Range` header but the server responded
    /// with `200 OK` instead of `206 Partial Content`, indicating the
    /// resource has changed (e.g., ETag mismatch) and our partial data is stale.
    pub fn is_resource_changed(status_code: u16, was_range_request: bool) -> bool {
        was_range_request && status_code == 200
    }
}

fn load_netrc_auth(path: Option<&std::path::Path>) -> Result<Option<NetrcAuthMap>> {
    let parsed = match path {
        Some(path) => Netrc::from_file(path)
            .with_context(|| format!("failed to load netrc file: {}", path.display()))?,
        None => return Ok(None),
    };

    let entries = parsed
        .hosts
        .into_iter()
        .map(|(host, auth)| (host, (auth.login, auth.password)))
        .collect::<HashMap<_, _>>();

    Ok(Some(Arc::new(entries)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_backend_creates_successfully() {
        let backend = HttpBackend::new().unwrap();
        assert_eq!(backend.name(), "http");
    }

    #[test]
    fn build_headers_converts_pairs() {
        let ctx = ProbeContext {
            headers: vec![
                ("Accept".into(), "application/octet-stream".into()),
                ("X-Custom".into(), "value".into()),
            ],
            ..Default::default()
        };
        let headers = HttpBackend::build_headers(&ctx);
        assert_eq!(headers.len(), 2);
        assert_eq!(
            headers.get("accept").unwrap().to_str().unwrap(),
            "application/octet-stream"
        );
    }

    #[test]
    fn backend_with_default_config() {
        let config = HttpBackendConfig {
            check_certificate: true,
            ..Default::default()
        };
        let backend = HttpBackend::with_config(&config).unwrap();
        assert_eq!(backend.name(), "http");
    }

    #[test]
    fn backend_with_proxy_config() {
        // Proxy URLs are validated during construction.
        let config = HttpBackendConfig {
            all_proxy: Some("http://proxy.example.com:8080".into()),
            check_certificate: true,
            ..Default::default()
        };
        let backend = HttpBackend::with_config(&config).unwrap();
        assert_eq!(backend.name(), "http");
    }

    #[test]
    fn backend_with_invalid_proxy_errors() {
        let config = HttpBackendConfig {
            all_proxy: Some("not a valid url".into()),
            ..Default::default()
        };
        assert!(HttpBackend::with_config(&config).is_err());
    }

    #[test]
    fn backend_with_disabled_cert_check() {
        let config = HttpBackendConfig {
            check_certificate: false,
            ..Default::default()
        };
        // Should construct without error — dangerous but valid.
        let backend = HttpBackend::with_config(&config).unwrap();
        assert_eq!(backend.name(), "http");
    }

    #[test]
    fn backend_with_custom_user_agent() {
        let config = HttpBackendConfig {
            user_agent: Some("Custom/1.0".into()),
            check_certificate: true,
            ..Default::default()
        };
        let backend = HttpBackend::with_config(&config).unwrap();
        assert_eq!(backend.name(), "http");
    }

    #[test]
    fn backend_with_no_proxy_list() {
        let config = HttpBackendConfig {
            all_proxy: Some("http://proxy.example.com:8080".into()),
            no_proxy: Some("localhost,127.0.0.1,*.internal.corp".into()),
            check_certificate: true,
            ..Default::default()
        };
        let backend = HttpBackend::with_config(&config).unwrap();
        assert_eq!(backend.name(), "http");
    }

    #[test]
    fn backend_with_http_and_https_proxy() {
        let config = HttpBackendConfig {
            http_proxy: Some("http://http-proxy:3128".into()),
            https_proxy: Some("http://https-proxy:3129".into()),
            check_certificate: true,
            ..Default::default()
        };
        let backend = HttpBackend::with_config(&config).unwrap();
        assert_eq!(backend.name(), "http");
    }

    #[test]
    fn backend_config_default() {
        let config = HttpBackendConfig::default();
        assert!(config.all_proxy.is_none());
        assert!(config.http_proxy.is_none());
        assert!(config.https_proxy.is_none());
        assert!(config.no_proxy.is_none());
        assert!(!config.check_certificate); // Default is false (struct default)
        assert!(config.ca_certificate.is_none());
        assert!(config.user_agent.is_none());
    }
}
