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

use std::path::Path;

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

/// Load URIs from an input file on disk.
///
/// Returns an error if the file cannot be read.
pub fn load_input_file(path: &Path) -> anyhow::Result<Vec<String>> {
    let content = read_input_file(path)?;
    Ok(parse_input_file(&content))
}

/// Load structured input-file entries from disk.
pub fn load_input_file_entries(path: &Path) -> anyhow::Result<Vec<InputFileEntry>> {
    let content = read_input_file(path)?;
    Ok(parse_input_file_entries(&content))
}
