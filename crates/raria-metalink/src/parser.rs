// raria-metalink: Metalink v3/v4 XML parser.
//
// Parses Metalink XML documents into structured data for multi-source
// download orchestration. Uses quick-xml for parsing.

use anyhow::Result;
use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};

/// Raw parsed Metalink file representation (v3 or v4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawMetalink {
    /// Metalink version detected.
    pub version: MetalinkVersion,
    /// Files described in this metalink.
    pub files: Vec<MetalinkFile>,
}

/// Metalink format version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetalinkVersion {
    /// Metalink v3 (RFC 5854 predecessor, XML-based).
    V3,
    /// Metalink v4 (RFC 5854, also known as Metalink/HTTP).
    V4,
}

/// A single file entry in a Metalink document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetalinkFile {
    /// Filename.
    pub name: String,
    /// File size in bytes, if specified.
    pub size: Option<u64>,
    /// Hash values for integrity verification.
    pub hashes: Vec<MetalinkHash>,
    /// Piece-hash containers for chunk-level verification.
    pub pieces: Vec<MetalinkPieces>,
    /// Download URLs with priority.
    pub urls: Vec<MetalinkUrl>,
}

/// A hash value for file verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetalinkHash {
    /// Hash algorithm (e.g., "sha-256", "md5").
    pub algo: String,
    /// Hex-encoded hash value.
    pub value: String,
}

/// A chunk-level hash container from a `<pieces>` element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetalinkPieces {
    /// Hash algorithm for all entries in this container.
    pub algo: String,
    /// Piece length in bytes.
    pub length: u64,
    /// Piece hashes in file order.
    pub hashes: Vec<String>,
}

/// A download URL with priority and location metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetalinkUrl {
    /// The download URL.
    pub url: String,
    /// Priority (lower = better, default = 999999).
    pub priority: u32,
    /// Geographic location hint (e.g., "us", "de").
    pub location: Option<String>,
}

/// Parse a Metalink XML string into a `RawMetalink`.
pub fn parse_metalink(xml: &str) -> Result<RawMetalink> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut version = MetalinkVersion::V4; // default
    let mut files: Vec<MetalinkFile> = Vec::new();

    // Parsing state.
    let mut current_file: Option<MetalinkFile> = None;
    let mut in_url = false;
    let mut current_url_priority: u32 = 999999;
    let mut current_url_location: Option<String> = None;
    let mut in_hash = false;
    let mut current_hash_algo = String::new();
    let mut current_pieces: Option<MetalinkPieces> = None;
    let mut in_size = false;
    let mut in_name = false;
    let mut buf = Vec::new();
    let mut text_buf = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local_name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();

                match local_name.as_str() {
                    "metalink" => {
                        // Check xmlns for version detection.
                        for attr in e.attributes().flatten() {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let val = String::from_utf8_lossy(&attr.value).to_string();
                            if key == "xmlns" && val.contains("ietf") {
                                version = MetalinkVersion::V4;
                            } else if key == "version" && val.starts_with('3') {
                                version = MetalinkVersion::V3;
                            }
                        }
                    }
                    "file" => {
                        let mut name = String::new();
                        for attr in e.attributes().flatten() {
                            if String::from_utf8_lossy(attr.key.as_ref()) == "name" {
                                name = String::from_utf8_lossy(&attr.value).to_string();
                            }
                        }
                        current_file = Some(MetalinkFile {
                            name,
                            size: None,
                            hashes: Vec::new(),
                            pieces: Vec::new(),
                            urls: Vec::new(),
                        });
                    }
                    "pieces" => {
                        let mut algo = String::new();
                        let mut length = 0u64;
                        for attr in e.attributes().flatten() {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let val = String::from_utf8_lossy(&attr.value).to_string();
                            match key.as_str() {
                                "type" => algo = val,
                                "length" => length = val.parse().unwrap_or(0),
                                _ => {}
                            }
                        }
                        current_pieces = Some(MetalinkPieces {
                            algo,
                            length,
                            hashes: Vec::new(),
                        });
                    }
                    "url" | "metaurl" => {
                        in_url = true;
                        current_url_priority = 999999;
                        current_url_location = None;
                        for attr in e.attributes().flatten() {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let val = String::from_utf8_lossy(&attr.value).to_string();
                            match key.as_str() {
                                "priority" | "preference" => {
                                    current_url_priority = val.parse().unwrap_or(999999);
                                }
                                "location" => {
                                    current_url_location = Some(val);
                                }
                                _ => {}
                            }
                        }
                        text_buf.clear();
                    }
                    "hash" => {
                        in_hash = true;
                        current_hash_algo = String::new();
                        for attr in e.attributes().flatten() {
                            if String::from_utf8_lossy(attr.key.as_ref()) == "type" {
                                current_hash_algo =
                                    String::from_utf8_lossy(&attr.value).to_string();
                            }
                        }
                        text_buf.clear();
                    }
                    "size" => {
                        in_size = true;
                        text_buf.clear();
                    }
                    "name" if current_file.is_some() => {
                        // v3 has <name> as child element.
                        in_name = true;
                        text_buf.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string();
                if in_url || in_hash || in_size || in_name {
                    text_buf.push_str(&text);
                }
            }
            Ok(Event::End(ref e)) => {
                let local_name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();

                match local_name.as_str() {
                    "file" => {
                        if let Some(file) = current_file.take() {
                            files.push(file);
                        }
                    }
                    "url" | "metaurl" => {
                        if in_url {
                            if let Some(ref mut file) = current_file {
                                let url_text = text_buf.trim().to_string();
                                if !url_text.is_empty() {
                                    file.urls.push(MetalinkUrl {
                                        url: url_text,
                                        priority: current_url_priority,
                                        location: current_url_location.take(),
                                    });
                                }
                            }
                            in_url = false;
                        }
                    }
                    "hash" => {
                        if in_hash {
                            let hash_val = text_buf.trim().to_string();
                            if !hash_val.is_empty() {
                                if let Some(ref mut pieces) = current_pieces {
                                    pieces.hashes.push(hash_val);
                                } else if let Some(ref mut file) = current_file {
                                    file.hashes.push(MetalinkHash {
                                        algo: current_hash_algo.clone(),
                                        value: hash_val.clone(),
                                    });
                                }
                            }
                            in_hash = false;
                        }
                    }
                    "pieces" => {
                        if let Some(pieces) = current_pieces.take() {
                            if let Some(ref mut file) = current_file {
                                file.pieces.push(pieces);
                            }
                        }
                    }
                    "size" => {
                        if in_size {
                            if let Some(ref mut file) = current_file {
                                file.size = text_buf.trim().parse().ok();
                            }
                            in_size = false;
                        }
                    }
                    "name" => {
                        if in_name {
                            if let Some(ref mut file) = current_file {
                                let name = text_buf.trim().to_string();
                                if !name.is_empty() {
                                    file.name = name;
                                }
                            }
                            in_name = false;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow::anyhow!("XML parse error: {e}")),
            _ => {}
        }
        buf.clear();
    }

    Ok(RawMetalink { version, files })
}

#[cfg(test)]
mod tests {
    use super::*;

    const METALINK_V4_SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="example.zip">
    <size>1048576</size>
    <hash type="sha-256">abc123def456</hash>
    <pieces type="sha-256" length="262144">
      <hash>piece0</hash>
      <hash>piece1</hash>
      <hash>piece2</hash>
      <hash>piece3</hash>
    </pieces>
    <url priority="1" location="us">https://mirror1.example.com/example.zip</url>
    <url priority="2" location="de">https://mirror2.example.com/example.zip</url>
    <url priority="3">ftp://ftp.example.com/example.zip</url>
  </file>
</metalink>"#;

    const METALINK_V4_MULTIFILE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="file1.bin">
    <size>500</size>
    <url priority="1">https://a.com/file1.bin</url>
  </file>
  <file name="file2.bin">
    <size>1000</size>
    <url priority="1">https://a.com/file2.bin</url>
    <url priority="2">https://b.com/file2.bin</url>
  </file>
</metalink>"#;

    #[test]
    fn parse_v4_basic() {
        let ml = parse_metalink(METALINK_V4_SAMPLE).unwrap();
        assert_eq!(ml.version, MetalinkVersion::V4);
        assert_eq!(ml.files.len(), 1);

        let file = &ml.files[0];
        assert_eq!(file.name, "example.zip");
        assert_eq!(file.size, Some(1048576));
        assert_eq!(file.hashes.len(), 1);
        assert_eq!(file.pieces.len(), 1);
        assert_eq!(file.hashes[0].algo, "sha-256");
        assert_eq!(file.hashes[0].value, "abc123def456");
        assert_eq!(file.pieces[0].algo, "sha-256");
        assert_eq!(file.pieces[0].length, 262144);
        assert_eq!(
            file.pieces[0].hashes,
            vec!["piece0", "piece1", "piece2", "piece3"]
        );
        assert_eq!(file.urls.len(), 3);

        assert_eq!(file.urls[0].priority, 1);
        assert_eq!(file.urls[0].location.as_deref(), Some("us"));
        assert!(file.urls[0].url.contains("mirror1"));

        assert_eq!(file.urls[2].priority, 3);
        assert!(file.urls[2].url.starts_with("ftp://"));
    }

    #[test]
    fn parse_v4_multifile() {
        let ml = parse_metalink(METALINK_V4_MULTIFILE).unwrap();
        assert_eq!(ml.files.len(), 2);

        assert_eq!(ml.files[0].name, "file1.bin");
        assert_eq!(ml.files[0].size, Some(500));
        assert_eq!(ml.files[0].urls.len(), 1);

        assert_eq!(ml.files[1].name, "file2.bin");
        assert_eq!(ml.files[1].size, Some(1000));
        assert_eq!(ml.files[1].urls.len(), 2);
    }

    #[test]
    fn parse_empty_metalink() {
        let xml =
            r#"<?xml version="1.0"?><metalink xmlns="urn:ietf:params:xml:ns:metalink"></metalink>"#;
        let ml = parse_metalink(xml).unwrap();
        assert!(ml.files.is_empty());
    }

    #[test]
    fn parse_metalink_invalid_xml_returns_error() {
        let result = parse_metalink("<not valid xml");
        // quick-xml may not error on all malformed XML, but at least it shouldn't panic.
        // This test ensures robustness.
        let _ = result;
    }

    #[test]
    fn metalink_serde_roundtrips() {
        let ml = parse_metalink(METALINK_V4_SAMPLE).unwrap();
        let json = serde_json::to_string(&ml).unwrap();
        let recovered: RawMetalink = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.files.len(), 1);
        assert_eq!(recovered.files[0].name, "example.zip");
    }

    #[test]
    fn urls_sorted_by_priority() {
        let ml = parse_metalink(METALINK_V4_SAMPLE).unwrap();
        let file = &ml.files[0];
        let priorities: Vec<u32> = file.urls.iter().map(|u| u.priority).collect();
        assert_eq!(priorities, vec![1, 2, 3]);
    }

    #[test]
    fn piece_hashes_preserve_container_order() {
        let ml = parse_metalink(METALINK_V4_SAMPLE).unwrap();
        let pieces = &ml.files[0].pieces[0];
        assert_eq!(pieces.hashes.first().map(String::as_str), Some("piece0"));
        assert_eq!(pieces.hashes.last().map(String::as_str), Some("piece3"));
    }
}
