use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use raria_core::config::GlobalConfig;
use std::path::Path;

pub(crate) fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.2} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

pub(crate) fn parse_header_args(values: &[String]) -> Result<Vec<(String, String)>> {
    values
        .iter()
        .map(|header| {
            let (name, value) = header.split_once(':').ok_or_else(|| {
                anyhow::anyhow!("invalid header '{header}': expected Name: Value")
            })?;
            let name = name.trim();
            let value = value.trim();
            anyhow::ensure!(!name.is_empty(), "invalid header '{header}': empty name");
            Ok((name.to_string(), value.to_string()))
        })
        .collect()
}

pub(crate) fn build_conditional_get_probe_headers(
    config: &GlobalConfig,
    uri: &url::Url,
    candidate_path: &Path,
    control_file_path: &Path,
    base_headers: &[(String, String)],
) -> Result<Vec<(String, String)>> {
    let mut probe_headers = base_headers.to_vec();

    if config.conditional_get
        && config.allow_overwrite
        && matches!(uri.scheme(), "http" | "https")
        && candidate_path.is_file()
        && !control_file_path.exists()
    {
        let modified = std::fs::metadata(candidate_path)
            .and_then(|meta| meta.modified())
            .with_context(|| {
                format!(
                    "failed to read local file mtime for conditional-get: {}",
                    candidate_path.display()
                )
            })?;
        let modified: DateTime<Utc> = modified.into();
        probe_headers.push((
            "If-Modified-Since".into(),
            modified.format("%a, %d %b %Y %H:%M:%S GMT").to_string(),
        ));
    }

    Ok(probe_headers)
}

#[cfg(test)]
mod tests {
    use super::{build_conditional_get_probe_headers, format_bytes, parse_header_args};
    use raria_core::config::GlobalConfig;
    use tempfile::tempdir;

    #[test]
    fn format_bytes_small() {
        assert_eq!(format_bytes(42), "42 B");
    }

    #[test]
    fn format_bytes_kib() {
        assert_eq!(format_bytes(2048), "2.00 KiB");
    }

    #[test]
    fn format_bytes_mib() {
        assert_eq!(format_bytes(1024 * 1024 * 5), "5.00 MiB");
    }

    #[test]
    fn format_bytes_gib() {
        assert_eq!(format_bytes(1024 * 1024 * 1024 * 2), "2.00 GiB");
    }

    #[test]
    fn parse_header_args_parses_pairs() {
        let headers = parse_header_args(&["X-Test: value".into()]).unwrap();
        assert_eq!(headers, vec![("X-Test".into(), "value".into())]);
    }

    #[test]
    fn parse_header_args_rejects_invalid_shape() {
        assert!(parse_header_args(&["broken".into()]).is_err());
    }

    #[test]
    fn conditional_get_adds_if_modified_since_for_existing_http_target() {
        let temp = tempdir().unwrap();
        let file = temp.path().join("cached.bin");
        std::fs::write(&file, b"cached").unwrap();

        let mut config = GlobalConfig::default();
        config.conditional_get = true;
        config.allow_overwrite = true;

        let headers = build_conditional_get_probe_headers(
            &config,
            &"http://example.com/cached.bin".parse().unwrap(),
            &file,
            &file.with_extension("bin.aria2"),
            &[],
        )
        .unwrap();

        assert!(
            headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("if-modified-since"))
        );
    }
}
