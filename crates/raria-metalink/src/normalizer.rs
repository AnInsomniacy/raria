// raria-metalink: Normalizer — convert RawMetalink into download job seeds.
//
// This module transforms parsed Metalink data into a normalized format
// suitable for creating RangeJob instances, including URL prioritization,
// checksum extraction, and CLI option merging.

use crate::parser::{
    MetalinkFile, MetalinkHash, MetalinkMetaUrl, MetalinkPieces, MetalinkUrl, RawMetalink,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

/// A normalized seed for creating a RangeJob.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeJobSeed {
    /// Ordered list of download URLs (best first).
    pub uris: Vec<String>,
    /// Output filename.
    pub filename: String,
    /// Expected file size, if known.
    pub expected_size: Option<u64>,
    /// Preferred hash for verification.
    pub checksum: Option<NormalizedChecksum>,
    /// Preferred piece-hash container for chunk verification.
    pub piece_checksum: Option<NormalizedPieceChecksum>,
    /// Metadata sources associated with this file, such as torrent descriptors.
    pub metadata_sources: Vec<NormalizedMetadataSource>,
}

/// A normalized checksum for file verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedChecksum {
    /// Algorithm name (lowercase, e.g., "sha-256").
    pub algo: String,
    /// Hex-encoded hash value (lowercase).
    pub value: String,
}

/// A normalized piece-hash container for chunk verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedPieceChecksum {
    /// Algorithm name (lowercase).
    pub algo: String,
    /// Piece size in bytes.
    pub length: u64,
    /// Piece hashes in file order.
    pub hashes: Vec<String>,
}

/// A normalized metadata source associated with a Metalink file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedMetadataSource {
    /// Metadata URI.
    pub uri: String,
    /// Metadata media type, such as `torrent`.
    pub media_type: String,
    /// Priority (lower is better).
    pub priority: u32,
    /// Optional metadata name.
    pub name: Option<String>,
}

/// Options controlling normalization behavior.
#[derive(Debug, Clone)]
pub struct NormalizeOptions {
    /// Override output directory.
    pub dir: Option<PathBuf>,
    /// Preferred hash algorithms in order of preference.
    pub preferred_hash_algos: Vec<String>,
    /// Preferred mirror locations in order of preference.
    pub preferred_locations: Vec<String>,
    /// Preferred mirror protocol, such as `https`, `http`, or `ftp`.
    pub preferred_protocol: Option<String>,
    /// Keep only the best source for each protocol after preference sorting.
    pub unique_protocols: bool,
}

impl Default for NormalizeOptions {
    fn default() -> Self {
        Self {
            dir: None,
            preferred_hash_algos: vec![
                "sha-256".into(),
                "sha-512".into(),
                "sha-1".into(),
                "md5".into(),
            ],
            preferred_locations: Vec::new(),
            preferred_protocol: None,
            unique_protocols: false,
        }
    }
}

/// Normalize a parsed Metalink into a list of download job seeds.
///
/// Each `MetalinkFile` becomes one `RangeJobSeed`. URLs are sorted
/// by priority (ascending). The best available hash is selected
/// based on the preference list in `NormalizeOptions`.
pub fn normalize(metalink: &RawMetalink, opts: &NormalizeOptions) -> Vec<RangeJobSeed> {
    metalink
        .files
        .iter()
        .map(|file| normalize_file(file, opts))
        .collect()
}

fn normalize_file(file: &MetalinkFile, opts: &NormalizeOptions) -> RangeJobSeed {
    let urls = normalize_urls(&file.urls, opts);
    let uris: Vec<String> = urls.into_iter().map(|u| u.url).collect();

    // Select the best hash.
    let checksum = select_best_hash(&file.hashes, &opts.preferred_hash_algos);
    let piece_checksum = select_best_piece_hashes(&file.pieces, &opts.preferred_hash_algos);

    RangeJobSeed {
        uris,
        filename: file.name.clone(),
        expected_size: file.size,
        checksum,
        piece_checksum,
        metadata_sources: normalize_metaurls(&file.metaurls),
    }
}

fn normalize_urls(urls: &[MetalinkUrl], opts: &NormalizeOptions) -> Vec<MetalinkUrl> {
    let mut urls = urls.to_vec();
    let preferred_protocol = opts
        .preferred_protocol
        .as_ref()
        .map(|protocol| protocol.to_ascii_lowercase())
        .filter(|protocol| !protocol.is_empty() && protocol != "none");
    let preferred_locations = opts
        .preferred_locations
        .iter()
        .map(|location| location.to_ascii_lowercase())
        .collect::<Vec<_>>();

    urls.sort_by_key(|url| {
        (
            protocol_rank(url, preferred_protocol.as_deref()),
            location_rank(url, &preferred_locations),
            url.priority,
        )
    });

    if opts.unique_protocols {
        let mut seen = HashSet::new();
        urls.retain(|url| {
            protocol_of(&url.url)
                .map(|protocol| seen.insert(protocol))
                .unwrap_or(true)
        });
    }

    urls
}

fn protocol_rank(url: &MetalinkUrl, preferred_protocol: Option<&str>) -> u8 {
    let Some(preferred_protocol) = preferred_protocol else {
        return 1;
    };
    protocol_of(&url.url)
        .map(|protocol| (protocol != preferred_protocol) as u8)
        .unwrap_or(1)
}

fn location_rank(url: &MetalinkUrl, preferred_locations: &[String]) -> usize {
    let Some(location) = url.location.as_ref() else {
        return preferred_locations.len();
    };
    let location = location.to_ascii_lowercase();
    preferred_locations
        .iter()
        .position(|preferred| preferred == &location)
        .unwrap_or(preferred_locations.len())
}

fn protocol_of(uri: &str) -> Option<String> {
    url::Url::parse(uri)
        .ok()
        .map(|url| url.scheme().to_ascii_lowercase())
}

fn normalize_metaurls(metaurls: &[MetalinkMetaUrl]) -> Vec<NormalizedMetadataSource> {
    let mut metaurls = metaurls.to_vec();
    metaurls.sort_by_key(|metaurl| metaurl.priority);
    metaurls
        .into_iter()
        .map(|metaurl| NormalizedMetadataSource {
            uri: metaurl.url,
            media_type: metaurl.media_type.to_lowercase(),
            priority: metaurl.priority,
            name: metaurl.name,
        })
        .collect()
}

fn select_best_hash(hashes: &[MetalinkHash], preferred: &[String]) -> Option<NormalizedChecksum> {
    for algo in preferred {
        if let Some(hash) = hashes.iter().find(|h| h.algo.eq_ignore_ascii_case(algo)) {
            return Some(NormalizedChecksum {
                algo: hash.algo.to_lowercase(),
                value: hash.value.to_lowercase(),
            });
        }
    }
    // Fallback: return the first hash if any.
    hashes.first().map(|h| NormalizedChecksum {
        algo: h.algo.to_lowercase(),
        value: h.value.to_lowercase(),
    })
}

fn select_best_piece_hashes(
    pieces: &[MetalinkPieces],
    preferred: &[String],
) -> Option<NormalizedPieceChecksum> {
    for algo in preferred {
        if let Some(piece_set) = pieces
            .iter()
            .find(|pieces| pieces.algo.eq_ignore_ascii_case(algo))
        {
            return Some(NormalizedPieceChecksum {
                algo: piece_set.algo.to_lowercase(),
                length: piece_set.length,
                hashes: piece_set
                    .hashes
                    .iter()
                    .map(|hash| hash.to_lowercase())
                    .collect(),
            });
        }
    }

    pieces.first().map(|piece_set| NormalizedPieceChecksum {
        algo: piece_set.algo.to_lowercase(),
        length: piece_set.length,
        hashes: piece_set
            .hashes
            .iter()
            .map(|hash| hash.to_lowercase())
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{
        MetalinkFile, MetalinkHash, MetalinkMetaUrl, MetalinkPieces, MetalinkUrl, MetalinkVersion,
        RawMetalink,
    };

    fn sample_metalink() -> RawMetalink {
        RawMetalink {
            version: MetalinkVersion::V4,
            files: vec![MetalinkFile {
                name: "test.zip".into(),
                size: Some(5000),
                hashes: vec![
                    MetalinkHash {
                        algo: "md5".into(),
                        value: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    },
                    MetalinkHash {
                        algo: "sha-256".into(),
                        value: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                            .into(),
                    },
                ],
                pieces: vec![],
                metaurls: vec![],
                urls: vec![
                    MetalinkUrl {
                        url: "https://slow.example.com/test.zip".into(),
                        priority: 10,
                        location: Some("us".into()),
                    },
                    MetalinkUrl {
                        url: "https://fast.example.com/test.zip".into(),
                        priority: 1,
                        location: Some("de".into()),
                    },
                    MetalinkUrl {
                        url: "ftp://ftp.example.com/test.zip".into(),
                        priority: 5,
                        location: None,
                    },
                ],
            }],
        }
    }

    #[test]
    fn normalize_sorts_urls_by_priority() {
        let ml = sample_metalink();
        let seeds = normalize(&ml, &NormalizeOptions::default());

        assert_eq!(seeds.len(), 1);
        let seed = &seeds[0];
        assert_eq!(seed.uris.len(), 3);
        // Priority 1 first, then 5, then 10.
        assert!(seed.uris[0].contains("fast"));
        assert!(seed.uris[1].contains("ftp"));
        assert!(seed.uris[2].contains("slow"));
    }

    #[test]
    fn normalize_prefers_configured_locations_before_plain_priority() {
        let ml = sample_metalink();
        let opts = NormalizeOptions {
            preferred_locations: vec!["us".into()],
            ..Default::default()
        };

        let seeds = normalize(&ml, &opts);

        assert_eq!(
            seeds[0].uris,
            vec![
                "https://slow.example.com/test.zip",
                "https://fast.example.com/test.zip",
                "ftp://ftp.example.com/test.zip",
            ]
        );
    }

    #[test]
    fn normalize_prefers_configured_protocol_before_plain_priority() {
        let ml = sample_metalink();
        let opts = NormalizeOptions {
            preferred_protocol: Some("ftp".into()),
            ..Default::default()
        };

        let seeds = normalize(&ml, &opts);

        assert_eq!(seeds[0].uris[0], "ftp://ftp.example.com/test.zip");
    }

    #[test]
    fn normalize_can_keep_one_source_per_protocol_after_preference_sorting() {
        let ml = RawMetalink {
            version: MetalinkVersion::V4,
            files: vec![MetalinkFile {
                name: "f.bin".into(),
                size: None,
                hashes: vec![],
                pieces: vec![],
                metaurls: vec![],
                urls: vec![
                    MetalinkUrl {
                        url: "https://us.example/f.bin".into(),
                        priority: 10,
                        location: Some("us".into()),
                    },
                    MetalinkUrl {
                        url: "https://de.example/f.bin".into(),
                        priority: 1,
                        location: Some("de".into()),
                    },
                    MetalinkUrl {
                        url: "ftp://ftp.example/f.bin".into(),
                        priority: 5,
                        location: None,
                    },
                ],
            }],
        };
        let opts = NormalizeOptions {
            preferred_locations: vec!["us".into()],
            unique_protocols: true,
            ..Default::default()
        };

        let seeds = normalize(&ml, &opts);

        assert_eq!(
            seeds[0].uris,
            vec!["https://us.example/f.bin", "ftp://ftp.example/f.bin"]
        );
    }

    #[test]
    fn normalize_selects_sha256_over_md5() {
        let ml = sample_metalink();
        let seeds = normalize(&ml, &NormalizeOptions::default());

        let checksum = seeds[0].checksum.as_ref().expect("should have checksum");
        assert_eq!(checksum.algo, "sha-256");
    }

    #[test]
    fn normalize_preserves_filename_and_size() {
        let ml = sample_metalink();
        let seeds = normalize(&ml, &NormalizeOptions::default());

        assert_eq!(seeds[0].filename, "test.zip");
        assert_eq!(seeds[0].expected_size, Some(5000));
    }

    #[test]
    fn normalize_fallback_hash_when_no_preferred() {
        let ml = RawMetalink {
            version: MetalinkVersion::V4,
            files: vec![MetalinkFile {
                name: "f.bin".into(),
                size: None,
                hashes: vec![MetalinkHash {
                    algo: "whirlpool".into(),
                    value: "AABBCC".into(),
                }],
                pieces: vec![],
                metaurls: vec![],
                urls: vec![MetalinkUrl {
                    url: "https://a.com/f".into(),
                    priority: 1,
                    location: None,
                }],
            }],
        };

        let seeds = normalize(&ml, &NormalizeOptions::default());
        let checksum = seeds[0].checksum.as_ref().unwrap();
        assert_eq!(checksum.algo, "whirlpool");
        assert_eq!(checksum.value, "aabbcc"); // lowercased
    }

    #[test]
    fn normalize_no_hashes_returns_none() {
        let ml = RawMetalink {
            version: MetalinkVersion::V4,
            files: vec![MetalinkFile {
                name: "f.bin".into(),
                size: None,
                hashes: vec![],
                pieces: vec![],
                metaurls: vec![],
                urls: vec![MetalinkUrl {
                    url: "https://a.com/f".into(),
                    priority: 1,
                    location: None,
                }],
            }],
        };

        let seeds = normalize(&ml, &NormalizeOptions::default());
        assert!(seeds[0].checksum.is_none());
    }

    #[test]
    fn normalize_multifile() {
        let ml = RawMetalink {
            version: MetalinkVersion::V4,
            files: vec![
                MetalinkFile {
                    name: "a.bin".into(),
                    size: Some(100),
                    hashes: vec![],
                    pieces: vec![],
                    metaurls: vec![],
                    urls: vec![MetalinkUrl {
                        url: "https://a.com/a".into(),
                        priority: 1,
                        location: None,
                    }],
                },
                MetalinkFile {
                    name: "b.bin".into(),
                    size: Some(200),
                    hashes: vec![],
                    pieces: vec![],
                    metaurls: vec![],
                    urls: vec![MetalinkUrl {
                        url: "https://a.com/b".into(),
                        priority: 1,
                        location: None,
                    }],
                },
            ],
        };

        let seeds = normalize(&ml, &NormalizeOptions::default());
        assert_eq!(seeds.len(), 2);
        assert_eq!(seeds[0].filename, "a.bin");
        assert_eq!(seeds[1].filename, "b.bin");
    }

    #[test]
    fn seed_serde_roundtrips() {
        let ml = sample_metalink();
        let seeds = normalize(&ml, &NormalizeOptions::default());

        let json = serde_json::to_string(&seeds[0]).unwrap();
        let recovered: RangeJobSeed = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.filename, "test.zip");
        assert_eq!(recovered.uris.len(), 3);
    }

    #[test]
    fn normalize_keeps_piece_checksum_when_available() {
        let ml = RawMetalink {
            version: MetalinkVersion::V4,
            files: vec![MetalinkFile {
                name: "piece.bin".into(),
                size: Some(2048),
                hashes: vec![],
                pieces: vec![MetalinkPieces {
                    algo: "sha-256".into(),
                    length: 1024,
                    hashes: vec!["AA".into(), "BB".into()],
                }],
                metaurls: vec![],
                urls: vec![MetalinkUrl {
                    url: "https://a.com/piece.bin".into(),
                    priority: 1,
                    location: None,
                }],
            }],
        };

        let seeds = normalize(&ml, &NormalizeOptions::default());
        let piece_checksum = seeds[0].piece_checksum.as_ref().expect("piece checksum");
        assert_eq!(piece_checksum.algo, "sha-256");
        assert_eq!(piece_checksum.length, 1024);
        assert_eq!(piece_checksum.hashes, vec!["aa", "bb"]);
    }

    #[test]
    fn normalize_keeps_torrent_metaurl_as_metadata_source() {
        let ml = RawMetalink {
            version: MetalinkVersion::V4,
            files: vec![MetalinkFile {
                name: "example.iso".into(),
                size: Some(1048576),
                hashes: vec![],
                pieces: vec![],
                urls: vec![MetalinkUrl {
                    url: "https://mirror.example.com/example.iso".into(),
                    priority: 1,
                    location: None,
                }],
                metaurls: vec![MetalinkMetaUrl {
                    url: "https://meta.example.com/example.iso.torrent".into(),
                    media_type: "torrent".into(),
                    priority: 2,
                    name: Some("example.iso.torrent".into()),
                }],
            }],
        };

        let seeds = normalize(&ml, &NormalizeOptions::default());

        assert_eq!(
            seeds[0].uris,
            vec!["https://mirror.example.com/example.iso"]
        );
        assert_eq!(seeds[0].metadata_sources.len(), 1);
        assert_eq!(
            seeds[0].metadata_sources[0].uri,
            "https://meta.example.com/example.iso.torrent"
        );
        assert_eq!(seeds[0].metadata_sources[0].media_type, "torrent");
    }
}
