use anyhow::{Context, Result};
use raria_core::config::MetalinkMetadataSource;
use raria_core::engine::Engine;
use raria_core::job::{Gid, PieceChecksum};
use raria_metalink::normalizer::{
    NormalizeOptions, NormalizedMetadataSource, RangeJobSeed, normalize,
};
use raria_metalink::parser::{RawMetalink, parse_metalink};

pub(crate) fn parse_metalink_xml(xml: &str) -> Result<RawMetalink> {
    parse_metalink(xml).context("failed to parse metalink")
}

pub(crate) fn normalize_metalink_for_engine(
    engine: &Engine,
    metalink: &RawMetalink,
) -> Vec<RangeJobSeed> {
    normalize(
        metalink,
        &NormalizeOptions {
            preferred_locations: engine.config.metalink_preferred_locations.clone(),
            preferred_protocol: engine.config.metalink_preferred_protocol.clone(),
            unique_protocols: engine.config.metalink_unique_protocols,
            ..NormalizeOptions::default()
        },
    )
}

pub(crate) fn torrent_metadata_source(seed: &RangeJobSeed) -> Option<&NormalizedMetadataSource> {
    seed.metadata_sources.iter().find(|source| {
        source.media_type.eq_ignore_ascii_case("torrent")
            || source
                .uri
                .split('?')
                .next()
                .is_some_and(|path| path.ends_with(".torrent"))
    })
}

pub(crate) fn apply_metalink_seed_metadata(
    engine: &Engine,
    gid: Gid,
    seed: &RangeJobSeed,
) -> Result<()> {
    engine
        .registry
        .update(gid, |job| {
            if let Some(checksum) = seed.checksum.as_ref() {
                job.options.checksum = Some(format!("{}={}", checksum.algo, checksum.value));
            }
            job.total_size = seed.expected_size;
            job.piece_checksum = seed
                .piece_checksum
                .as_ref()
                .map(|piece_checksum| PieceChecksum {
                    algo: piece_checksum.algo.clone(),
                    length: piece_checksum.length,
                    hashes: piece_checksum.hashes.clone(),
                });
            job.options.metalink_metadata_sources = seed
                .metadata_sources
                .iter()
                .map(|source| MetalinkMetadataSource {
                    uri: source.uri.clone(),
                    media_type: source.media_type.clone(),
                    priority: source.priority,
                    name: source.name.clone(),
                })
                .collect();
        })
        .context("native Metalink task not found")?;
    Ok(())
}
