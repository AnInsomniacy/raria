// raria-http: Netscape cookie file parser + serializer.
//
// Supports loading and saving cookies in Netscape/Mozilla cookie file format,
// used by aria2's --load-cookies / --save-cookies options.
//
// Format: domain\tflag\tpath\tsecure\texpiration\tname\tvalue
// Lines starting with # are comments. Empty lines are skipped.

use chrono::{DateTime, Utc};
use reqwest_cookie_store::{CookieStore, CookieStoreMutex, RawCookie};
use std::io::{BufWriter, Write};
use std::path::Path;
use url::Url;

use cookie_store::{CookieDomain, CookieExpiration};

/// Parse a Netscape cookie file and load cookies into a cookie store.
///
/// Returns the store with all valid cookies loaded.
/// Invalid lines are silently skipped (matches aria2 behavior).
pub fn load_cookie_store(path: &Path) -> std::io::Result<CookieStore> {
    let content = std::fs::read_to_string(path)?;
    Ok(parse_cookie_content(&content))
}

/// Parse Netscape cookie content into a cookie store.
pub fn parse_cookie_content(content: &str) -> CookieStore {
    let mut store = CookieStore::default();

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
        let expires = fields[4].parse::<i64>().unwrap_or(0);
        let name = fields[5];
        let value = fields[6];

        let scheme = if secure { "https" } else { "http" };
        let clean_domain = domain.trim_start_matches('.');
        let cookie_path = if path.is_empty() { "/" } else { path };
        let url_str = format!("{scheme}://{clean_domain}{cookie_path}");
        let Ok(url) = Url::parse(&url_str) else {
            continue;
        };

        let mut cookie_str = format!("{name}={value}; Domain={domain}; Path={cookie_path}");
        if secure {
            cookie_str.push_str("; Secure");
        }
        if expires > 0 {
            if let Some(dt) = DateTime::<Utc>::from_timestamp(expires, 0) {
                cookie_str.push_str(&format!("; Expires={}", dt.to_rfc2822()));
            }
        }

        if let Ok(raw) = RawCookie::parse(cookie_str) {
            let _ = store.insert_raw(&raw, &url);
        }
    }

    store
}

/// Persist a cookie store to a Netscape cookie file.
pub fn save_cookie_store(path: &Path, store: &CookieStoreMutex) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut writer = BufWriter::new(file);
    writeln!(writer, "# Netscape HTTP Cookie File")?;

    let store = store
        .lock()
        .map_err(|_| std::io::Error::other("cookie store lock poisoned"))?;

    for cookie in store.iter_any() {
        let (domain, include_subdomains) = match &cookie.domain {
            CookieDomain::HostOnly(host) => (host.as_str(), "FALSE"),
            CookieDomain::Suffix(suffix) => (suffix.as_str(), "TRUE"),
            CookieDomain::NotPresent | CookieDomain::Empty => ("", "FALSE"),
        };
        if domain.is_empty() {
            continue;
        }

        let domain = if include_subdomains == "TRUE" {
            format!(".{domain}")
        } else {
            domain.to_string()
        };
        let path = cookie.path.as_ref();
        let secure = if cookie.secure().unwrap_or(false) { "TRUE" } else { "FALSE" };
        let expires = match cookie.expires {
            CookieExpiration::AtUtc(dt) => dt.unix_timestamp().max(0) as u64,
            CookieExpiration::SessionEnd => 0,
        };
        writeln!(
            writer,
            "{domain}\t{include_subdomains}\t{path}\t{secure}\t{expires}\t{}\t{}",
            cookie.name(),
            cookie.value()
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_content() {
        let store = parse_cookie_content("");
        assert_eq!(store.iter_any().count(), 0);
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
        let store = parse_cookie_content(content);
        assert_eq!(store.iter_any().count(), 1);
    }

    #[test]
    fn parse_secure_cookie() {
        let content = ".secure.example.com\tTRUE\t/api\tTRUE\t1700000000\ttoken\tsecret";
        let store = parse_cookie_content(content);
        assert_eq!(store.iter_any().count(), 1);
    }

    #[test]
    fn parse_multiple_cookies() {
        let content = r#"# Cookie file
.example.com	TRUE	/	FALSE	0	session	abc
.example.com	TRUE	/	FALSE	0	user	john
.other.com	FALSE	/path	TRUE	1700000000	auth	xyz"#;
        let store = parse_cookie_content(content);
        assert_eq!(store.iter_any().count(), 3);
    }

    #[test]
    fn skip_invalid_lines() {
        let content = r#".example.com	TRUE	/	FALSE	0	session	abc
this is not a valid cookie line
only	three	fields
.valid.com	TRUE	/	FALSE	0	name	value"#;
        // Should not panic — invalid lines silently skipped.
        let store = parse_cookie_content(content);
        assert_eq!(store.iter_any().count(), 2);
    }

    #[test]
    fn parse_subdomain_cookie() {
        let content = "example.com\tFALSE\t/\tFALSE\t0\tcsrf\ttoken123";
        let store = parse_cookie_content(content);
        assert_eq!(store.iter_any().count(), 1);
    }

    #[test]
    fn load_nonexistent_file_returns_error() {
        let result = load_cookie_store(Path::new("/nonexistent/cookies.txt"));
        assert!(result.is_err());
    }
}
