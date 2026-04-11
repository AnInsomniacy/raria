// Tests for --input-file support.
//
// Verifies that raria can read URIs from a file and create
// download jobs for each one (aria2: --input-file / -i).

#[cfg(test)]
mod tests {
    use raria_core::config::GlobalConfig;
    use raria_core::engine::Engine;
    use raria_core::input_file::InputFileEntry;

    /// Parse an input file and return URIs.
    fn parse_input_file(content: &str) -> Vec<String> {
        raria_core::input_file::parse_input_file(content)
    }

    fn parse_input_file_entries(content: &str) -> Vec<InputFileEntry> {
        raria_core::input_file::parse_input_file_entries(content)
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
    fn richer_entries_capture_supported_per_uri_options_while_legacy_api_stays_flat() {
        let content = "\
https://mirror1.com/f.zip\thttps://mirror2.com/f.zip
  dir=/tmp/downloads
  out=custom.bin
  checksum=sha-256=abc123
  header=X-Test: from-input
  http-user=alice
  http-passwd=secret
  max-download-limit=1024
";

        let legacy = parse_input_file(content);
        assert_eq!(legacy, vec!["https://mirror1.com/f.zip\thttps://mirror2.com/f.zip"]);

        let entries = raria_core::input_file::parse_input_file_entries(content)
            .expect("parse richer input-file entries");
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(
            entry.uris,
            vec![
                "https://mirror1.com/f.zip".to_string(),
                "https://mirror2.com/f.zip".to_string()
            ]
        );
        assert_eq!(
            entry.options.dir.as_deref(),
            Some(std::path::Path::new("/tmp/downloads"))
        );
        assert_eq!(entry.options.out.as_deref(), Some("custom.bin"));
        assert_eq!(entry.options.checksum.as_deref(), Some("sha-256=abc123"));
        assert_eq!(entry.options.headers, vec!["X-Test: from-input"]);
        assert_eq!(entry.options.http_user.as_deref(), Some("alice"));
        assert_eq!(entry.options.http_passwd.as_deref(), Some("secret"));
        assert_eq!(
            entry.options.extra.get("max-download-limit").map(String::as_str),
            Some("1024")
        );
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
