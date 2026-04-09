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

/// Parse the content of an input file and return a list of URI strings.
///
/// Each entry may contain tab-separated URIs for multi-source download.
/// Per-URI option lines (prefixed with whitespace) are currently skipped.
/// Comment lines (starting with #) and blank lines are ignored.
pub fn parse_input_file(content: &str) -> Vec<String> {
    let mut uris = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip blank lines and comments.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Lines starting with whitespace are per-URI options (for the
        // preceding URI). We skip them for now.
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }

        // The line is a URI (possibly tab-separated for multi-source).
        // We keep the full line as-is; the caller can split on tabs.
        uris.push(trimmed.to_string());
    }

    uris
}

/// Load URIs from an input file on disk.
///
/// Returns an error if the file cannot be read.
pub fn load_input_file(path: &Path) -> anyhow::Result<Vec<String>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read input file '{}': {e}", path.display()))?;
    Ok(parse_input_file(&content))
}
