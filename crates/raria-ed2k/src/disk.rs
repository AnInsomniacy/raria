//! ED2K verified-byte state, corrupt data handling, and resume snapshots.

use crate::hash::{
    AichHash, ED2K_PART_SIZE, Ed2kHash, Ed2kHashError, ed2k_root_hash_from_part_hashes, md4_digest,
    theoretical_part_hash_count,
};
use crate::transfer::PartRange;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// ED2K disk integrity and resume error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Ed2kDiskStateError {
    /// Resume row version is newer than this binary understands.
    #[error("unsupported ED2K resume snapshot version")]
    UnsupportedVersion,
    /// File size and hashset metadata do not match ED2K rules.
    #[error("invalid ED2K hashset")]
    InvalidHashSet,
    /// ED2K root hash does not match the supplied hashset metadata.
    #[error("ED2K root hash mismatch")]
    RootHashMismatch,
    /// Byte range is empty, inverted, crosses a part boundary, or exceeds file size.
    #[error("invalid ED2K disk range")]
    InvalidRange,
    /// Byte payload length does not match the target range.
    #[error("ED2K disk payload length mismatch")]
    PayloadLengthMismatch,
    /// Requested part has not received every byte needed for verification.
    #[error("incomplete ED2K part")]
    IncompletePart,
    /// Completed part does not match the expected MD4 hash.
    #[error("ED2K part hash mismatch")]
    PartHashMismatch {
        /// ED2K part index that failed verification.
        part_index: u64,
    },
}

/// Durable ED2K source state retained for resume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ed2kResumeSource {
    /// Native endpoint string such as `host:port`.
    pub endpoint: String,
    /// Last seen timestamp in caller-owned monotonic or wall-clock seconds.
    pub last_seen_seconds: u64,
    /// Last observed queue rank.
    pub queue_rank: Option<u16>,
}

/// Versioned native ED2K resume snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ed2kResumeSnapshot {
    /// Snapshot schema version.
    pub row_version: u32,
    /// Total file size in bytes.
    pub file_size: u64,
    /// ED2K root hash.
    pub root_hash: Ed2kHash,
    /// ED2K part hashes when required by protocol boundary rules.
    pub part_hashes: Vec<Ed2kHash>,
    /// Optional AICH root hash.
    pub aich_root: Option<AichHash>,
    /// Ranges verified by disk and hash truth.
    pub verified_ranges: Vec<PartRange>,
    /// Ranges rejected and queued for re-download.
    pub requeue_ranges: Vec<PartRange>,
    /// Resumeable source state.
    pub sources: Vec<Ed2kResumeSource>,
}

impl Ed2kResumeSnapshot {
    /// Current ED2K resume snapshot schema version.
    pub const CURRENT_ROW_VERSION: u32 = 1;
}

/// ED2K verified-byte state for one file.
#[derive(Debug, Clone)]
pub struct Ed2kDiskState {
    file_size: u64,
    root_hash: Ed2kHash,
    part_hashes: Vec<Ed2kHash>,
    aich_root: Option<AichHash>,
    buffers: BTreeMap<u64, PartBuffer>,
    verified_ranges: Vec<PartRange>,
    requeue_ranges: Vec<PartRange>,
    resume_sources: Vec<Ed2kResumeSource>,
}

#[derive(Debug, Clone)]
struct PartBuffer {
    range: PartRange,
    data: Vec<u8>,
    written: Vec<bool>,
}

impl Ed2kDiskState {
    /// Create a new ED2K disk state from file identity metadata.
    pub fn new(
        file_size: u64,
        root_hash: Ed2kHash,
        part_hashes: Vec<Ed2kHash>,
        aich_root: Option<AichHash>,
    ) -> Result<Self, Ed2kDiskStateError> {
        validate_hashset(file_size, root_hash, &part_hashes)?;
        Ok(Self {
            file_size,
            root_hash,
            part_hashes,
            aich_root,
            buffers: BTreeMap::new(),
            verified_ranges: Vec::new(),
            requeue_ranges: Vec::new(),
            resume_sources: Vec::new(),
        })
    }

    /// Restore ED2K disk state from a versioned snapshot.
    pub fn from_resume_snapshot(snapshot: Ed2kResumeSnapshot) -> Result<Self, Ed2kDiskStateError> {
        if snapshot.row_version > Ed2kResumeSnapshot::CURRENT_ROW_VERSION {
            return Err(Ed2kDiskStateError::UnsupportedVersion);
        }
        validate_hashset(
            snapshot.file_size,
            snapshot.root_hash,
            &snapshot.part_hashes,
        )?;
        for range in snapshot
            .verified_ranges
            .iter()
            .chain(snapshot.requeue_ranges.iter())
        {
            validate_range(*range, snapshot.file_size)?;
        }
        Ok(Self {
            file_size: snapshot.file_size,
            root_hash: snapshot.root_hash,
            part_hashes: snapshot.part_hashes,
            aich_root: snapshot.aich_root,
            buffers: BTreeMap::new(),
            verified_ranges: snapshot.verified_ranges,
            requeue_ranges: snapshot.requeue_ranges,
            resume_sources: snapshot.sources,
        })
    }

    /// Build a versioned resume snapshot.
    pub fn to_resume_snapshot(&self, sources: Vec<Ed2kResumeSource>) -> Ed2kResumeSnapshot {
        Ed2kResumeSnapshot {
            row_version: Ed2kResumeSnapshot::CURRENT_ROW_VERSION,
            file_size: self.file_size,
            root_hash: self.root_hash,
            part_hashes: self.part_hashes.clone(),
            aich_root: self.aich_root,
            verified_ranges: self.verified_ranges.clone(),
            requeue_ranges: self.requeue_ranges.clone(),
            sources,
        }
    }

    /// Stage bytes that have been durably written by the disk owner.
    pub fn stage_write(&mut self, range: PartRange, data: &[u8]) -> Result<(), Ed2kDiskStateError> {
        validate_range(range, self.file_size)?;
        if range.begin / ED2K_PART_SIZE != (range.end - 1) / ED2K_PART_SIZE {
            return Err(Ed2kDiskStateError::InvalidRange);
        }
        if u64::try_from(data.len()).map_err(|_| Ed2kDiskStateError::PayloadLengthMismatch)?
            != range.end - range.begin
        {
            return Err(Ed2kDiskStateError::PayloadLengthMismatch);
        }
        let part_index = range.begin / ED2K_PART_SIZE;
        let part_range = self.part_range(part_index)?;
        let buffer = self
            .buffers
            .entry(part_index)
            .or_insert_with(|| PartBuffer {
                range: part_range,
                data: vec![0; part_range_len(part_range)],
                written: vec![false; part_range_len(part_range)],
            });
        let offset = usize::try_from(range.begin - buffer.range.begin)
            .map_err(|_| Ed2kDiskStateError::InvalidRange)?;
        buffer.data[offset..offset + data.len()].copy_from_slice(data);
        for written in &mut buffer.written[offset..offset + data.len()] {
            *written = true;
        }
        Ok(())
    }

    /// Flush one complete part into verified state after MD4 validation.
    pub fn flush_part(&mut self, part_index: u64) -> Result<PartRange, Ed2kDiskStateError> {
        let Some(buffer) = self.buffers.get(&part_index) else {
            return Err(Ed2kDiskStateError::IncompletePart);
        };
        if buffer.written.iter().any(|written| !*written) {
            return Err(Ed2kDiskStateError::IncompletePart);
        }
        let expected = self.expected_part_hash(part_index)?;
        if md4_digest(&buffer.data) != expected {
            let range = buffer.range;
            self.buffers.remove(&part_index);
            push_unique_range(&mut self.requeue_ranges, range);
            return Err(Ed2kDiskStateError::PartHashMismatch { part_index });
        }
        let range = buffer.range;
        self.buffers.remove(&part_index);
        push_unique_range(&mut self.verified_ranges, range);
        self.requeue_ranges.retain(|existing| *existing != range);
        Ok(range)
    }

    /// Return ranges verified by disk and integrity truth.
    pub fn verified_ranges(&self) -> &[PartRange] {
        &self.verified_ranges
    }

    /// Return ranges queued for re-download after integrity failure.
    pub fn requeue_ranges(&self) -> &[PartRange] {
        &self.requeue_ranges
    }

    /// Return the optional AICH root.
    pub fn aich_root(&self) -> Option<&AichHash> {
        self.aich_root.as_ref()
    }

    /// Return restored resume sources.
    pub fn resume_sources(&self) -> &[Ed2kResumeSource] {
        &self.resume_sources
    }

    fn expected_part_hash(&self, part_index: u64) -> Result<Ed2kHash, Ed2kDiskStateError> {
        if theoretical_part_hash_count(self.file_size) == 0 {
            if part_index == 0 {
                return Ok(self.root_hash);
            }
            return Err(Ed2kDiskStateError::InvalidRange);
        }
        self.part_hashes
            .get(part_index as usize)
            .copied()
            .ok_or(Ed2kDiskStateError::InvalidRange)
    }

    fn part_range(&self, part_index: u64) -> Result<PartRange, Ed2kDiskStateError> {
        let begin = part_index
            .checked_mul(ED2K_PART_SIZE)
            .ok_or(Ed2kDiskStateError::InvalidRange)?;
        if begin >= self.file_size {
            return Err(Ed2kDiskStateError::InvalidRange);
        }
        Ok(PartRange {
            begin,
            end: begin.saturating_add(ED2K_PART_SIZE).min(self.file_size),
        })
    }
}

fn validate_hashset(
    file_size: u64,
    root_hash: Ed2kHash,
    part_hashes: &[Ed2kHash],
) -> Result<(), Ed2kDiskStateError> {
    if theoretical_part_hash_count(file_size) == 0 {
        if !part_hashes.is_empty() {
            return Err(Ed2kDiskStateError::InvalidHashSet);
        }
        return Ok(());
    }
    let computed =
        ed2k_root_hash_from_part_hashes(file_size, part_hashes).map_err(|error| match error {
            Ed2kHashError::InvalidPartHashCount { .. } => Ed2kDiskStateError::InvalidHashSet,
            _ => Ed2kDiskStateError::RootHashMismatch,
        })?;
    if computed != root_hash {
        return Err(Ed2kDiskStateError::RootHashMismatch);
    }
    Ok(())
}

fn validate_range(range: PartRange, file_size: u64) -> Result<(), Ed2kDiskStateError> {
    if range.end <= range.begin || range.end > file_size {
        return Err(Ed2kDiskStateError::InvalidRange);
    }
    Ok(())
}

fn part_range_len(range: PartRange) -> usize {
    usize::try_from(range.end - range.begin).expect("ED2K part range fits usize")
}

fn push_unique_range(ranges: &mut Vec<PartRange>, range: PartRange) {
    if !ranges.contains(&range) {
        ranges.push(range);
    }
}
