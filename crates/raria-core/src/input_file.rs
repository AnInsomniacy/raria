// raria-core: Input file parser (aria2: --input-file / -i).
//
// Parses a text file containing one logical URI entry per block. Supports:
// - Comments (lines starting with #)
// - Blank lines (ignored)
// - Per-URI options (lines starting with whitespace, after a URI line)
// - Tab-separated multi-source URIs on a single line
//
// Format example:
//   https://mirror1.com/file.zip\thttps://mirror2.com/file.zip
//     dir=/tmp/downloads
//     out=custom_name.zip
//   https://example.com/other.zip

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Parsed per-entry options from an aria2-style input file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputFileOptions {
    /// Optional output directory override.
    pub dir: Option<PathBuf>,
    /// Optional output filename override.
    pub out: Option<String>,
    /// Optional expected checksum in `algo=hex` form.
    pub checksum: Option<String>,
    /// Additional request headers in `Name: Value` form.
    pub headers: Vec<String>,
    /// Optional HTTP basic-auth username.
    pub http_user: Option<String>,
    /// Optional HTTP basic-auth password.
    pub http_passwd: Option<String>,
    /// Supported-but-not-yet-modeled options captured verbatim.
    pub extra: BTreeMap<String, String>,
}

/// One URI entry from an aria2-style input file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputFileEntry {
    /// One or more source URIs for this entry.
    pub uris: Vec<String>,
    /// Parsed per-entry option overrides.
    pub options: InputFileOptions,
}

fn read_input_file(path: &Path) -> anyhow::Result<String> {
    std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read input file '{}': {e}", path.display()))
}

fn apply_option(options: &mut InputFileOptions, key: &str, value: &str) {
    let key = key.trim();
    let value = value.trim();

    match key {
        "dir" => {
            if !value.is_empty() {
                options.dir = Some(PathBuf::from(value));
            }
        }
        "out" => {
            if !value.is_empty() {
                options.out = Some(value.to_string());
            }
        }
        "checksum" => {
            if !value.is_empty() {
                options.checksum = Some(value.to_string());
            }
        }
        "header" => {
            if !value.is_empty() {
                options.headers.push(value.to_string());
            }
        }
        "http-user" => {
            if !value.is_empty() {
                options.http_user = Some(value.to_string());
            }
        }
        "http-passwd" => {
            if !value.is_empty() {
                options.http_passwd = Some(value.to_string());
            }
        }
        _ => {
            if !value.is_empty() {
                options.extra.insert(key.to_string(), value.to_string());
            }
        }
    }
}

/// Parse the content of an input file into structured entries.
pub fn parse_input_file_entries(content: &str) -> anyhow::Result<Vec<InputFileEntry>> {
    let mut entries = Vec::new();
    let mut current: Option<InputFileEntry> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if line.starts_with(' ') || line.starts_with('\t') {
            let Some(entry) = current.as_mut() else {
                continue;
            };

            if let Some((key, value)) = trimmed.split_once('=') {
                apply_option(&mut entry.options, key, value);
            }
            continue;
        }

        if let Some(entry) = current.take() {
            entries.push(entry);
        }

        let uris = trimmed
            .split('\t')
            .map(str::trim)
            .filter(|uri| !uri.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        if uris.is_empty() {
            continue;
        }

        current = Some(InputFileEntry {
            uris,
            options: InputFileOptions::default(),
        });
    }

    if let Some(entry) = current {
        entries.push(entry);
    }

    Ok(entries)
}

/// Parse the content of an input file and return a flat list of URI lines.
///
/// This legacy API intentionally preserves the old behavior of returning one
/// string per logical entry, ignoring per-entry options.
pub fn parse_input_file(content: &str) -> Vec<String> {
    parse_input_file_entries(content)
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.uris.join("\t"))
        .collect()
}

/// Load URIs from an input file on disk.
pub fn load_input_file(path: &Path) -> anyhow::Result<Vec<String>> {
    let content = read_input_file(path)?;
    Ok(parse_input_file(&content))
}

/// Load richer URI entries from an input file on disk.
pub fn load_input_file_entries(path: &Path) -> anyhow::Result<Vec<InputFileEntry>> {
    let content = read_input_file(path)?;
    parse_input_file_entries(&content)
}
