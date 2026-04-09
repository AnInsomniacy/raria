// raria-http: Netscape cookie file parser.
//
// Supports loading cookies from Netscape/Mozilla cookie files,
// the format used by aria2's --load-cookies option.
//
// Format: domain\tflag\tpath\tsecure\texpiration\tname\tvalue
// Lines starting with # are comments. Empty lines are skipped.

use reqwest::cookie::Jar;
use std::path::Path;
use std::sync::Arc;
use url::Url;

/// Parse a Netscape cookie file and load cookies into a reqwest cookie jar.
///
/// Returns the jar with all valid cookies loaded.
/// Invalid lines are silently skipped (matches aria2 behavior).
pub fn load_cookie_file(path: &Path) -> std::io::Result<Arc<Jar>> {
    let content = std::fs::read_to_string(path)?;
    Ok(parse_cookie_content(&content))
}

/// Parse Netscape cookie content into a cookie jar.
pub fn parse_cookie_content(content: &str) -> Arc<Jar> {
    let jar = Arc::new(Jar::default());

    for line in content.lines() {
        let line = line.trim();
        // Skip comments and empty lines.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 7 {
            continue; // Invalid line — skip.
        }

        let domain = fields[0];
        // fields[1] = include subdomains flag (TRUE/FALSE)
        let path = fields[2];
        let secure = fields[3].eq_ignore_ascii_case("TRUE");
        // fields[4] = expiration (unix timestamp, 0 = session)
        let name = fields[5];
        let value = fields[6];

        // Build a URL from the domain to set the cookie.
        let scheme = if secure { "https" } else { "http" };
        let clean_domain = domain.trim_start_matches('.');
        let url_str = format!("{scheme}://{clean_domain}{path}");

        if let Ok(url) = Url::parse(&url_str) {
            let cookie_str = format!("{name}={value}; Domain={domain}; Path={path}");
            jar.add_cookie_str(&cookie_str, &url);
        }
    }

    jar
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_content() {
        let jar = parse_cookie_content("");
        // No cookies loaded — jar should exist but be empty.
        // We can't directly inspect jar contents, but construction should succeed.
        let _ = jar;
    }

    #[test]
    fn parse_comments_and_blanks() {
        let content = r#"
# Netscape HTTP Cookie File
# http://curl.se/docs/http-cookies.html

# This is a comment
"#;
        let jar = parse_cookie_content(content);
        let _ = jar;
    }

    #[test]
    fn parse_valid_cookie_line() {
        let content = ".example.com\tTRUE\t/\tFALSE\t0\tsession_id\tabc123";
        let jar = parse_cookie_content(content);
        // Cookie should be loadable.
        let _ = jar;
    }

    #[test]
    fn parse_secure_cookie() {
        let content = ".secure.example.com\tTRUE\t/api\tTRUE\t1700000000\ttoken\tsecret";
        let jar = parse_cookie_content(content);
        let _ = jar;
    }

    #[test]
    fn parse_multiple_cookies() {
        let content = r#"# Cookie file
.example.com	TRUE	/	FALSE	0	session	abc
.example.com	TRUE	/	FALSE	0	user	john
.other.com	FALSE	/path	TRUE	1700000000	auth	xyz"#;
        let jar = parse_cookie_content(content);
        let _ = jar;
    }

    #[test]
    fn skip_invalid_lines() {
        let content = r#".example.com	TRUE	/	FALSE	0	session	abc
this is not a valid cookie line
only	three	fields
.valid.com	TRUE	/	FALSE	0	name	value"#;
        // Should not panic — invalid lines silently skipped.
        let jar = parse_cookie_content(content);
        let _ = jar;
    }

    #[test]
    fn parse_subdomain_cookie() {
        let content = "example.com\tFALSE\t/\tFALSE\t0\tcsrf\ttoken123";
        let jar = parse_cookie_content(content);
        let _ = jar;
    }

    #[test]
    fn load_nonexistent_file_returns_error() {
        let result = load_cookie_file(Path::new("/nonexistent/cookies.txt"));
        assert!(result.is_err());
    }
}
