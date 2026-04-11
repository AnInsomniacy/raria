// raria-core: Input file parser (aria2: --input-file / -i).
//
// Parses a text file containing one URI per line. Supports:
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
    /// Optional expected checksum in `algo=hex` format.
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
    /// One or more URIs for this entry.
    pub uris: Vec<String>,
    /// Per-entry option overrides.
    pub options: InputFileOptions,
}

/// Structured entry parsed from an aria2-style input file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputFileEntry {
    /// One or more source URIs for this entry.
    pub uris: Vec<String>,
    /// Raw per-entry options in file order.
    pub options: Vec<(String, String)>,
}

fn read_input_file(path: &Path) -> anyhow::Result<String> {
    std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read input file '{}': {e}", path.display()))
}

/// Parse the content of an input file into structured entries.
///
/// Each non-indented URI line starts a new entry. Indented `key=value` lines are
/// attached to the most recent entry as raw options. Blank lines, comments, and
/// malformed option lines are ignored.
pub fn parse_input_file_entries(content: &str) -> Vec<InputFileEntry> {
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
                entry
                    .options
                    .push((key.trim().to_string(), value.trim().to_string()));
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
            options: Vec::new(),
        });
    }

    if let Some(entry) = current {
        entries.push(entry);
    }

    entries
}

/// Parse the content of an input file and return a list of URI strings.
///
/// Each entry may contain tab-separated URIs for multi-source download.
/// Per-URI option lines (prefixed with whitespace) are currently skipped.
/// Comment lines (starting with #) and blank lines are ignored.
pub fn parse_input_file(content: &str) -> Vec<String> {
    parse_input_file_entries(content)
        .into_iter()
        .map(|entry| entry.uris.join("\t"))
        .collect()
}

/// Parse the content of an input file into richer entry records.
///
/// This preserves tab-separated multi-source lines as a single entry with
/// multiple `uris`, and captures supported per-entry options from indented
/// `key=value` lines.
pub fn parse_input_file_entries(content: &str) -> anyhow::Result<Vec<InputFileEntry>> {
    let mut entries: Vec<InputFileEntry> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if line.starts_with(' ') || line.starts_with('\t') {
            let entry = entries.last_mut().ok_or_else(|| {
                anyhow::anyhow!("input-file option line appeared before any URI entry")
            })?;
            let (raw_key, raw_value) = trimmed.split_once('=').ok_or_else(|| {
                anyhow::anyhow!("invalid input-file option '{trimmed}': expected key=value")
            })?;
            let key = raw_key.trim();
            let value = raw_value.trim();
            anyhow::ensure!(
                !key.is_empty(),
                "invalid input-file option '{trimmed}': empty key"
            );

            match key {
                "dir" => entry.options.dir = Some(PathBuf::from(value)),
                "out" => entry.options.out = Some(value.to_string()),
                "checksum" => entry.options.checksum = Some(value.to_string()),
                "header" => entry.options.headers.push(value.to_string()),
                "http-user" => entry.options.http_user = Some(value.to_string()),
                "http-passwd" => entry.options.http_passwd = Some(value.to_string()),
                _ => {
                    entry
                        .options
                        .extra
                        .insert(key.to_string(), value.to_string());
                }
            }

            continue;
        }

        let uris = trimmed
            .split('\t')
            .map(str::trim)
            .filter(|uri| !uri.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        anyhow::ensure!(
            !uris.is_empty(),
            "input-file entry must contain at least one URI"
        );
        entries.push(InputFileEntry {
            uris,
            options: InputFileOptions::default(),
        });
    }

    Ok(entries)
}

/// Load URIs from an input file on disk.
///
/// Returns an error if the file cannot be read.
pub fn load_input_file(path: &Path) -> anyhow::Result<Vec<String>> {
    let content = read_input_file(path)?;
    Ok(parse_input_file(&content))
}

/// Load richer URI entries from an input file on disk.
pub fn load_input_file_entries(path: &Path) -> anyhow::Result<Vec<InputFileEntry>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read input file '{}': {e}", path.display()))?;
    parse_input_file_entries(&content)
}
