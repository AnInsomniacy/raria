// Tests for --input-file support.
//
// Verifies that raria can read URIs from a file and create
// download jobs for each one (aria2: --input-file / -i).

#[cfg(test)]
mod tests {
    use raria_core::config::GlobalConfig;
    use raria_core::engine::Engine;
    use std::sync::Arc;

    /// Parse an input file and return URIs.
    fn parse_input_file(content: &str) -> Vec<String> {
        raria_core::input_file::parse_input_file(content)
    }

    #[test]
    fn parses_simple_uri_list() {
        let content = "https://example.com/file1.zip\nhttps://example.com/file2.zip\n";
        let uris = parse_input_file(content);
        assert_eq!(uris.len(), 2);
        assert_eq!(uris[0], "https://example.com/file1.zip");
        assert_eq!(uris[1], "https://example.com/file2.zip");
    }

    #[test]
    fn ignores_blank_lines_and_comments() {
        let content = "\
# This is a comment
https://example.com/file1.zip

# Another comment

https://example.com/file2.zip
";
        let uris = parse_input_file(content);
        assert_eq!(uris.len(), 2);
    }

    #[test]
    fn trims_trailing_whitespace() {
        // Only trailing whitespace should be trimmed; leading whitespace
        // marks option lines in aria2 format.
        let content = "https://example.com/file.zip   \n";
        let uris = parse_input_file(content);
        assert_eq!(uris.len(), 1);
        assert_eq!(uris[0], "https://example.com/file.zip");
    }

    #[test]
    fn handles_empty_file() {
        let uris = parse_input_file("");
        assert!(uris.is_empty());
    }

    #[test]
    fn handles_per_uri_options() {
        // aria2 format: URI lines followed by option lines (prefixed with space)
        let content = "\
https://example.com/file1.zip
  dir=/tmp/downloads
  out=custom_name.zip
https://example.com/file2.zip
";
        let uris = parse_input_file(content);
        // For now, we extract URIs only (options are future work).
        assert_eq!(uris.len(), 2);
    }

    #[test]
    fn supports_multiple_uris_per_line_tab_separated() {
        // aria2 supports multiple URIs per line (tab-separated) for multi-source download.
        let content = "https://mirror1.com/f.zip\thttps://mirror2.com/f.zip\n";
        let uris = parse_input_file(content);
        // Tab-separated URIs should be treated as one entry (multi-source).
        assert_eq!(uris.len(), 1);
        assert!(uris[0].contains("mirror1.com"));
    }

    #[test]
    fn engine_adds_jobs_from_input_file() {
        let config = GlobalConfig::default();
        let engine = Engine::new(config);

        let content = "\
https://example.com/file1.zip
https://example.com/file2.zip
https://example.com/file3.zip
";
        let uris = parse_input_file(content);
        assert_eq!(uris.len(), 3);

        for uri in &uris {
            let spec = raria_core::engine::AddUriSpec {
                uris: vec![uri.clone()],
                filename: None,
                dir: std::path::PathBuf::from("/tmp"),
                connections: 1,
            };
            engine.add_uri(&spec).unwrap();
        }

        let jobs = engine.registry.snapshot();
        assert_eq!(jobs.len(), 3);
    }
}
