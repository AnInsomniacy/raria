//! Torrent metainfo extraction for WebSeed pre-download.
//!
//! Parses `.torrent` bencode structures using [`librqbit_bencode`] to extract
//! the file list, piece SHA-1 hashes, piece length, and WebSeed `url-list`
//! entries (BEP-17 / BEP-19).
//!
//! This module operates purely on raw torrent bytes and has **no dependency**
//! on librqbit's internal session or piece-tracking machinery.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use librqbit_bencode::{BencodeValue, dyn_from_bytes};
use tracing::warn;

// ----- Public types -----

/// Metadata extracted from a `.torrent` file for WebSeed pre-download.
#[derive(Debug)]
pub struct TorrentMeta {
    /// Human-readable torrent name (from `info.name`).
    pub name: String,
    /// Ordered file list with cumulative byte offsets.
    pub files: Vec<TorrentFile>,
    /// Piece length in bytes.
    pub piece_length: u64,
    /// SHA-1 hash for each piece (20 bytes each).
    pub piece_hashes: Vec<[u8; 20]>,
    /// Validated WebSeed URIs (HTTP/HTTPS/FTP/FTPS/SFTP).
    pub web_seed_uris: Vec<url::Url>,
    /// Whether this is a single-file torrent (affects URL construction).
    pub is_single_file: bool,
}

/// A single file within a torrent.
#[derive(Debug, Clone)]
pub struct TorrentFile {
    /// Relative path within the torrent output directory.
    pub path: PathBuf,
    /// File length in bytes.
    pub length: u64,
    /// Absolute byte offset within the torrent's concatenated data stream.
    pub offset: u64,
}

/// A contiguous byte range within a single file that belongs to a piece.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRange {
    /// Index into [`TorrentMeta::files`].
    pub file_index: usize,
    /// Byte offset within that file.
    pub file_offset: u64,
    /// Number of bytes to read from this file for the piece.
    pub length: u64,
}

// ----- Implementation -----

impl TorrentMeta {
    /// Parse a `.torrent` file from raw bencode bytes.
    pub fn from_bytes(torrent_bytes: &[u8]) -> Result<Self> {
        let root =
            dyn_from_bytes::<Vec<u8>>(torrent_bytes).context("invalid bencode in torrent file")?;

        let BencodeValue::Dict(top) = root else {
            bail!("torrent root is not a dict");
        };

        // Extract "info" dict.
        let info_val = top
            .get(b"info".as_slice())
            .context("missing 'info' key")?;
        let BencodeValue::Dict(info) = info_val else {
            bail!("'info' is not a dict");
        };

        // Name.
        let name = dict_get_string(info, b"name").context("missing or invalid 'info.name'")?;

        // Piece length.
        let piece_length =
            dict_get_int(info, b"piece length").context("missing 'info.piece length'")? as u64;

        // Piece hashes (concatenated 20-byte SHA-1 digests).
        let pieces_bytes = dict_get_bytes(info, b"pieces").context("missing 'info.pieces'")?;
        if pieces_bytes.len() % 20 != 0 {
            bail!(
                "info.pieces length {} is not a multiple of 20",
                pieces_bytes.len()
            );
        }
        let piece_hashes: Vec<[u8; 20]> = pieces_bytes
            .chunks_exact(20)
            .map(|chunk| {
                let mut hash = [0u8; 20];
                hash.copy_from_slice(chunk);
                hash
            })
            .collect();

        // Files — single-file vs multi-file.
        let (files, is_single_file) = if info.contains_key(b"files".as_slice()) {
            // Multi-file torrent: info.files is a list of dicts.
            let files_val = info.get(b"files".as_slice()).unwrap();
            let BencodeValue::List(file_list) = files_val else {
                bail!("'info.files' is not a list");
            };
            let mut offset = 0u64;
            let mut result = Vec::with_capacity(file_list.len());
            for entry in file_list {
                let BencodeValue::Dict(fd) = entry else {
                    bail!("file entry is not a dict");
                };
                let length =
                    dict_get_int(fd, b"length").context("file entry missing 'length'")? as u64;
                let path_list = dict_get_list(fd, b"path").context("file entry missing 'path'")?;
                let path = path_list_to_pathbuf(path_list)?;
                result.push(TorrentFile {
                    path,
                    length,
                    offset,
                });
                offset += length;
            }
            (result, false)
        } else {
            // Single-file torrent.
            let length = dict_get_int(info, b"length")
                .context("single-file torrent missing 'info.length'")?
                as u64;
            let path = PathBuf::from(&name);
            (
                vec![TorrentFile {
                    path,
                    length,
                    offset: 0,
                }],
                true,
            )
        };

        // WebSeed URIs from top-level dict.
        let web_seed_uris = extract_web_seed_uris(&top);

        Ok(Self {
            name,
            files,
            piece_length,
            piece_hashes,
            web_seed_uris,
            is_single_file,
        })
    }

    /// Merge additional WebSeed URIs from external sources (e.g. RPC params).
    ///
    /// Deduplicates and validates URI schemes.
    pub fn merge_web_seed_uris(&mut self, explicit: &[String]) {
        let mut seen: HashSet<String> = self.web_seed_uris.iter().map(|u| u.to_string()).collect();

        for raw in explicit {
            let trimmed = raw.trim();
            if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
                continue;
            }
            match url::Url::parse(trimmed) {
                Ok(url) if is_supported_scheme(url.scheme()) => {
                    self.web_seed_uris.push(url);
                }
                Ok(_) => {
                    warn!(value = %trimmed, "ignoring unsupported WebSeed URI scheme");
                }
                Err(error) => {
                    warn!(value = %trimmed, %error, "ignoring invalid WebSeed URI");
                }
            }
        }
    }

    /// Total number of pieces.
    pub fn num_pieces(&self) -> u32 {
        self.piece_hashes.len() as u32
    }

    /// Total torrent data length across all files.
    pub fn total_length(&self) -> u64 {
        self.files.iter().map(|f| f.length).sum()
    }

    /// Length of a specific piece (the last piece may be shorter).
    pub fn piece_size(&self, piece_index: u32) -> u64 {
        let offset = piece_index as u64 * self.piece_length;
        let remaining = self.total_length().saturating_sub(offset);
        remaining.min(self.piece_length)
    }

    /// Compute the file ranges that a given piece spans.
    ///
    /// A piece may span across multiple files in a multi-file torrent.
    pub fn piece_file_ranges(&self, piece_index: u32) -> Vec<FileRange> {
        let piece_start = piece_index as u64 * self.piece_length;
        let piece_len = self.piece_size(piece_index);
        if piece_len == 0 {
            return Vec::new();
        }
        let piece_end = piece_start + piece_len;

        let mut ranges = Vec::new();
        for (idx, file) in self.files.iter().enumerate() {
            let file_start = file.offset;
            let file_end = file.offset + file.length;

            // Skip files that don't overlap with this piece.
            if file_end <= piece_start || file_start >= piece_end {
                continue;
            }

            let overlap_start = piece_start.max(file_start);
            let overlap_end = piece_end.min(file_end);
            ranges.push(FileRange {
                file_index: idx,
                file_offset: overlap_start - file_start,
                length: overlap_end - overlap_start,
            });
        }
        ranges
    }

    /// Verify a piece's SHA-1 hash against the expected value.
    ///
    /// `data` must be exactly `piece_size(piece_index)` bytes.
    pub fn verify_piece(&self, piece_index: u32, data: &[u8]) -> bool {
        let idx = piece_index as usize;
        if idx >= self.piece_hashes.len() {
            return false;
        }
        let expected = &self.piece_hashes[idx];
        let actual = sha1_smol::Sha1::from(data).digest().bytes();
        actual == *expected
    }
}

// ----- Bencode helpers -----

fn dict_get_bytes<'a>(
    dict: &'a HashMap<Vec<u8>, BencodeValue<Vec<u8>>>,
    key: &[u8],
) -> Option<&'a [u8]> {
    match dict.get(key)? {
        BencodeValue::Bytes(b) => Some(b.as_slice()),
        _ => None,
    }
}

fn dict_get_string(dict: &HashMap<Vec<u8>, BencodeValue<Vec<u8>>>, key: &[u8]) -> Option<String> {
    let bytes = dict_get_bytes(dict, key)?;
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

fn dict_get_int(dict: &HashMap<Vec<u8>, BencodeValue<Vec<u8>>>, key: &[u8]) -> Option<i64> {
    match dict.get(key)? {
        BencodeValue::Integer(n) => Some(*n),
        _ => None,
    }
}

fn dict_get_list<'a>(
    dict: &'a HashMap<Vec<u8>, BencodeValue<Vec<u8>>>,
    key: &[u8],
) -> Option<&'a Vec<BencodeValue<Vec<u8>>>> {
    match dict.get(key)? {
        BencodeValue::List(l) => Some(l),
        _ => None,
    }
}

fn path_list_to_pathbuf(list: &[BencodeValue<Vec<u8>>]) -> Result<PathBuf> {
    let mut path = PathBuf::new();
    for component in list {
        let BencodeValue::Bytes(b) = component else {
            bail!("path component is not bytes");
        };
        let s = std::str::from_utf8(b).context("path component is not utf8")?;
        path.push(s);
    }
    if path.as_os_str().is_empty() {
        bail!("empty path in file entry");
    }
    Ok(path)
}

fn is_supported_scheme(scheme: &str) -> bool {
    matches!(scheme, "http" | "https" | "ftp" | "ftps" | "sftp")
}

/// Extract WebSeed URIs from the top-level torrent dict.
///
/// Handles both BEP-19 `url-list` (string or list) and legacy `httpseeds`.
fn extract_web_seed_uris(top: &HashMap<Vec<u8>, BencodeValue<Vec<u8>>>) -> Vec<url::Url> {
    let mut seen = HashSet::<String>::new();
    let mut out = Vec::new();

    // BEP-19: "url-list" can be either a string or a list of strings.
    if let Some(value) = top.get(b"url-list".as_slice()) {
        match value {
            BencodeValue::Bytes(b) => {
                if let Some(s) = bytes_to_string(b) {
                    try_push_uri(&s, &mut seen, &mut out);
                }
            }
            BencodeValue::List(items) => {
                for item in items {
                    if let BencodeValue::Bytes(b) = item {
                        if let Some(s) = bytes_to_string(b) {
                            try_push_uri(&s, &mut seen, &mut out);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Legacy: "httpseeds" is typically a list of strings.
    if let Some(BencodeValue::List(items)) = top.get(b"httpseeds".as_slice()) {
        for item in items {
            if let BencodeValue::Bytes(b) = item {
                if let Some(s) = bytes_to_string(b) {
                    try_push_uri(&s, &mut seen, &mut out);
                }
            }
        }
    }

    out
}

fn bytes_to_string(b: &[u8]) -> Option<String> {
    std::str::from_utf8(b).ok().map(|s| s.to_string())
}

fn try_push_uri(raw: &str, seen: &mut HashSet<String>, out: &mut Vec<url::Url>) {
    let trimmed = raw.trim();
    if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
        return;
    }
    match url::Url::parse(trimmed) {
        Ok(url) if is_supported_scheme(url.scheme()) => out.push(url),
        Ok(_) => {
            warn!(value = %trimmed, "ignoring unsupported WebSeed URI scheme");
        }
        Err(error) => {
            warn!(value = %trimmed, %error, "ignoring invalid WebSeed URI");
        }
    }
}

// ----- Unit tests -----

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal single-file .torrent bencode for testing.
    fn build_single_file_torrent(data: &[u8], piece_length: usize) -> Vec<u8> {
        use sha1_smol::Sha1;

        let mut pieces = Vec::new();
        for chunk in data.chunks(piece_length) {
            let hash = Sha1::from(chunk).digest().bytes();
            pieces.extend_from_slice(&hash);
        }

        // Manual bencode construction.
        let mut out = Vec::new();
        out.extend_from_slice(b"d");
        // info
        out.extend_from_slice(b"4:info");
        out.extend_from_slice(b"d");
        // info.length
        out.extend_from_slice(format!("6:lengthi{}e", data.len()).as_bytes());
        // info.name
        let name = "test.bin";
        out.extend_from_slice(format!("4:name{}:{}", name.len(), name).as_bytes());
        // info.piece length
        out.extend_from_slice(format!("12:piece lengthi{}e", piece_length).as_bytes());
        // info.pieces
        out.extend_from_slice(format!("6:pieces{}:", pieces.len()).as_bytes());
        out.extend_from_slice(&pieces);
        out.extend_from_slice(b"e"); // end info
        // url-list
        let url_str = "https://mirror.example.com/test.bin";
        out.extend_from_slice(format!("8:url-list{}:{}", url_str.len(), url_str).as_bytes());
        out.extend_from_slice(b"e"); // end top
        out
    }

    #[test]
    fn parse_single_file_torrent() {
        let data: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let piece_length = 16384;
        let torrent = build_single_file_torrent(&data, piece_length);

        let meta = TorrentMeta::from_bytes(&torrent).expect("parse torrent");
        assert_eq!(meta.name, "test.bin");
        assert!(meta.is_single_file);
        assert_eq!(meta.piece_length, piece_length as u64);
        assert_eq!(meta.files.len(), 1);
        assert_eq!(meta.files[0].length, data.len() as u64);
        assert_eq!(meta.files[0].offset, 0);
        assert_eq!(meta.total_length(), data.len() as u64);

        let expected_pieces = data.len().div_ceil(piece_length);
        assert_eq!(meta.num_pieces() as usize, expected_pieces);

        // Verify piece hash.
        let first_piece = &data[..piece_length.min(data.len())];
        assert!(meta.verify_piece(0, first_piece));
        assert!(!meta.verify_piece(0, &[0u8; 16384])); // wrong data
    }

    #[test]
    fn piece_file_ranges_single_file() {
        let data: Vec<u8> = (0..50_000u32).map(|i| (i % 251) as u8).collect();
        let torrent = build_single_file_torrent(&data, 16384);
        let meta = TorrentMeta::from_bytes(&torrent).unwrap();

        // First piece: 0..16384
        let ranges = meta.piece_file_ranges(0);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].file_index, 0);
        assert_eq!(ranges[0].file_offset, 0);
        assert_eq!(ranges[0].length, 16384);

        // Last piece: partial
        let last = meta.num_pieces() - 1;
        let ranges = meta.piece_file_ranges(last);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].length, data.len() as u64 % 16384);
    }

    #[test]
    fn merge_web_seed_uris_deduplicates() {
        let torrent = build_single_file_torrent(&[0u8; 1024], 512);
        let mut meta = TorrentMeta::from_bytes(&torrent).unwrap();
        assert_eq!(meta.web_seed_uris.len(), 1);

        meta.merge_web_seed_uris(&[
            "https://mirror.example.com/test.bin".to_string(), // duplicate
            "ftp://ftp.example.com/test.bin".to_string(),      // new
            "gopher://old.example.com/test.bin".to_string(),   // unsupported
        ]);
        assert_eq!(meta.web_seed_uris.len(), 2);
        assert_eq!(meta.web_seed_uris[1].scheme(), "ftp");
    }

    #[test]
    fn verify_all_pieces() {
        let data: Vec<u8> = (0..32768u32).map(|i| (i % 251) as u8).collect();
        let torrent = build_single_file_torrent(&data, 16384);
        let meta = TorrentMeta::from_bytes(&torrent).unwrap();

        for i in 0..meta.num_pieces() {
            let start = i as usize * 16384;
            let end = (start + 16384).min(data.len());
            assert!(meta.verify_piece(i, &data[start..end]));
        }
    }
}
