//! ED2K server search request and result ownership.

use crate::hash::Ed2kHash;
use crate::tag::{Tag, TagName, TagValue, decode_tag_prefix, encode_tag};
use crate::wire::{Cursor, ipv4_from_server_met};

const TAG_FILE_NAME: u8 = 0x01;
const TAG_FILE_SIZE: u8 = 0x02;
const TAG_FILE_TYPE: u8 = 0x03;
const TAG_FILE_EXTENSION: u8 = 0x04;
const TAG_SOURCES: u8 = 0x15;
const TAG_COMPLETE_SOURCES: u8 = 0x30;
const TAG_FILE_SIZE_HI: u8 = 0x3a;
const MAX_SEARCH_TERMS: usize = 30;
const MAX_SEARCH_RESULTS: u32 = 10_000;

/// Native server search query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ed2kServerSearchQuery {
    /// Required keyword query.
    pub keyword: String,
    /// Optional retained ED2K file type.
    pub file_type: Option<String>,
    /// Optional file extension filter.
    pub extension: Option<String>,
    /// Optional minimum file size.
    pub min_size: Option<u64>,
    /// Optional maximum file size.
    pub max_size: Option<u64>,
    /// Optional minimum source count.
    pub min_source_count: Option<u32>,
    /// Optional minimum complete-source count.
    pub min_complete_source_count: Option<u32>,
}

/// Parsed native ED2K server search result set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kServerSearchResults {
    /// Parsed result entries.
    pub entries: Vec<Ed2kServerSearchResult>,
    /// Whether the server advertised more results.
    pub more_results: bool,
}

/// Parsed native ED2K server search result entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kServerSearchResult {
    /// File hash.
    pub hash: Ed2kHash,
    /// File name.
    pub name: String,
    /// File size in bytes.
    pub size: u64,
    /// Reported source count.
    pub source_count: u32,
    /// Reported complete-source count.
    pub complete_source_count: u32,
    /// Optional retained file type.
    pub file_type: Option<String>,
    /// Optional file extension.
    pub extension: Option<String>,
    /// Source network label supplied by the caller.
    pub source_network: String,
    /// Direct source endpoints carried by the result.
    pub sources: Vec<SearchResultSource>,
    /// Startable native ED2K file URI.
    pub ed2k_uri: String,
}

/// Direct source endpoint carried by a server search result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResultSource {
    /// Source host.
    pub host: String,
    /// Source TCP port.
    pub port: u16,
}

/// ED2K server search codec error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Ed2kServerSearchError {
    /// Search query is invalid.
    #[error("invalid ED2K server search query")]
    InvalidQuery,
    /// Search payload is malformed.
    #[error("invalid ED2K server search payload")]
    InvalidPayload,
    /// Search payload is too large for retained bounds.
    #[error("ED2K server search payload is too large")]
    PayloadTooLarge,
    /// A tag could not be encoded or decoded.
    #[error("invalid ED2K server search tag")]
    InvalidTag,
}

/// Build an ED2K server keyword-search request payload.
pub fn build_server_search_request(
    query: &Ed2kServerSearchQuery,
) -> Result<Vec<u8>, Ed2kServerSearchError> {
    let mut terms = Vec::<SearchTerm>::new();
    let keyword = query.keyword.trim();
    if keyword.is_empty() || keyword.len() > 450 {
        return Err(Ed2kServerSearchError::InvalidQuery);
    }
    terms.push(SearchTerm::Keyword(keyword.to_string()));
    if let Some(file_type) = trimmed_non_empty(&query.file_type) {
        terms.push(SearchTerm::StringTag {
            tag: TAG_FILE_TYPE,
            value: file_type,
        });
    }
    if let Some(min_size) = query.min_size.filter(|value| *value > 0) {
        terms.push(SearchTerm::NumberTag {
            tag: TAG_FILE_SIZE,
            operator: 0x01,
            value: min_size,
        });
    }
    if let Some(max_size) = query.max_size.filter(|value| *value > 0) {
        terms.push(SearchTerm::NumberTag {
            tag: TAG_FILE_SIZE,
            operator: 0x02,
            value: max_size,
        });
    }
    if let Some(min_sources) = query.min_source_count.filter(|value| *value > 0) {
        terms.push(SearchTerm::NumberTag {
            tag: TAG_SOURCES,
            operator: 0x01,
            value: u64::from(min_sources),
        });
    }
    if let Some(min_complete) = query.min_complete_source_count.filter(|value| *value > 0) {
        terms.push(SearchTerm::NumberTag {
            tag: TAG_COMPLETE_SOURCES,
            operator: 0x01,
            value: u64::from(min_complete),
        });
    }
    if let Some(extension) = trimmed_non_empty(&query.extension) {
        terms.push(SearchTerm::StringTag {
            tag: TAG_FILE_EXTENSION,
            value: extension,
        });
    }
    if terms.len() > MAX_SEARCH_TERMS {
        return Err(Ed2kServerSearchError::PayloadTooLarge);
    }

    let mut payload = Vec::new();
    for (index, term) in terms.iter().enumerate() {
        if index + 1 < terms.len() {
            payload.extend_from_slice(&[0, 0]);
        }
        encode_search_term(&mut payload, term)?;
    }
    Ok(payload)
}

/// Parse ED2K server search result payloads.
pub fn parse_server_search_results(
    payload: &[u8],
    source_network: &str,
) -> Result<Ed2kServerSearchResults, Ed2kServerSearchError> {
    let mut cursor = Cursor::new(payload);
    let count = cursor
        .read_u32()
        .ok_or(Ed2kServerSearchError::InvalidPayload)?;
    if count > MAX_SEARCH_RESULTS {
        return Err(Ed2kServerSearchError::PayloadTooLarge);
    }
    let mut entries = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let hash = cursor
            .read_hash16()
            .ok_or(Ed2kServerSearchError::InvalidPayload)?;
        let client_id = cursor
            .read_u32()
            .ok_or(Ed2kServerSearchError::InvalidPayload)?;
        let client_port = cursor
            .read_u16()
            .ok_or(Ed2kServerSearchError::InvalidPayload)?;
        let tag_count = cursor
            .read_u32()
            .ok_or(Ed2kServerSearchError::InvalidPayload)?;
        let mut tags = Vec::new();
        for _ in 0..tag_count {
            let (tag, consumed) = decode_tag_prefix(cursor.remaining_bytes())
                .map_err(|_| Ed2kServerSearchError::InvalidTag)?;
            cursor
                .read_exact(consumed)
                .ok_or(Ed2kServerSearchError::InvalidPayload)?;
            tags.push(tag);
        }
        let mut result = search_result_from_tags(hash, tags, source_network)?;
        if client_id > 0x00ff_ffff && client_port != 0 {
            result.sources.push(SearchResultSource {
                host: ipv4_from_server_met(client_id),
                port: client_port,
            });
        }
        result.ed2k_uri = file_link_for_result(&result);
        entries.push(result);
    }
    let more_results = match cursor.remaining() {
        0 => false,
        1 => cursor.read_u8().unwrap_or(0) != 0,
        _ => return Err(Ed2kServerSearchError::InvalidPayload),
    };
    Ok(Ed2kServerSearchResults {
        entries,
        more_results,
    })
}

/// Build a server search result payload for local fixtures.
pub fn build_server_search_result_payload(
    entries: &[Ed2kServerSearchResult],
) -> Result<Vec<u8>, Ed2kServerSearchError> {
    let count = u32::try_from(entries.len()).map_err(|_| Ed2kServerSearchError::PayloadTooLarge)?;
    let mut payload = Vec::new();
    payload.extend_from_slice(&count.to_le_bytes());
    for entry in entries {
        payload.extend_from_slice(&entry.hash);
        if let Some(source) = entry.sources.first() {
            payload.extend_from_slice(&source_host_to_wire(&source.host)?.to_le_bytes());
            payload.extend_from_slice(&source.port.to_le_bytes());
        } else {
            payload.extend_from_slice(&0_u32.to_le_bytes());
            payload.extend_from_slice(&0_u16.to_le_bytes());
        }
        let mut tags = vec![
            Tag::new(
                TagName::Id(TAG_FILE_NAME),
                TagValue::String(entry.name.clone()),
            ),
            Tag::new(
                TagName::Id(TAG_FILE_SIZE),
                TagValue::UInt32((entry.size & 0xffff_ffff) as u32),
            ),
            Tag::new(
                TagName::Id(TAG_SOURCES),
                TagValue::UInt32(entry.source_count),
            ),
            Tag::new(
                TagName::Id(TAG_COMPLETE_SOURCES),
                TagValue::UInt32(entry.complete_source_count),
            ),
        ];
        if entry.size > u64::from(u32::MAX) {
            tags.push(Tag::new(
                TagName::Id(TAG_FILE_SIZE_HI),
                TagValue::UInt32((entry.size >> 32) as u32),
            ));
        }
        if let Some(file_type) = &entry.file_type {
            tags.push(Tag::new(
                TagName::Id(TAG_FILE_TYPE),
                TagValue::String(file_type.clone()),
            ));
        }
        if let Some(extension) = &entry.extension {
            tags.push(Tag::new(
                TagName::Id(TAG_FILE_EXTENSION),
                TagValue::String(extension.clone()),
            ));
        }
        payload.extend_from_slice(
            &u32::try_from(tags.len())
                .map_err(|_| Ed2kServerSearchError::PayloadTooLarge)?
                .to_le_bytes(),
        );
        for tag in tags {
            payload.extend_from_slice(
                &encode_tag(&tag).map_err(|_| Ed2kServerSearchError::InvalidTag)?,
            );
        }
    }
    Ok(payload)
}

enum SearchTerm {
    Keyword(String),
    StringTag { tag: u8, value: String },
    NumberTag { tag: u8, operator: u8, value: u64 },
}

fn encode_search_term(
    payload: &mut Vec<u8>,
    term: &SearchTerm,
) -> Result<(), Ed2kServerSearchError> {
    match term {
        SearchTerm::Keyword(value) => {
            payload.push(0x01);
            write_string(payload, value, 450)?;
        }
        SearchTerm::StringTag { tag, value } => {
            payload.push(0x02);
            write_string(payload, value, 20)?;
            payload.extend_from_slice(&1_u16.to_le_bytes());
            payload.push(*tag);
        }
        SearchTerm::NumberTag {
            tag,
            operator,
            value,
        } => {
            if *value > u64::from(u32::MAX) {
                payload.push(0x08);
                payload.extend_from_slice(&value.to_le_bytes());
            } else {
                payload.push(0x03);
                payload.extend_from_slice(&(*value as u32).to_le_bytes());
            }
            payload.push(*operator);
            payload.extend_from_slice(&1_u16.to_le_bytes());
            payload.push(*tag);
        }
    }
    Ok(())
}

fn write_string(
    payload: &mut Vec<u8>,
    value: &str,
    max_len: usize,
) -> Result<(), Ed2kServerSearchError> {
    if value.is_empty() || value.len() > max_len || value.len() > usize::from(u16::MAX) {
        return Err(Ed2kServerSearchError::InvalidQuery);
    }
    payload.extend_from_slice(
        &u16::try_from(value.len())
            .map_err(|_| Ed2kServerSearchError::PayloadTooLarge)?
            .to_le_bytes(),
    );
    payload.extend_from_slice(value.as_bytes());
    Ok(())
}

fn search_result_from_tags(
    hash: Ed2kHash,
    tags: Vec<Tag>,
    source_network: &str,
) -> Result<Ed2kServerSearchResult, Ed2kServerSearchError> {
    let mut name = None;
    let mut size_low = 0_u64;
    let mut size_high = 0_u64;
    let mut source_count = 0_u32;
    let mut complete_source_count = 0_u32;
    let mut file_type = None;
    let mut extension = None;

    for tag in tags {
        match (tag.name, tag.value) {
            (TagName::Id(TAG_FILE_NAME), TagValue::String(value)) => name = Some(value),
            (TagName::Id(TAG_FILE_SIZE), value) => {
                size_low = numeric_tag(value).ok_or(Ed2kServerSearchError::InvalidPayload)?;
            }
            (TagName::Id(TAG_FILE_SIZE_HI), value) => {
                size_high = numeric_tag(value).ok_or(Ed2kServerSearchError::InvalidPayload)?;
            }
            (TagName::Id(TAG_SOURCES), value) => {
                source_count =
                    u32::try_from(numeric_tag(value).ok_or(Ed2kServerSearchError::InvalidPayload)?)
                        .map_err(|_| Ed2kServerSearchError::InvalidPayload)?;
            }
            (TagName::Id(TAG_COMPLETE_SOURCES), value) => {
                complete_source_count =
                    u32::try_from(numeric_tag(value).ok_or(Ed2kServerSearchError::InvalidPayload)?)
                        .map_err(|_| Ed2kServerSearchError::InvalidPayload)?;
            }
            (TagName::Id(TAG_FILE_TYPE), TagValue::String(value)) => file_type = Some(value),
            (TagName::Id(TAG_FILE_EXTENSION), TagValue::String(value)) => extension = Some(value),
            _ => {}
        }
    }

    let name = name.ok_or(Ed2kServerSearchError::InvalidPayload)?;
    if name.is_empty() {
        return Err(Ed2kServerSearchError::InvalidPayload);
    }
    Ok(Ed2kServerSearchResult {
        hash,
        name,
        size: size_low | (size_high << 32),
        source_count,
        complete_source_count,
        file_type,
        extension,
        source_network: source_network.to_string(),
        sources: Vec::new(),
        ed2k_uri: String::new(),
    })
}

fn numeric_tag(value: TagValue) -> Option<u64> {
    match value {
        TagValue::UInt8(value) => Some(u64::from(value)),
        TagValue::UInt16(value) => Some(u64::from(value)),
        TagValue::UInt32(value) => Some(u64::from(value)),
        TagValue::UInt64(value) => Some(value),
        _ => None,
    }
}

fn trimmed_non_empty(value: &Option<String>) -> Option<String> {
    let value = value.as_ref()?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn file_link_for_result(result: &Ed2kServerSearchResult) -> String {
    let mut uri = format!(
        "ed2k://|file|{}|{}|{}|",
        percent_encode(&result.name),
        result.size,
        hex_hash(result.hash)
    );
    if !result.sources.is_empty() {
        uri.push_str("sources");
        for source in &result.sources {
            uri.push(',');
            uri.push_str(&percent_encode(&source.host));
            uri.push(':');
            uri.push_str(&source.port.to_string());
        }
        uri.push('|');
    }
    uri.push('/');
    uri
}

fn percent_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
            output.push(byte as char);
        } else {
            output.push('%');
            output.push(HEX[(byte >> 4) as usize] as char);
            output.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    output
}

fn hex_hash(hash: Ed2kHash) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(32);
    for byte in hash {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn source_host_to_wire(host: &str) -> Result<u32, Ed2kServerSearchError> {
    let octets = host
        .parse::<std::net::Ipv4Addr>()
        .map_err(|_| Ed2kServerSearchError::InvalidPayload)?
        .octets();
    Ok(u32::from_le_bytes(octets))
}
