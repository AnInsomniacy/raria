// raria-http: Content-Disposition header parsing.
//
// Extracts suggested filenames from HTTP Content-Disposition headers.
// Supports the most common formats:
// - Content-Disposition: attachment; filename="file.zip"
// - Content-Disposition: attachment; filename=file.zip
// - Content-Disposition: attachment; filename*=UTF-8''encoded%20name.zip

/// Extract the filename from a Content-Disposition header value.
///
/// Returns `None` if the header is absent or doesn't contain a filename.
pub fn parse_content_disposition(value: &str) -> Option<String> {
    // Try filename*= first (RFC 5987 extended parameter), then filename=
    if let Some(filename) = extract_filename_star(value) {
        return Some(filename);
    }
    extract_filename(value)
}

/// Extract filename from the standard `filename=` parameter.
fn extract_filename(value: &str) -> Option<String> {
    // Look for filename="..." or filename=...
    let lower = value.to_lowercase();
    let idx = lower.find("filename=")?;
    let after = &value[idx + "filename=".len()..];

    if let Some(stripped) = after.strip_prefix('"') {
        // Quoted string: filename="some file.zip"
        let end = stripped.find('"')?;
        Some(stripped[..end].to_string())
    } else {
        // Unquoted token: filename=file.zip
        let end = after.find([';', ' ', '\t']).unwrap_or(after.len());
        let name = after[..end].trim();
        if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        }
    }
}

/// Extract filename from the RFC 5987 `filename*=` parameter.
fn extract_filename_star(value: &str) -> Option<String> {
    let lower = value.to_lowercase();
    let idx = lower.find("filename*=")?;
    let after = &value[idx + "filename*=".len()..];

    // Format: charset'language'encoded-value
    // Usually: UTF-8''percent-encoded-name
    let parts: Vec<&str> = after.splitn(3, '\'').collect();
    if parts.len() < 3 {
        return None;
    }

    let encoded = parts[2].split([';', ' ', '\t']).next().unwrap_or("");

    // Percent-decode the filename.
    let decoded = percent_decode(encoded);
    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

/// Simple percent-decoding for filenames.
fn percent_decode(input: &str) -> String {
    let mut result = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                result.push(byte);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quoted_filename() {
        let val = r#"attachment; filename="report.pdf""#;
        assert_eq!(parse_content_disposition(val), Some("report.pdf".into()));
    }

    #[test]
    fn parse_unquoted_filename() {
        let val = "attachment; filename=file.zip";
        assert_eq!(parse_content_disposition(val), Some("file.zip".into()));
    }

    #[test]
    fn parse_filename_with_spaces_in_quotes() {
        let val = r#"attachment; filename="my file (1).tar.gz""#;
        assert_eq!(
            parse_content_disposition(val),
            Some("my file (1).tar.gz".into())
        );
    }

    #[test]
    fn parse_filename_star_utf8() {
        let val = "attachment; filename*=UTF-8''my%20file%20%282%29.zip";
        assert_eq!(
            parse_content_disposition(val),
            Some("my file (2).zip".into())
        );
    }

    #[test]
    fn filename_star_takes_precedence() {
        let val = r#"attachment; filename="fallback.zip"; filename*=UTF-8''preferred.zip"#;
        assert_eq!(parse_content_disposition(val), Some("preferred.zip".into()));
    }

    #[test]
    fn parse_inline_with_filename() {
        let val = r#"inline; filename="document.pdf""#;
        assert_eq!(parse_content_disposition(val), Some("document.pdf".into()));
    }

    #[test]
    fn parse_no_filename() {
        let val = "attachment";
        assert_eq!(parse_content_disposition(val), None);
    }

    #[test]
    fn parse_empty_string() {
        assert_eq!(parse_content_disposition(""), None);
    }

    #[test]
    fn parse_case_insensitive_filename() {
        let val = r#"attachment; Filename="CAPS.zip""#;
        assert_eq!(parse_content_disposition(val), Some("CAPS.zip".into()));
    }

    #[test]
    fn parse_filename_with_semicolon_in_quotes() {
        // This is an edge case — the filename contains a semicolon inside quotes
        let val = r#"attachment; filename="file;name.zip""#;
        assert_eq!(parse_content_disposition(val), Some("file;name.zip".into()));
    }

    #[test]
    fn percent_decode_basic() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("file%28test%29.zip"), "file(test).zip");
    }

    #[test]
    fn percent_decode_no_encoding() {
        assert_eq!(percent_decode("normal.txt"), "normal.txt");
    }
}
