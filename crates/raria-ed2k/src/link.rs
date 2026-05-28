//! ED2K link parsing and normalized task/search model ownership.

use serde::{Deserialize, Serialize};

/// Maximum ED2K file size accepted by modern eMule-compatible links.
pub const MAX_ED2K_FILE_SIZE: u64 = 1_u64 << 38;

/// Parsed native ED2K link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Ed2kLink {
    /// File download link.
    File(Ed2kFileLink),
    /// Server bootstrap link.
    Server(Ed2kServerLink),
    /// Server list bootstrap link.
    ServerList(Ed2kUrlLink),
    /// Kad nodes list bootstrap link.
    NodesList(Ed2kUrlLink),
    /// Native ED2K search link.
    Search(Ed2kSearchLink),
}

/// Parsed ED2K file link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ed2kFileLink {
    /// Safe file name after percent decoding and path-separator normalization.
    pub name: String,
    /// File size in bytes.
    pub size: u64,
    /// ED2K root hash.
    pub root_hash: [u8; 16],
    /// Optional ED2K part hashes.
    pub part_hashes: Vec<[u8; 16]>,
    /// Optional AICH root in canonical Base32 text form.
    pub aich_root: Option<String>,
    /// Inline sources carried by the link.
    pub sources: Vec<Ed2kLinkSource>,
}

/// Inline ED2K source carried by a file link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ed2kLinkSource {
    /// Source host or address.
    pub host: String,
    /// Source TCP port.
    pub port: u16,
    /// Optional crypt option byte carried by the source.
    pub crypt_options: Option<u8>,
    /// Optional source client hash.
    pub client_hash: Option<[u8; 16]>,
}

/// Parsed ED2K server link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ed2kServerLink {
    /// Server host or address.
    pub host: String,
    /// Server TCP port.
    pub port: u16,
}

/// Parsed ED2K URL metadata link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ed2kUrlLink {
    /// Bootstrap resource URI.
    pub uri: String,
}

/// Parsed ED2K search link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ed2kSearchLink {
    /// Search query text.
    pub query: String,
}

/// ED2K link parse error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Ed2kLinkError {
    /// The input is not an ED2K link in pipe-delimited form.
    #[error("invalid ED2K link format")]
    InvalidFormat,
    /// The link type is not supported by raria.
    #[error("unsupported ED2K link type")]
    UnsupportedType,
    /// A required text field is empty.
    #[error("empty ED2K link field")]
    EmptyField,
    /// A numeric field is invalid or out of range.
    #[error("invalid ED2K numeric field")]
    InvalidNumber,
    /// A hash field is invalid.
    #[error("invalid ED2K hash")]
    InvalidHash,
    /// An AICH root is invalid.
    #[error("invalid ED2K AICH root")]
    InvalidAichRoot,
    /// Percent encoding is invalid or not UTF-8.
    #[error("invalid ED2K percent encoding")]
    InvalidPercentEncoding,
}

/// Parse an ED2K link into a native model.
pub fn parse_link(input: &str) -> Result<Ed2kLink, Ed2kLinkError> {
    let fields: Vec<&str> = input.trim().split('|').collect();
    if fields.len() < 4 || fields.first() != Some(&"ed2k://") || fields.last() != Some(&"/") {
        return Err(Ed2kLinkError::InvalidFormat);
    }

    match fields[1].to_ascii_lowercase().as_str() {
        "file" => parse_file_link(&fields).map(Ed2kLink::File),
        "server" => parse_server_link(&fields).map(Ed2kLink::Server),
        "serverlist" => parse_url_link(&fields).map(Ed2kLink::ServerList),
        "nodeslist" => parse_url_link(&fields).map(Ed2kLink::NodesList),
        "search" => parse_search_link(&fields).map(Ed2kLink::Search),
        _ => Err(Ed2kLinkError::UnsupportedType),
    }
}

fn parse_file_link(fields: &[&str]) -> Result<Ed2kFileLink, Ed2kLinkError> {
    if fields.len() < 6 {
        return Err(Ed2kLinkError::InvalidFormat);
    }
    let name = safe_name(&percent_decode(fields[2].trim())?)?;
    let size = parse_size(fields[3].trim())?;
    let root_hash = parse_hash16(fields[4].trim())?;
    let mut part_hashes = Vec::new();
    let mut aich_root = None;
    let mut sources = Vec::new();

    for option in &fields[5..fields.len() - 1] {
        let option = option.trim();
        let lower = option.to_ascii_lowercase();
        if lower.starts_with("p=") {
            part_hashes = parse_part_hashes(&option[2..])?;
        } else if lower.starts_with("h=") {
            aich_root = Some(parse_aich_root(&option[2..])?);
        } else if lower.starts_with("sources,") {
            sources = parse_sources(option)?;
        }
    }

    Ok(Ed2kFileLink {
        name,
        size,
        root_hash,
        part_hashes,
        aich_root,
        sources,
    })
}

fn parse_server_link(fields: &[&str]) -> Result<Ed2kServerLink, Ed2kLinkError> {
    if fields.len() != 5 {
        return Err(Ed2kLinkError::InvalidFormat);
    }
    let host = percent_decode(fields[2].trim())?;
    if host.is_empty() {
        return Err(Ed2kLinkError::EmptyField);
    }
    Ok(Ed2kServerLink {
        host,
        port: parse_port(fields[3].trim())?,
    })
}

fn parse_url_link(fields: &[&str]) -> Result<Ed2kUrlLink, Ed2kLinkError> {
    if fields.len() != 4 {
        return Err(Ed2kLinkError::InvalidFormat);
    }
    let uri = percent_decode(fields[2].trim())?;
    if uri.is_empty() {
        return Err(Ed2kLinkError::EmptyField);
    }
    Ok(Ed2kUrlLink { uri })
}

fn parse_search_link(fields: &[&str]) -> Result<Ed2kSearchLink, Ed2kLinkError> {
    if fields.len() != 4 {
        return Err(Ed2kLinkError::InvalidFormat);
    }
    let query = percent_decode(fields[2].trim())?;
    if query.is_empty() {
        return Err(Ed2kLinkError::EmptyField);
    }
    Ok(Ed2kSearchLink { query })
}

fn parse_size(value: &str) -> Result<u64, Ed2kLinkError> {
    let size = value
        .parse::<u64>()
        .map_err(|_| Ed2kLinkError::InvalidNumber)?;
    if size == 0 || size > MAX_ED2K_FILE_SIZE {
        return Err(Ed2kLinkError::InvalidNumber);
    }
    Ok(size)
}

fn parse_port(value: &str) -> Result<u16, Ed2kLinkError> {
    let port = value
        .parse::<u16>()
        .map_err(|_| Ed2kLinkError::InvalidNumber)?;
    if port == 0 {
        return Err(Ed2kLinkError::InvalidNumber);
    }
    Ok(port)
}

fn parse_part_hashes(value: &str) -> Result<Vec<[u8; 16]>, Ed2kLinkError> {
    if value.is_empty() {
        return Err(Ed2kLinkError::InvalidHash);
    }
    value
        .split(':')
        .map(|hash| parse_hash16(hash.trim()))
        .collect()
}

fn parse_sources(value: &str) -> Result<Vec<Ed2kLinkSource>, Ed2kLinkError> {
    let mut sources = Vec::new();
    for source in value.split(',').skip(1) {
        if source.trim().is_empty() {
            return Err(Ed2kLinkError::EmptyField);
        }
        let parts: Vec<&str> = source.split(':').collect();
        if !(2..=4).contains(&parts.len()) {
            return Err(Ed2kLinkError::InvalidFormat);
        }
        let host = percent_decode(parts[0].trim())?;
        if host.is_empty() {
            return Err(Ed2kLinkError::EmptyField);
        }
        let port = parse_port(parts[1].trim())?;
        let crypt_options = if parts.len() >= 3 && !parts[2].trim().is_empty() {
            Some(
                parts[2]
                    .trim()
                    .parse::<u8>()
                    .map_err(|_| Ed2kLinkError::InvalidNumber)?,
            )
        } else {
            None
        };
        let client_hash = if parts.len() == 4 {
            Some(parse_hash16(parts[3].trim())?)
        } else {
            None
        };
        if crypt_options.is_some_and(|value| value & 0x80 != 0) && client_hash.is_none() {
            return Err(Ed2kLinkError::InvalidHash);
        }
        sources.push(Ed2kLinkSource {
            host,
            port,
            crypt_options,
            client_hash,
        });
    }
    if sources.is_empty() {
        return Err(Ed2kLinkError::EmptyField);
    }
    Ok(sources)
}

fn parse_hash16(value: &str) -> Result<[u8; 16], Ed2kLinkError> {
    if value.len() != 32 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(Ed2kLinkError::InvalidHash);
    }
    let mut hash = [0_u8; 16];
    for index in 0..16 {
        hash[index] = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| Ed2kLinkError::InvalidHash)?;
    }
    Ok(hash)
}

fn parse_aich_root(value: &str) -> Result<String, Ed2kLinkError> {
    let root = value.trim().to_ascii_uppercase();
    if root.len() != 32
        || !root
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ('2'..='7').contains(&ch))
    {
        return Err(Ed2kLinkError::InvalidAichRoot);
    }
    Ok(root)
}

fn safe_name(value: &str) -> Result<String, Ed2kLinkError> {
    let name = value.trim().replace(['/', '\\'], "_");
    if name.is_empty() {
        return Err(Ed2kLinkError::EmptyField);
    }
    Ok(name)
}

fn percent_decode(value: &str) -> Result<String, Ed2kLinkError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(Ed2kLinkError::InvalidPercentEncoding);
            }
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3])
                .map_err(|_| Ed2kLinkError::InvalidPercentEncoding)?;
            decoded.push(
                u8::from_str_radix(hex, 16).map_err(|_| Ed2kLinkError::InvalidPercentEncoding)?,
            );
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| Ed2kLinkError::InvalidPercentEncoding)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_file_link_with_integrity_and_sources() {
        let parsed = parse_link(
            "ed2k://|file|folder%2Fsample.iso|1234|0123456789ABCDEF0123456789ABCDEF|\
             p=11111111111111111111111111111111:22222222222222222222222222222222|\
             h=ABCDEFGHIJKLMNOPQRSTUVWXYZ234567|\
             sources,192.0.2.1:4662,203.0.113.7:4672:131:33333333333333333333333333333333|/",
        )
        .expect("file link");

        let Ed2kLink::File(file) = parsed else {
            panic!("expected file link");
        };
        assert_eq!(file.name, "folder_sample.iso");
        assert_eq!(file.size, 1234);
        assert_eq!(
            file.root_hash,
            [
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
                0xcd, 0xef
            ]
        );
        assert_eq!(file.part_hashes.len(), 2);
        assert_eq!(
            file.aich_root.as_deref(),
            Some("ABCDEFGHIJKLMNOPQRSTUVWXYZ234567")
        );
        assert_eq!(file.sources.len(), 2);
        assert_eq!(file.sources[0].host, "192.0.2.1");
        assert_eq!(file.sources[0].port, 4662);
        assert_eq!(file.sources[1].crypt_options, Some(131));
        assert_eq!(file.sources[1].client_hash, Some([0x33; 16]));
    }

    #[test]
    fn parses_metadata_links() {
        assert_eq!(
            parse_link("ed2k://|server|203.0.113.10|4661|/").expect("server"),
            Ed2kLink::Server(Ed2kServerLink {
                host: "203.0.113.10".into(),
                port: 4661,
            })
        );
        assert_eq!(
            parse_link("ed2k://|serverlist|https%3A%2F%2Fexample.test%2Fserver.met|/")
                .expect("server list"),
            Ed2kLink::ServerList(Ed2kUrlLink {
                uri: "https://example.test/server.met".into(),
            })
        );
        assert_eq!(
            parse_link("ed2k://|nodeslist|https%3A%2F%2Fexample.test%2Fnodes.dat|/")
                .expect("nodes list"),
            Ed2kLink::NodesList(Ed2kUrlLink {
                uri: "https://example.test/nodes.dat".into(),
            })
        );
        assert_eq!(
            parse_link("ed2k://|search|linux%20iso|/").expect("search"),
            Ed2kLink::Search(Ed2kSearchLink {
                query: "linux iso".into(),
            })
        );
    }

    #[test]
    fn rejects_malformed_file_links() {
        assert!(parse_link("ed2k://|file||123|0123456789abcdef0123456789abcdef|/").is_err());
        assert!(parse_link("ed2k://|file|x|0|0123456789abcdef0123456789abcdef|/").is_err());
        assert!(
            parse_link("ed2k://|file|x|274877906945|0123456789abcdef0123456789abcdef|/").is_err()
        );
        assert!(parse_link("ed2k://|file|x|1|not-a-hash|/").is_err());
        assert!(parse_link("ed2k://|file|x|1|0123456789abcdef0123456789abcdef|p=|/").is_err());
        assert!(parse_link("ed2k://|file|x|1|0123456789abcdef0123456789abcdef|h=ABC|/").is_err());
    }
}
