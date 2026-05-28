//! Native ED2K shared-file, upload queue, and credit ownership.

use crate::disk::Ed2kDiskState;
use crate::hash::{AichHash, Ed2kHash};
use crate::kad::{KadSearchError, build_kad_publish_source_request};
use crate::opcode::PeerOpcode;
use crate::packet::{PacketFrame, Protocol};
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

/// Native upload-slot request from a remote ED2K peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedUploadRequest {
    /// Remote peer endpoint.
    pub endpoint: PeerEndpoint,
    /// Optional remote user hash.
    pub user_hash: Option<Ed2kHash>,
    /// Requested shared file hash.
    pub file_hash: Ed2kHash,
    /// Caller-owned timestamp in seconds.
    pub now_seconds: u64,
}

/// Upload-queue decision for a remote peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadDecision {
    /// Peer may upload immediately.
    Accepted,
    /// Peer is waiting at a one-based queue rank.
    Queued {
        /// One-based queue rank.
        rank: u16,
    },
    /// Requested file is not shared.
    FileNotShared,
    /// Another queued or uploading endpoint already owns this user hash.
    DuplicatePeer,
    /// Waiting queue is full.
    QueueFull,
}

/// UDP reask response selected from native upload state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdpReaskResponse {
    /// Peer is known and receives its current queue rank. Uploading peers use rank zero.
    Ack {
        /// Current queue rank, or zero for an active upload slot.
        rank: u16,
    },
    /// Requested shared file is missing.
    FileNotFound,
    /// Peer is unknown and should retry over TCP.
    QueueFull,
}

/// Shared part frame build error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SharedPartError {
    /// Shared file metadata or verified range was not readable.
    #[error("ED2K shared part read failed")]
    ReadFailed,
    /// The selected offset format cannot represent the requested range.
    #[error("ED2K shared part offset is too large")]
    OffsetTooLarge,
}

/// Native ED2K peer credit counters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerCredit {
    /// Remote ED2K user hash.
    pub user_hash: Ed2kHash,
    /// Bytes uploaded to the peer.
    pub uploaded_bytes: u64,
    /// Bytes downloaded from the peer.
    pub downloaded_bytes: u64,
}

/// Native ED2K credit store error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PeerCreditError {
    /// Snapshot contains duplicate user hashes.
    #[error("duplicate ED2K credit entry")]
    DuplicateEntry,
}

/// Native ED2K peer credit store.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeerCreditStore {
    credits: Vec<PeerCredit>,
}

/// Retained upload-queue peer state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadPeer {
    /// Remote peer endpoint.
    pub endpoint: PeerEndpoint,
    /// Optional remote user hash.
    pub user_hash: Option<Ed2kHash>,
    /// Requested shared file hash.
    pub file_hash: Ed2kHash,
    /// Whether this peer owns an upload slot.
    pub uploading: bool,
    /// One-based waiting rank, or zero for an active upload slot.
    pub rank: u16,
    /// First wait timestamp in caller-owned seconds.
    pub wait_start_seconds: u64,
    /// Upload start timestamp in caller-owned seconds.
    pub upload_start_seconds: u64,
    /// Uploaded bytes during this process lifetime.
    pub session_uploaded: u64,
}

/// Deterministic native ED2K upload queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadQueue {
    max_slots: usize,
    max_waiting: usize,
    credits: PeerCreditStore,
    peers: Vec<UploadPeer>,
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

impl UploadQueue {
    /// Create an upload queue with bounded active slots and waiting peers.
    pub fn new(max_slots: usize, max_waiting: usize) -> Self {
        Self {
            max_slots: max_slots.max(1),
            max_waiting,
            credits: PeerCreditStore::default(),
            peers: Vec::new(),
        }
    }

    /// Request an upload slot for a shared file.
    pub fn request_upload(
        &mut self,
        store: &SharedFileStore,
        request: SharedUploadRequest,
    ) -> UploadDecision {
        if store.find_by_hash(&request.file_hash).is_none() {
            return UploadDecision::FileNotShared;
        }
        if self.duplicate_user_hash(request.endpoint, request.user_hash) {
            return UploadDecision::DuplicatePeer;
        }
        if let Some(index) = self.peer_index(request.endpoint) {
            self.peers[index].file_hash = request.file_hash;
            if self.peers[index].user_hash.is_none() {
                self.peers[index].user_hash = request.user_hash;
            }
            return peer_decision(&self.peers[index]);
        }
        if self.uploading_count() < self.max_slots {
            self.peers.push(UploadPeer {
                endpoint: request.endpoint,
                user_hash: request.user_hash,
                file_hash: request.file_hash,
                uploading: true,
                rank: 0,
                wait_start_seconds: 0,
                upload_start_seconds: request.now_seconds,
                session_uploaded: 0,
            });
            return UploadDecision::Accepted;
        }
        if self.waiting_count() >= self.max_waiting {
            return UploadDecision::QueueFull;
        }
        self.peers.push(UploadPeer {
            endpoint: request.endpoint,
            user_hash: request.user_hash,
            file_hash: request.file_hash,
            uploading: false,
            rank: 0,
            wait_start_seconds: request.now_seconds,
            upload_start_seconds: 0,
            session_uploaded: 0,
        });
        self.sort_waiting();
        let peer = self
            .peers
            .iter()
            .find(|peer| peer.endpoint == request.endpoint)
            .expect("queued peer was inserted");
        UploadDecision::Queued { rank: peer.rank }
    }

    /// Return whether a peer currently owns an upload slot.
    pub fn is_uploading(&self, endpoint: PeerEndpoint) -> bool {
        self.peers
            .iter()
            .any(|peer| peer.endpoint == endpoint && peer.uploading)
    }

    /// Return the current queue rank for a peer.
    pub fn queue_rank(&self, endpoint: PeerEndpoint) -> Option<u16> {
        self.peers
            .iter()
            .find(|peer| peer.endpoint == endpoint)
            .map(|peer| peer.rank)
    }

    /// Remove a peer and promote the next waiting peer when a slot opens.
    pub fn remove(&mut self, endpoint: PeerEndpoint) -> bool {
        let Some(index) = self.peer_index(endpoint) else {
            return false;
        };
        let was_uploading = self.peers[index].uploading;
        self.peers.remove(index);
        if was_uploading {
            self.promote_waiting();
        }
        self.sort_waiting();
        true
    }

    /// Return retained peers.
    pub fn peers(&self) -> &[UploadPeer] {
        &self.peers
    }

    /// Return immutable peer credit state.
    pub fn credits(&self) -> &PeerCreditStore {
        &self.credits
    }

    /// Return mutable peer credit state.
    pub fn credits_mut(&mut self) -> &mut PeerCreditStore {
        &mut self.credits
    }

    /// Record uploaded bytes for an active peer.
    pub fn note_uploaded(&mut self, endpoint: PeerEndpoint, bytes: u64) {
        let Some(peer) = self.peers.iter_mut().find(|peer| peer.endpoint == endpoint) else {
            return;
        };
        if bytes == 0 {
            return;
        }
        peer.session_uploaded = peer.session_uploaded.saturating_add(bytes);
        if let Some(user_hash) = peer.user_hash {
            self.credits.add_uploaded(user_hash, bytes);
        }
    }

    /// Record downloaded bytes from a remote ED2K identity.
    pub fn note_downloaded(&mut self, user_hash: Ed2kHash, bytes: u64) {
        self.credits.add_downloaded(user_hash, bytes);
        self.sort_waiting();
    }

    /// Select a truthful UDP reask response.
    pub fn handle_udp_reask(
        &self,
        store: &SharedFileStore,
        endpoint: PeerEndpoint,
        file_hash: Ed2kHash,
    ) -> UdpReaskResponse {
        if store.find_by_hash(&file_hash).is_none() {
            return UdpReaskResponse::FileNotFound;
        }
        let Some(peer) = self.peers.iter().find(|peer| peer.endpoint == endpoint) else {
            return UdpReaskResponse::QueueFull;
        };
        if peer.file_hash != file_hash {
            return UdpReaskResponse::FileNotFound;
        }
        UdpReaskResponse::Ack { rank: peer.rank }
    }

    fn peer_index(&self, endpoint: PeerEndpoint) -> Option<usize> {
        self.peers.iter().position(|peer| peer.endpoint == endpoint)
    }

    fn duplicate_user_hash(&self, endpoint: PeerEndpoint, user_hash: Option<Ed2kHash>) -> bool {
        let Some(user_hash) = user_hash else {
            return false;
        };
        self.peers
            .iter()
            .any(|peer| peer.endpoint != endpoint && peer.user_hash == Some(user_hash))
    }

    fn uploading_count(&self) -> usize {
        self.peers.iter().filter(|peer| peer.uploading).count()
    }

    fn waiting_count(&self) -> usize {
        self.peers.iter().filter(|peer| !peer.uploading).count()
    }

    fn promote_waiting(&mut self) {
        if self.uploading_count() >= self.max_slots {
            return;
        }
        if let Some(peer) = self
            .peers
            .iter_mut()
            .filter(|peer| !peer.uploading)
            .min_by_key(|peer| {
                (
                    peer.wait_start_seconds,
                    peer.endpoint.ip,
                    peer.endpoint.port,
                )
            })
        {
            peer.uploading = true;
            peer.rank = 0;
            peer.upload_start_seconds = peer.wait_start_seconds;
        }
    }

    fn sort_waiting(&mut self) {
        self.peers.sort_by(|lhs, rhs| {
            (!lhs.uploading)
                .cmp(&!rhs.uploading)
                .then_with(|| {
                    if lhs.uploading {
                        lhs.upload_start_seconds.cmp(&rhs.upload_start_seconds)
                    } else {
                        rhs.credit_score(&self.credits)
                            .total_cmp(&lhs.credit_score(&self.credits))
                            .then_with(|| lhs.wait_start_seconds.cmp(&rhs.wait_start_seconds))
                    }
                })
                .then_with(|| lhs.endpoint.ip.cmp(&rhs.endpoint.ip))
                .then_with(|| lhs.endpoint.port.cmp(&rhs.endpoint.port))
        });
        let mut rank = 1_u16;
        for peer in &mut self.peers {
            if peer.uploading {
                peer.rank = 0;
            } else {
                peer.rank = rank;
                rank = rank.saturating_add(1);
            }
        }
    }
}

impl UploadPeer {
    fn credit_score(&self, credits: &PeerCreditStore) -> f64 {
        self.user_hash
            .as_ref()
            .map_or(1.0, |user_hash| credits.score_ratio(user_hash))
    }
}

impl PeerCreditStore {
    /// Record bytes uploaded to a peer.
    pub fn add_uploaded(&mut self, user_hash: Ed2kHash, bytes: u64) {
        if bytes == 0 {
            return;
        }
        let credit = self.get_or_create(user_hash);
        credit.uploaded_bytes = credit.uploaded_bytes.saturating_add(bytes);
    }

    /// Record bytes downloaded from a peer.
    pub fn add_downloaded(&mut self, user_hash: Ed2kHash, bytes: u64) {
        if bytes == 0 {
            return;
        }
        let credit = self.get_or_create(user_hash);
        credit.downloaded_bytes = credit.downloaded_bytes.saturating_add(bytes);
    }

    /// Return the bounded eMule-style queue score ratio.
    pub fn score_ratio(&self, user_hash: &Ed2kHash) -> f64 {
        let Some(credit) = self
            .credits
            .iter()
            .find(|credit| &credit.user_hash == user_hash)
        else {
            return 1.0;
        };
        if credit.downloaded_bytes < 1_000_000 {
            return 1.0;
        }
        let ratio = if credit.uploaded_bytes == 0 {
            10.0
        } else {
            credit.downloaded_bytes as f64 * 2.0 / credit.uploaded_bytes as f64
        };
        let limit = (credit.downloaded_bytes as f64 / 1_048_576.0 + 2.0).sqrt();
        ratio.min(limit).clamp(1.0, 10.0)
    }

    /// Return all retained credit entries.
    pub fn list(&self) -> &[PeerCredit] {
        &self.credits
    }

    /// Return a native serializable snapshot.
    pub fn snapshot(&self) -> Vec<PeerCredit> {
        self.credits.clone()
    }

    /// Restore a credit store from a native snapshot.
    pub fn from_snapshot(snapshot: Vec<PeerCredit>) -> Result<Self, PeerCreditError> {
        let mut credits = Vec::new();
        for credit in snapshot {
            if credits
                .iter()
                .any(|existing: &PeerCredit| existing.user_hash == credit.user_hash)
            {
                return Err(PeerCreditError::DuplicateEntry);
            }
            credits.push(credit);
        }
        Ok(Self { credits })
    }

    fn get_or_create(&mut self, user_hash: Ed2kHash) -> &mut PeerCredit {
        if let Some(index) = self
            .credits
            .iter()
            .position(|credit| credit.user_hash == user_hash)
        {
            return &mut self.credits[index];
        }
        self.credits.push(PeerCredit {
            user_hash,
            uploaded_bytes: 0,
            downloaded_bytes: 0,
        });
        self.credits.last_mut().expect("credit was inserted")
    }
}

/// Build a TCP upload response frame from an upload decision.
pub fn build_upload_response_frame(decision: UploadDecision) -> PacketFrame {
    match decision {
        UploadDecision::Accepted => peer_frame(PeerOpcode::AcceptUploadRequest, Vec::new()),
        UploadDecision::Queued { rank } => {
            peer_frame(PeerOpcode::QueueRank, rank.to_le_bytes().to_vec())
        }
        UploadDecision::FileNotShared => peer_frame(PeerOpcode::FileRequestNoFile, Vec::new()),
        UploadDecision::DuplicatePeer | UploadDecision::QueueFull => PacketFrame {
            protocol: Protocol::Emule,
            opcode: PeerOpcode::QueueFull.into(),
            payload: Vec::new(),
        },
    }
}

/// Build a UDP reask response frame.
pub fn build_udp_reask_frame(response: UdpReaskResponse) -> PacketFrame {
    match response {
        UdpReaskResponse::Ack { rank } => PacketFrame {
            protocol: Protocol::Emule,
            opcode: PeerOpcode::ReaskAck.into(),
            payload: rank.to_le_bytes().to_vec(),
        },
        UdpReaskResponse::FileNotFound => PacketFrame {
            protocol: Protocol::Emule,
            opcode: PeerOpcode::FileNotFound.into(),
            payload: Vec::new(),
        },
        UdpReaskResponse::QueueFull => PacketFrame {
            protocol: Protocol::Emule,
            opcode: PeerOpcode::QueueFull.into(),
            payload: Vec::new(),
        },
    }
}

/// Build a normal shared-part payload frame from verified shared bytes.
pub fn build_shared_part_frame(
    store: &SharedFileStore,
    file_hash: Ed2kHash,
    range: PartRange,
    use_i64_offsets: bool,
) -> Result<PacketFrame, SharedPartError> {
    if !use_i64_offsets && (range.begin > u64::from(u32::MAX) || range.end > u64::from(u32::MAX)) {
        return Err(SharedPartError::OffsetTooLarge);
    }
    let data = store
        .read_verified_range(&file_hash, range)
        .map_err(|_| SharedPartError::ReadFailed)?;
    let mut payload = file_hash.to_vec();
    if use_i64_offsets {
        payload.extend_from_slice(&range.begin.to_le_bytes());
        payload.extend_from_slice(&range.end.to_le_bytes());
    } else {
        payload.extend_from_slice(&(range.begin as u32).to_le_bytes());
        payload.extend_from_slice(&(range.end as u32).to_le_bytes());
    }
    payload.extend_from_slice(&data);
    Ok(PacketFrame {
        protocol: if use_i64_offsets {
            Protocol::Emule
        } else {
            Protocol::Edonkey
        },
        opcode: if use_i64_offsets {
            PeerOpcode::SendingPartI64.into()
        } else {
            PeerOpcode::SendingPart.into()
        },
        payload,
    })
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

fn peer_decision(peer: &UploadPeer) -> UploadDecision {
    if peer.uploading {
        UploadDecision::Accepted
    } else {
        UploadDecision::Queued { rank: peer.rank }
    }
}

fn peer_frame(opcode: PeerOpcode, payload: Vec<u8>) -> PacketFrame {
    PacketFrame {
        protocol: Protocol::Edonkey,
        opcode: opcode.into(),
        payload,
    }
}
