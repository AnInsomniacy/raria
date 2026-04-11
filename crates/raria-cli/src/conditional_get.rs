use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use raria_core::config::GlobalConfig;
use std::path::Path;
use url::Url;

pub(crate) fn build_conditional_get_probe_headers(
    base_headers: &[(String, String)],
    config: &GlobalConfig,
    parsed_url: &Url,
    target_path: &Path,
    control_file_path: &Path,
) -> Result<Vec<(String, String)>> {
    let mut headers = base_headers.to_vec();

    if config.conditional_get
        && config.allow_overwrite
        && matches!(parsed_url.scheme(), "http" | "https")
        && target_path.is_file()
        && !control_file_path.exists()
    {
        let modified = std::fs::metadata(target_path)
            .and_then(|meta| meta.modified())
            .with_context(|| {
                format!(
                    "failed to read local file mtime for conditional-get: {}",
                    target_path.display()
                )
            })?;
        let modified: DateTime<Utc> = modified.into();
        headers.push((
            "If-Modified-Since".into(),
            modified.format("%a, %d %b %Y %H:%M:%S GMT").to_string(),
        ));
    }

    Ok(headers)
}
