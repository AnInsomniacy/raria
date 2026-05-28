//! ED2K typed tag codec.

use crate::hash::Ed2kHash;
use crate::wire::Cursor;

const TAG_HASH16: u8 = 0x01;
const TAG_STRING: u8 = 0x02;
const TAG_UINT32: u8 = 0x03;
const TAG_BOOL: u8 = 0x05;
const TAG_BLOB: u8 = 0x07;
const TAG_UINT16: u8 = 0x08;
const TAG_UINT8: u8 = 0x09;
const TAG_BSOB: u8 = 0x0a;
const TAG_UINT64: u8 = 0x0b;
const TAG_STR1: u8 = 0x11;
const TAG_STR16: u8 = 0x20;

/// ED2K tag name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagName {
    /// Compact one-byte tag identifier.
    Id(u8),
    /// Text tag name.
    Text(String),
}

/// ED2K tag value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagValue {
    /// UTF-8 string value.
    String(String),
    /// Eight-bit unsigned integer.
    UInt8(u8),
    /// Sixteen-bit unsigned integer.
    UInt16(u16),
    /// Thirty-two-bit unsigned integer.
    UInt32(u32),
    /// Sixty-four-bit unsigned integer.
    UInt64(u64),
    /// Boolean value.
    Bool(bool),
    /// ED2K 16-byte hash value.
    Hash(Ed2kHash),
    /// Binary value.
    Binary(Vec<u8>),
}

/// ED2K typed tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// Tag name.
    pub name: TagName,
    /// Tag value.
    pub value: TagValue,
}

impl Tag {
    /// Create a typed ED2K tag.
    pub fn new(name: TagName, value: TagValue) -> Self {
        Self { name, value }
    }
}

/// Tag codec error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TagError {
    /// The tag payload is truncated.
    #[error("truncated ED2K tag")]
    Truncated,
    /// The tag name cannot be encoded.
    #[error("invalid ED2K tag name")]
    InvalidName,
    /// The tag type is not retained by raria.
    #[error("unsupported ED2K tag type: 0x{0:02x}")]
    UnsupportedType(u8),
    /// String data is not valid UTF-8.
    #[error("invalid ED2K tag UTF-8")]
    InvalidUtf8,
    /// The encoded value is too large for the retained wire width.
    #[error("ED2K tag value is too large")]
    ValueTooLarge,
}

/// Encode a typed ED2K tag.
pub fn encode_tag(tag: &Tag) -> Result<Vec<u8>, TagError> {
    let tag_type = tag_type_for_value(&tag.value);
    let mut out = Vec::new();
    write_name(&mut out, tag_type, &tag.name)?;
    write_value(&mut out, &tag.value)?;
    Ok(out)
}

/// Decode a single typed ED2K tag.
pub fn decode_tag(input: &[u8]) -> Result<Tag, TagError> {
    let mut cursor = Cursor::new(input);
    let raw_type = cursor.read_u8().ok_or(TagError::Truncated)?;
    let tag_type = raw_type & 0x7f;
    let name = read_name(&mut cursor, raw_type)?;
    let value = read_value(&mut cursor, tag_type)?;
    if !cursor.is_done() {
        return Err(TagError::ValueTooLarge);
    }
    Ok(Tag { name, value })
}

fn tag_type_for_value(value: &TagValue) -> u8 {
    match value {
        TagValue::String(value) if (1..=16).contains(&value.len()) => {
            TAG_STR1 + u8::try_from(value.len()).expect("short string length") - 1
        }
        TagValue::String(_) => TAG_STRING,
        TagValue::UInt8(_) => TAG_UINT8,
        TagValue::UInt16(_) => TAG_UINT16,
        TagValue::UInt32(_) => TAG_UINT32,
        TagValue::UInt64(_) => TAG_UINT64,
        TagValue::Bool(_) => TAG_BOOL,
        TagValue::Hash(_) => TAG_HASH16,
        TagValue::Binary(value) if value.len() <= usize::from(u8::MAX) => TAG_BSOB,
        TagValue::Binary(_) => TAG_BLOB,
    }
}

fn write_name(out: &mut Vec<u8>, tag_type: u8, name: &TagName) -> Result<(), TagError> {
    match name {
        TagName::Id(id) => {
            out.push(tag_type | 0x80);
            out.push(*id);
        }
        TagName::Text(name) => {
            if name.len() == 1 || name.len() > usize::from(u16::MAX) {
                return Err(TagError::InvalidName);
            }
            out.push(tag_type);
            out.extend_from_slice(
                &u16::try_from(name.len())
                    .map_err(|_| TagError::InvalidName)?
                    .to_le_bytes(),
            );
            out.extend_from_slice(name.as_bytes());
        }
    }
    Ok(())
}

fn read_name(cursor: &mut Cursor<'_>, raw_type: u8) -> Result<TagName, TagError> {
    if raw_type & 0x80 != 0 {
        let id = cursor.read_u8().ok_or(TagError::Truncated)?;
        return Ok(TagName::Id(id));
    }
    let len = cursor.read_u16().ok_or(TagError::Truncated)? as usize;
    if len == 1 {
        let id = cursor.read_u8().ok_or(TagError::Truncated)?;
        return Ok(TagName::Id(id));
    }
    let bytes = cursor.read_exact(len).ok_or(TagError::Truncated)?;
    let name = String::from_utf8(bytes.to_vec()).map_err(|_| TagError::InvalidUtf8)?;
    Ok(TagName::Text(name))
}

fn write_value(out: &mut Vec<u8>, value: &TagValue) -> Result<(), TagError> {
    match value {
        TagValue::String(value) => {
            if value.len() > usize::from(u16::MAX) {
                return Err(TagError::ValueTooLarge);
            }
            if value.len() > 16 {
                out.extend_from_slice(
                    &u16::try_from(value.len())
                        .map_err(|_| TagError::ValueTooLarge)?
                        .to_le_bytes(),
                );
            }
            out.extend_from_slice(value.as_bytes());
        }
        TagValue::UInt8(value) => out.push(*value),
        TagValue::UInt16(value) => out.extend_from_slice(&value.to_le_bytes()),
        TagValue::UInt32(value) => out.extend_from_slice(&value.to_le_bytes()),
        TagValue::UInt64(value) => out.extend_from_slice(&value.to_le_bytes()),
        TagValue::Bool(value) => out.push(u8::from(*value)),
        TagValue::Hash(value) => out.extend_from_slice(value),
        TagValue::Binary(value) if value.len() <= usize::from(u8::MAX) => {
            out.push(u8::try_from(value.len()).map_err(|_| TagError::ValueTooLarge)?);
            out.extend_from_slice(value);
        }
        TagValue::Binary(value) => {
            out.extend_from_slice(
                &u32::try_from(value.len())
                    .map_err(|_| TagError::ValueTooLarge)?
                    .to_le_bytes(),
            );
            out.extend_from_slice(value);
        }
    }
    Ok(())
}

fn read_value(cursor: &mut Cursor<'_>, tag_type: u8) -> Result<TagValue, TagError> {
    Ok(match tag_type {
        TAG_HASH16 => TagValue::Hash(cursor.read_hash16().ok_or(TagError::Truncated)?),
        TAG_STRING => {
            let len = cursor.read_u16().ok_or(TagError::Truncated)? as usize;
            TagValue::String(read_string(cursor, len)?)
        }
        TAG_UINT32 => TagValue::UInt32(cursor.read_u32().ok_or(TagError::Truncated)?),
        TAG_BOOL => TagValue::Bool(cursor.read_u8().ok_or(TagError::Truncated)? != 0),
        TAG_BLOB => {
            let len = cursor.read_u32().ok_or(TagError::Truncated)? as usize;
            TagValue::Binary(read_bytes(cursor, len)?.to_vec())
        }
        TAG_UINT16 => TagValue::UInt16(cursor.read_u16().ok_or(TagError::Truncated)?),
        TAG_UINT8 => TagValue::UInt8(cursor.read_u8().ok_or(TagError::Truncated)?),
        TAG_BSOB => {
            let len = cursor.read_u8().ok_or(TagError::Truncated)? as usize;
            TagValue::Binary(read_bytes(cursor, len)?.to_vec())
        }
        TAG_UINT64 => TagValue::UInt64(cursor.read_u64().ok_or(TagError::Truncated)?),
        TAG_STR1..=TAG_STR16 => {
            let len = usize::from(tag_type - TAG_STR1 + 1);
            TagValue::String(read_string(cursor, len)?)
        }
        _ => return Err(TagError::UnsupportedType(tag_type)),
    })
}

fn read_string(cursor: &mut Cursor<'_>, len: usize) -> Result<String, TagError> {
    String::from_utf8(read_bytes(cursor, len)?.to_vec()).map_err(|_| TagError::InvalidUtf8)
}

fn read_bytes<'a>(cursor: &mut Cursor<'a>, len: usize) -> Result<&'a [u8], TagError> {
    cursor.read_exact(len).ok_or(TagError::Truncated)
}
