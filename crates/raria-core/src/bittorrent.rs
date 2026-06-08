use bendy::decoding::{Error as BencodeError, FromBencode, Object, ResultExt};
use sha1::{Digest, Sha1};

use crate::{Error, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TorrentMeta {
    pub name: String,
    pub info_hash_hex: String,
    pub piece_length: u64,
    pub total_length: u64,
    pub files: Vec<TorrentFile>,
    pub announce: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TorrentFile {
    pub path: String,
    pub length: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MagnetMeta {
    pub info_hash_hex: String,
    pub name: Option<String>,
    pub trackers: Vec<String>,
}

pub fn parse_torrent_bytes(bytes: &[u8]) -> Result<TorrentMeta> {
    let raw_info = raw_info_dict(bytes)?;
    let decoded = DecodedTorrent::from_bencode(bytes)
        .map_err(|error| Error::Download(format!("invalid torrent metadata: {error}")))?;
    let info_hash_hex = format!("{:x}", Sha1::digest(raw_info));
    let files = decoded.info.files.unwrap_or_else(|| {
        vec![DecodedTorrentFile {
            length: decoded.info.length.unwrap_or(0),
            path: vec![decoded.info.name.clone()],
        }]
    });
    let files = files
        .into_iter()
        .map(|file| TorrentFile {
            path: file.path.join("/"),
            length: file.length,
        })
        .collect::<Vec<_>>();
    let total_length = files.iter().map(|file| file.length).sum();
    Ok(TorrentMeta {
        name: decoded.info.name,
        info_hash_hex,
        piece_length: decoded.info.piece_length,
        total_length,
        files,
        announce: decoded.announce,
    })
}

pub fn parse_magnet_uri(uri: &str) -> Result<MagnetMeta> {
    let parsed = url::Url::parse(uri)
        .map_err(|error| Error::Download(format!("invalid magnet URI: {error}")))?;
    if parsed.scheme() != "magnet" {
        return Err(Error::Download("magnet URI must use magnet scheme".into()));
    }
    let mut info_hash_hex = None;
    let mut name = None;
    let mut trackers = Vec::new();
    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "xt" => {
                if let Some(hash) = value.strip_prefix("urn:btih:") {
                    info_hash_hex = Some(hash.to_ascii_lowercase());
                }
            }
            "dn" => name = Some(value.into_owned()),
            "tr" => trackers.push(value.into_owned()),
            _ => {}
        }
    }
    let info_hash_hex =
        info_hash_hex.ok_or_else(|| Error::Download("magnet URI is missing btih".into()))?;
    Ok(MagnetMeta {
        info_hash_hex,
        name,
        trackers,
    })
}

#[derive(Debug)]
struct DecodedTorrent {
    announce: Option<String>,
    info: DecodedTorrentInfo,
}

#[derive(Debug)]
struct DecodedTorrentInfo {
    name: String,
    piece_length: u64,
    length: Option<u64>,
    files: Option<Vec<DecodedTorrentFile>>,
}

#[derive(Debug)]
struct DecodedTorrentFile {
    length: u64,
    path: Vec<String>,
}

impl FromBencode for DecodedTorrent {
    const EXPECTED_RECURSION_DEPTH: usize = 8;

    fn decode_bencode_object(object: Object) -> std::result::Result<Self, BencodeError> {
        let mut announce = None;
        let mut info = None;
        let mut dict = object.try_into_dictionary()?;
        while let Some((key, value)) = dict.next_pair()? {
            match key {
                b"announce" => {
                    announce = String::decode_bencode_object(value)
                        .context("announce")
                        .map(Some)?;
                }
                b"info" => {
                    info = DecodedTorrentInfo::decode_bencode_object(value)
                        .context("info")
                        .map(Some)?;
                }
                _ => {}
            }
        }
        Ok(Self {
            announce,
            info: info.ok_or_else(|| BencodeError::missing_field("info"))?,
        })
    }
}

impl FromBencode for DecodedTorrentInfo {
    const EXPECTED_RECURSION_DEPTH: usize = 6;

    fn decode_bencode_object(object: Object) -> std::result::Result<Self, BencodeError> {
        let mut name = None;
        let mut piece_length = None;
        let mut length = None;
        let mut files = None;
        let mut dict = object.try_into_dictionary()?;
        while let Some((key, value)) = dict.next_pair()? {
            match key {
                b"name" => {
                    name = String::decode_bencode_object(value)
                        .context("name")
                        .map(Some)?;
                }
                b"piece length" => {
                    piece_length = u64::decode_bencode_object(value)
                        .context("piece length")
                        .map(Some)?;
                }
                b"length" => {
                    length = u64::decode_bencode_object(value)
                        .context("length")
                        .map(Some)?;
                }
                b"files" => {
                    files = Vec::<DecodedTorrentFile>::decode_bencode_object(value)
                        .context("files")
                        .map(Some)?;
                }
                _ => {}
            }
        }
        Ok(Self {
            name: name.ok_or_else(|| BencodeError::missing_field("name"))?,
            piece_length: piece_length
                .ok_or_else(|| BencodeError::missing_field("piece length"))?,
            length,
            files,
        })
    }
}

impl FromBencode for DecodedTorrentFile {
    const EXPECTED_RECURSION_DEPTH: usize = 2;

    fn decode_bencode_object(object: Object) -> std::result::Result<Self, BencodeError> {
        let mut length = None;
        let mut path = None;
        let mut dict = object.try_into_dictionary()?;
        while let Some((key, value)) = dict.next_pair()? {
            match key {
                b"length" => {
                    length = u64::decode_bencode_object(value)
                        .context("length")
                        .map(Some)?;
                }
                b"path" => {
                    path = Vec::<String>::decode_bencode_object(value)
                        .context("path")
                        .map(Some)?;
                }
                _ => {}
            }
        }
        Ok(Self {
            length: length.ok_or_else(|| BencodeError::missing_field("length"))?,
            path: path.ok_or_else(|| BencodeError::missing_field("path"))?,
        })
    }
}

fn raw_info_dict(bytes: &[u8]) -> Result<&[u8]> {
    let Some(key_start) = bytes.windows(6).position(|window| window == b"4:info") else {
        return Err(Error::Download(
            "torrent metadata is missing info dict".into(),
        ));
    };
    let start = key_start + 6;
    let end = bencode_value_end(bytes, start)?;
    Ok(&bytes[start..end])
}

fn bencode_value_end(bytes: &[u8], start: usize) -> Result<usize> {
    let tag = *bytes
        .get(start)
        .ok_or_else(|| Error::Download("unexpected end of bencode".into()))?;
    match tag {
        b'i' => bytes[start..]
            .iter()
            .position(|byte| *byte == b'e')
            .map(|offset| start + offset + 1)
            .ok_or_else(|| Error::Download("unterminated bencode integer".into())),
        b'l' | b'd' => {
            let mut offset = start + 1;
            while bytes.get(offset) != Some(&b'e') {
                offset = bencode_value_end(bytes, offset)?;
            }
            Ok(offset + 1)
        }
        b'0'..=b'9' => {
            let colon = bytes[start..]
                .iter()
                .position(|byte| *byte == b':')
                .ok_or_else(|| Error::Download("invalid bencode byte string".into()))?;
            let colon = start + colon;
            let len = std::str::from_utf8(&bytes[start..colon])
                .map_err(|error| Error::Download(error.to_string()))?
                .parse::<usize>()
                .map_err(|error| Error::Download(error.to_string()))?;
            Ok(colon + 1 + len)
        }
        _ => Err(Error::Download("invalid bencode value".into())),
    }
}
