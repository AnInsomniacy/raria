// raria-http: HTTP/HTTPS backend implementing ByteSourceBackend.
//
// Uses reqwest for all HTTP operations. Supports:
// - HEAD probing for size/range/etag detection
// - Range-based offset downloads
// - Cookie persistence via reqwest_cookie_store

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::TryStreamExt;
use raria_range::backend::{
    ByteSourceBackend, ByteStream, FileProbe, OpenContext, ProbeContext,
};
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, CONTENT_LENGTH, CONTENT_TYPE, ETAG, LAST_MODIFIED, RANGE,
};
use reqwest::Client;
use tokio_util::io::StreamReader;
use tracing::debug;
use url::Url;

/// HTTP/HTTPS download backend.
#[derive(Debug, Clone)]
pub struct HttpBackend {
    client: Client,
}

impl HttpBackend {
    /// Create a new HTTP backend with default settings.
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent(concat!("raria/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("failed to build reqwest client")?;

        Ok(Self { client })
    }

    /// Create a new HTTP backend with a custom client.
    pub fn with_client(client: Client) -> Self {
        Self { client }
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

        let mut request = self.client.head(uri.as_str()).timeout(ctx.timeout);

        if let Some(ref creds) = ctx.auth {
            request = request.basic_auth(&creds.username, Some(&creds.password));
        }

        request = request.headers(Self::build_headers(ctx));

        let resp = request
            .send()
            .await
            .context("HTTP HEAD request failed")?
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
        })
    }

    async fn open_from(&self, uri: &Url, offset: u64, ctx: &OpenContext) -> Result<ByteStream> {
        debug!(uri = %uri, offset, "opening HTTP stream");

        let mut request = self.client.get(uri.as_str()).timeout(ctx.timeout);

        if let Some(ref creds) = ctx.auth {
            request = request.basic_auth(&creds.username, Some(&creds.password));
        }

        if offset > 0 {
            request = request.header(RANGE, format!("bytes={offset}-"));
        }

        let resp = request
            .send()
            .await
            .context("HTTP GET request failed")?
            .error_for_status()
            .context("HTTP GET returned error status")?;

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
}
