//! Native ED2K shared-file, upload queue, and credit ownership.

use crate::disk::Ed2kDiskState;
use crate::hash::{AichHash, Ed2kHash};
use crate::kad::{KadSearchError, build_kad_publish_source_request};
use crate::peer::PeerEndpoint;
use crate::tag::{Tag, TagName, TagValue};
use crate::transfer::PartRange;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

/// Origin of a file exposed through native ED2K sharing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedFileOrigin {
    /// File completed through raria download ownership.
    CompletedDownload,
    /// File imported through explicit native raria sharing metadata.
    ImportedFile,
}

/// Native input used to add or replace a shared ED2K file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedFileInput {
    /// Local filesystem path.
    pub path: PathBuf,
    /// Public file name.
    pub name: String,
    /// File size in bytes.
    pub size: u64,
    /// ED2K root hash.
    pub root_hash: Ed2kHash,
    /// ED2K part hashset.
    pub part_hashes: Vec<Ed2kHash>,
    /// Optional AICH root.
    pub aich_root: Option<AichHash>,
    /// Native sharing origin.
    pub origin: SharedFileOrigin,
    /// Byte ranges verified by local disk and hash truth.
    pub verified_ranges: Vec<PartRange>,
    /// Caller-owned timestamp in seconds.
    pub now_seconds: u64,
}

/// Native ED2K shared file metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedFile {
    /// Local filesystem path.
    pub path: PathBuf,
    /// Public file name.
    pub name: String,
    /// File size in bytes.
    pub size: u64,
    /// ED2K root hash.
    pub root_hash: Ed2kHash,
    /// ED2K part hashset.
    pub part_hashes: Vec<Ed2kHash>,
    /// Optional AICH root.
    pub aich_root: Option<AichHash>,
    /// Native sharing origin.
    pub origin: SharedFileOrigin,
    /// Byte ranges verified by local disk and hash truth.
    pub verified_ranges: Vec<PartRange>,
    /// First retained timestamp in caller-owned seconds.
    pub created_seconds: u64,
    /// Last updated timestamp in caller-owned seconds.
    pub updated_seconds: u64,
}

/// Configuration needed to produce source publish records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedPublishConfig {
    /// Whether native sharing is enabled.
    pub sharing_enabled: bool,
    /// Local source endpoint advertised to peers.
    pub source_endpoint: PeerEndpoint,
    /// Local ED2K user hash advertised to Kad.
    pub source_id: Ed2kHash,
}

/// Native publish metadata for server and Kad source announcements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedPublishRecord {
    /// Shared file hash.
    pub file_hash: Ed2kHash,
    /// Shared file name.
    pub name: String,
    /// Shared file size.
    pub size: u64,
    /// ED2K part hashset.
    pub part_hashes: Vec<Ed2kHash>,
    /// Optional AICH root.
    pub aich_root: Option<AichHash>,
    /// Server-side file tags used by the server owner.
    pub server_tags: Vec<Tag>,
    /// Optional Kad source-publish payload.
    pub kad_payload: Option<Vec<u8>>,
}

/// Native shared-file store.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SharedFileStore {
    files: Vec<SharedFile>,
}

/// Shared file validation, publishing, and read error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SharingError {
    /// Public shared name is empty.
    #[error("invalid ED2K shared file name")]
    InvalidName,
    /// Shared file size is zero.
    #[error("invalid ED2K shared file size")]
    InvalidSize,
    /// ED2K hashset metadata does not match file size and root hash.
    #[error("invalid ED2K shared hashset")]
    InvalidHashSet,
    /// Verified range metadata is empty or invalid.
    #[error("invalid ED2K shared verified range")]
    InvalidRange,
    /// Requested file is not retained by the shared store.
    #[error("ED2K shared file is missing")]
    MissingFile,
    /// Requested byte range is not verified for upload serving.
    #[error("ED2K shared range is not verified")]
    UnverifiedRange,
    /// Shared file cannot be read from disk.
    #[error("ED2K shared file read failed")]
    ReadFailed,
    /// Shared file read returned fewer bytes than requested.
    #[error("ED2K shared file short read")]
    ShortRead,
    /// Kad publish payload could not be built.
    #[error("ED2K Kad publish failed")]
    KadPublishFailed,
}

impl SharedFileStore {
    /// Add or replace a shared file by ED2K root hash.
    pub fn add_or_replace(&mut self, input: SharedFileInput) -> Result<&SharedFile, SharingError> {
        let file = shared_file_from_input(input)?;
        if let Some(index) = self
            .files
            .iter()
            .position(|existing| existing.root_hash == file.root_hash)
        {
            let created_seconds = self.files[index].created_seconds;
            self.files[index] = SharedFile {
                created_seconds,
                ..file
            };
            return Ok(&self.files[index]);
        }
        self.files.push(file);
        Ok(self.files.last().expect("shared file was inserted"))
    }

    /// Return a shared file by ED2K root hash.
    pub fn find_by_hash(&self, root_hash: &Ed2kHash) -> Option<&SharedFile> {
        self.files.iter().find(|file| &file.root_hash == root_hash)
    }

    /// Return all shared files.
    pub fn list(&self) -> &[SharedFile] {
        &self.files
    }

    /// Build native publish records for all shared files.
    pub fn publish_records(&self, config: SharedPublishConfig) -> Vec<SharedPublishRecord> {
        if !config.sharing_enabled {
            return Vec::new();
        }
        self.files
            .iter()
            .filter_map(|file| publish_record(file, config).ok())
            .collect()
    }

    /// Read a verified byte range from a shared file.
    pub fn read_verified_range(
        &self,
        root_hash: &Ed2kHash,
        range: PartRange,
    ) -> Result<Vec<u8>, SharingError> {
        let file = self
            .find_by_hash(root_hash)
            .ok_or(SharingError::MissingFile)?;
        validate_range(range, file.size)?;
        if !range_is_verified(&file.verified_ranges, range) {
            return Err(SharingError::UnverifiedRange);
        }
        read_range(file, range)
    }
}

fn shared_file_from_input(input: SharedFileInput) -> Result<SharedFile, SharingError> {
    if input.name.trim().is_empty() {
        return Err(SharingError::InvalidName);
    }
    if input.size == 0 {
        return Err(SharingError::InvalidSize);
    }
    Ed2kDiskState::new(
        input.size,
        input.root_hash,
        input.part_hashes.clone(),
        input.aich_root,
    )
    .map_err(|_| SharingError::InvalidHashSet)?;
    if input.verified_ranges.is_empty() {
        return Err(SharingError::InvalidRange);
    }
    for range in &input.verified_ranges {
        validate_range(*range, input.size)?;
    }
    Ok(SharedFile {
        path: input.path,
        name: input.name.trim().to_string(),
        size: input.size,
        root_hash: input.root_hash,
        part_hashes: input.part_hashes,
        aich_root: input.aich_root,
        origin: input.origin,
        verified_ranges: input.verified_ranges,
        created_seconds: input.now_seconds,
        updated_seconds: input.now_seconds,
    })
}

fn publish_record(
    file: &SharedFile,
    config: SharedPublishConfig,
) -> Result<SharedPublishRecord, SharingError> {
    let kad_payload = build_kad_publish_source_request(
        file.root_hash,
        config.source_endpoint,
        config.source_id,
        file.size,
        config.sharing_enabled,
    )
    .map_err(map_kad_publish_error)?;
    Ok(SharedPublishRecord {
        file_hash: file.root_hash,
        name: file.name.clone(),
        size: file.size,
        part_hashes: file.part_hashes.clone(),
        aich_root: file.aich_root,
        server_tags: server_tags(file),
        kad_payload,
    })
}

fn server_tags(file: &SharedFile) -> Vec<Tag> {
    vec![
        Tag::new(TagName::Id(0x01), TagValue::String(file.name.clone())),
        Tag::new(TagName::Id(0x02), TagValue::UInt64(file.size)),
    ]
}

fn map_kad_publish_error(_error: KadSearchError) -> SharingError {
    SharingError::KadPublishFailed
}

fn validate_range(range: PartRange, file_size: u64) -> Result<(), SharingError> {
    if range.end <= range.begin || range.end > file_size {
        return Err(SharingError::InvalidRange);
    }
    Ok(())
}

fn range_is_verified(verified_ranges: &[PartRange], requested: PartRange) -> bool {
    verified_ranges
        .iter()
        .any(|verified| verified.begin <= requested.begin && verified.end >= requested.end)
}

fn read_range(file: &SharedFile, range: PartRange) -> Result<Vec<u8>, SharingError> {
    let mut handle = File::open(&file.path).map_err(|_| SharingError::ReadFailed)?;
    handle
        .seek(SeekFrom::Start(range.begin))
        .map_err(|_| SharingError::ReadFailed)?;
    let len = usize::try_from(range.end - range.begin).map_err(|_| SharingError::InvalidRange)?;
    let mut data = vec![0; len];
    handle
        .read_exact(&mut data)
        .map_err(|_| SharingError::ShortRead)?;
    Ok(data)
}
