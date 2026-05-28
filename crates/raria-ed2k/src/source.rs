//! ED2K Source Exchange and source lifecycle ownership.

use crate::hash::Ed2kHash;
use crate::opcode::PeerOpcode;
use crate::packet::{PacketFrame, Protocol};
use crate::peer::{PeerCapabilities, PeerEndpoint, PeerRequestPhase};
use crate::wire::Cursor;

const SOURCE_EXCHANGE_VERSION: u8 = 4;
const MAX_SOURCE_EXCHANGE_ENTRIES: usize = 500;
const CRYPT_REQUIRED: u8 = 0x04;

/// ED2K source endpoint identity.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct SourceEndpoint {
    /// IPv4 address as the ED2K wire integer.
    pub ip: u32,
    /// TCP port.
    pub port: u16,
}

impl SourceEndpoint {
    /// Create a source endpoint.
    pub fn new(ip: u32, port: u16) -> Self {
        Self { ip, port }
    }

    fn from_peer(endpoint: PeerEndpoint) -> Self {
        Self {
            ip: endpoint.ip,
            port: endpoint.port,
        }
    }

    fn as_peer(self) -> PeerEndpoint {
        PeerEndpoint {
            ip: self.ip,
            port: self.port,
        }
    }
}

/// Source Exchange entry retained by raria.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceExchangeEntry {
    /// Source peer endpoint.
    pub endpoint: PeerEndpoint,
    /// Optional source server endpoint.
    pub server: Option<PeerEndpoint>,
    /// Optional source user hash.
    pub user_hash: Option<Ed2kHash>,
    /// Optional eMule crypt option byte.
    pub crypt_options: Option<u8>,
}

impl SourceExchangeEntry {
    /// Return whether the source can be scheduled before crypt-layer support exists.
    pub fn is_schedulable_without_crypt(&self) -> bool {
        self.crypt_options
            .is_none_or(|options| options & CRYPT_REQUIRED == 0)
    }
}

/// Parsed Source Exchange answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceExchangeAnswer {
    /// Source Exchange version used for the payload.
    pub version: u8,
    /// Parsed source entries.
    pub entries: Vec<SourceExchangeEntry>,
}

/// Source Exchange parse or encode error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceExchangeError {
    /// Payload is malformed or truncated.
    #[error("truncated ED2K Source Exchange payload")]
    Truncated,
    /// Payload file hash does not match the expected file.
    #[error("ED2K Source Exchange hash mismatch")]
    HashMismatch,
    /// Source Exchange version is unsupported.
    #[error("unsupported ED2K Source Exchange version: {0}")]
    UnsupportedVersion(u8),
    /// Entry count exceeds the retained limit.
    #[error("too many ED2K Source Exchange entries")]
    TooManyEntries,
    /// Frame opcode or protocol does not match Source Exchange.
    #[error("unexpected ED2K Source Exchange frame")]
    UnexpectedFrame,
}

/// Source origin retained by source lifecycle policy.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceOrigin {
    /// Inline source from an ED2K link.
    Inline,
    /// Server TCP or UDP source discovery.
    Server,
    /// Kad source discovery.
    Kad,
    /// Peer Source Exchange.
    SourceExchange,
}

/// Source scheduling quality.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SourceQuality {
    /// New or freshly updated source.
    Fresh,
    /// Previously queued source.
    Queued,
    /// Source previously had no needed parts.
    NoNeededParts,
    /// Source recovered after dead-source expiry.
    Recovered,
}

/// Native ED2K source lifecycle record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecord {
    /// Source endpoint.
    pub endpoint: SourceEndpoint,
    /// Optional server endpoint.
    pub server: Option<PeerEndpoint>,
    /// Optional source user hash.
    pub user_hash: Option<Ed2kHash>,
    /// Optional crypt options.
    pub crypt_options: Option<u8>,
    /// Best known origin.
    pub origin: SourceOrigin,
    /// Current source quality.
    pub quality: SourceQuality,
    /// Last update timestamp in caller-owned seconds.
    pub last_seen_at: u64,
    /// Next timestamp when the source may be scheduled.
    pub next_retry_at: u64,
    active: bool,
}

/// Native source lifecycle store for one ED2K task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLifecycle {
    self_endpoint: SourceEndpoint,
    active_limit: usize,
    sources: Vec<SourceRecord>,
}

impl SourceLifecycle {
    /// Create a source lifecycle store.
    pub fn new(self_endpoint: SourceEndpoint, active_limit: usize) -> Self {
        Self {
            self_endpoint,
            active_limit,
            sources: Vec::new(),
        }
    }

    /// Merge a source candidate. Returns true when a new schedulable source is inserted.
    pub fn merge(
        &mut self,
        entry: SourceExchangeEntry,
        origin: SourceOrigin,
        now_seconds: u64,
    ) -> bool {
        if !is_useful_endpoint(entry.endpoint) {
            return false;
        }
        let endpoint = SourceEndpoint::from_peer(entry.endpoint);
        if endpoint == self.self_endpoint || is_loopback(endpoint.ip) {
            return false;
        }
        if let Some(existing) = self
            .sources
            .iter_mut()
            .find(|source| source.endpoint == endpoint)
        {
            existing.last_seen_at = now_seconds;
            if entry.server.is_some() {
                existing.server = entry.server;
            }
            if entry.user_hash.is_some() {
                existing.user_hash = entry.user_hash;
            }
            if entry.crypt_options.is_some() && entry.is_schedulable_without_crypt() {
                existing.crypt_options = entry.crypt_options;
            }
            if origin < existing.origin {
                existing.origin = origin;
            }
            return false;
        }
        if !entry.is_schedulable_without_crypt() {
            return false;
        }
        self.sources.push(SourceRecord {
            endpoint,
            server: entry.server,
            user_hash: entry.user_hash,
            crypt_options: entry.crypt_options,
            origin,
            quality: SourceQuality::Fresh,
            last_seen_at: now_seconds,
            next_retry_at: now_seconds,
            active: false,
        });
        true
    }

    /// Return the number of retained sources.
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// Return whether no sources are retained.
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Return retained source records.
    pub fn sources(&self) -> &[SourceRecord] {
        &self.sources
    }

    /// Select the next schedulable source under active caps and retry windows.
    pub fn next_schedulable(&self, now_seconds: u64) -> Option<&SourceRecord> {
        if self.sources.iter().filter(|source| source.active).count() >= self.active_limit {
            return None;
        }
        self.sources
            .iter()
            .filter(|source| !source.active && source.next_retry_at <= now_seconds)
            .min_by_key(|source| (source.quality_rank(), source.next_retry_at))
    }

    /// Mark a source active.
    pub fn mark_active(&mut self, endpoint: SourceEndpoint) {
        if let Some(source) = self
            .sources
            .iter_mut()
            .find(|source| source.endpoint == endpoint)
        {
            source.active = true;
        }
    }

    /// Mark a source queued and apply a retry delay.
    pub fn mark_queued(&mut self, endpoint: SourceEndpoint, retry_after_seconds: u64, now: u64) {
        if let Some(source) = self
            .sources
            .iter_mut()
            .find(|source| source.endpoint == endpoint)
        {
            source.active = false;
            source.quality = SourceQuality::Queued;
            source.next_retry_at = now.saturating_add(retry_after_seconds);
        }
    }

    /// Mark a source dead and apply an expiry delay before retry.
    pub fn mark_dead(&mut self, endpoint: SourceEndpoint, now: u64, expiry_seconds: u64) {
        if let Some(source) = self
            .sources
            .iter_mut()
            .find(|source| source.endpoint == endpoint)
        {
            source.active = false;
            source.quality = SourceQuality::Recovered;
            source.next_retry_at = now.saturating_add(expiry_seconds);
        }
    }

    /// Update lifecycle state from peer request phase.
    pub fn update_phase(
        &mut self,
        endpoint: SourceEndpoint,
        phase: PeerRequestPhase,
        now_seconds: u64,
    ) {
        if let Some(source) = self
            .sources
            .iter_mut()
            .find(|source| source.endpoint == endpoint)
        {
            source.active = false;
            source.last_seen_at = now_seconds;
            match phase {
                PeerRequestPhase::OnQueue => {
                    source.quality = SourceQuality::Queued;
                    source.next_retry_at = now_seconds;
                }
                PeerRequestPhase::NoNeededParts | PeerRequestPhase::OutOfParts => {
                    source.quality = SourceQuality::NoNeededParts;
                    source.next_retry_at = now_seconds;
                }
                PeerRequestPhase::NoFile
                | PeerRequestPhase::Cancelled
                | PeerRequestPhase::Failed => {
                    source.quality = SourceQuality::Recovered;
                    source.next_retry_at = now_seconds.saturating_add(60);
                }
                _ => {
                    source.quality = SourceQuality::Fresh;
                    source.next_retry_at = now_seconds;
                }
            }
        }
    }
}

impl SourceRecord {
    fn quality_rank(&self) -> u8 {
        match self.quality {
            SourceQuality::Fresh => 0,
            SourceQuality::Queued => 1,
            SourceQuality::NoNeededParts => 2,
            SourceQuality::Recovered => 3,
        }
    }
}

/// Build a Source Exchange request frame from peer capability truth.
pub fn build_source_exchange_request(
    file_hash: Ed2kHash,
    capabilities: PeerCapabilities,
) -> Option<PacketFrame> {
    if capabilities.supports_source_exchange2 {
        let mut payload = Vec::with_capacity(19);
        payload.push(SOURCE_EXCHANGE_VERSION);
        payload.extend_from_slice(&0_u16.to_le_bytes());
        payload.extend_from_slice(&file_hash);
        return Some(PacketFrame {
            protocol: Protocol::Emule,
            opcode: PeerOpcode::RequestSources2.into(),
            payload,
        });
    }
    if capabilities.source_exchange1_version > 1 {
        return Some(PacketFrame {
            protocol: Protocol::Emule,
            opcode: PeerOpcode::RequestSources.into(),
            payload: file_hash.to_vec(),
        });
    }
    None
}

/// Parse the requested Source Exchange version from a request frame.
pub fn source_exchange_request_version(
    frame: &PacketFrame,
    expected_file_hash: Ed2kHash,
) -> Result<u8, SourceExchangeError> {
    if frame.protocol != Protocol::Emule {
        return Err(SourceExchangeError::UnexpectedFrame);
    }
    if frame.opcode == u8::from(PeerOpcode::RequestSources) {
        if frame.payload.as_slice() == expected_file_hash {
            return Ok(1);
        }
        return Err(SourceExchangeError::HashMismatch);
    }
    if frame.opcode != u8::from(PeerOpcode::RequestSources2) {
        return Err(SourceExchangeError::UnexpectedFrame);
    }
    if frame.payload.len() != 19 {
        return Err(SourceExchangeError::Truncated);
    }
    let version = frame.payload[0];
    if version == 0 || version > SOURCE_EXCHANGE_VERSION {
        return Err(SourceExchangeError::UnsupportedVersion(version));
    }
    if frame.payload[3..19] != expected_file_hash {
        return Err(SourceExchangeError::HashMismatch);
    }
    Ok(version)
}

/// Build a Source Exchange answer frame.
pub fn build_source_exchange_answer(
    file_hash: Ed2kHash,
    version: u8,
    source_exchange2: bool,
    entries: &[SourceExchangeEntry],
) -> Result<PacketFrame, SourceExchangeError> {
    validate_version(version)?;
    if entries.len() > MAX_SOURCE_EXCHANGE_ENTRIES {
        return Err(SourceExchangeError::TooManyEntries);
    }
    let mut payload = Vec::new();
    if source_exchange2 {
        payload.push(version);
    }
    payload.extend_from_slice(&file_hash);
    payload.extend_from_slice(
        &u16::try_from(entries.len())
            .map_err(|_| SourceExchangeError::TooManyEntries)?
            .to_le_bytes(),
    );
    for entry in entries {
        write_endpoint(&mut payload, entry.endpoint);
        write_endpoint(
            &mut payload,
            entry.server.unwrap_or(PeerEndpoint { ip: 0, port: 0 }),
        );
        if version >= 2 {
            payload.extend_from_slice(&entry.user_hash.unwrap_or([0_u8; 16]));
        }
        if version >= 4 {
            payload.push(entry.crypt_options.unwrap_or(0));
        }
    }
    Ok(PacketFrame {
        protocol: Protocol::Emule,
        opcode: if source_exchange2 {
            PeerOpcode::AnswerSources2.into()
        } else {
            PeerOpcode::AnswerSources.into()
        },
        payload,
    })
}

/// Parse a Source Exchange answer frame.
pub fn parse_source_exchange_answer(
    frame: &PacketFrame,
    expected_file_hash: Ed2kHash,
    source_exchange1_version: Option<u8>,
) -> Result<SourceExchangeAnswer, SourceExchangeError> {
    if frame.protocol != Protocol::Emule {
        return Err(SourceExchangeError::UnexpectedFrame);
    }
    match PeerOpcode::from_byte(frame.opcode) {
        Some(PeerOpcode::AnswerSources2) => parse_answer_sx2(&frame.payload, expected_file_hash),
        Some(PeerOpcode::AnswerSources) => parse_answer_sx1(
            &frame.payload,
            expected_file_hash,
            source_exchange1_version.ok_or(SourceExchangeError::UnexpectedFrame)?,
        ),
        _ => Err(SourceExchangeError::UnexpectedFrame),
    }
}

fn parse_answer_sx2(
    payload: &[u8],
    expected_file_hash: Ed2kHash,
) -> Result<SourceExchangeAnswer, SourceExchangeError> {
    let mut cursor = Cursor::new(payload);
    let version = cursor.read_u8().ok_or(SourceExchangeError::Truncated)?;
    validate_version(version)?;
    read_expected_hash(&mut cursor, expected_file_hash)?;
    parse_entries(&mut cursor, version)
}

fn parse_answer_sx1(
    payload: &[u8],
    expected_file_hash: Ed2kHash,
    version: u8,
) -> Result<SourceExchangeAnswer, SourceExchangeError> {
    validate_version(version)?;
    let mut cursor = Cursor::new(payload);
    read_expected_hash(&mut cursor, expected_file_hash)?;
    parse_entries(&mut cursor, version)
}

fn parse_entries(
    cursor: &mut Cursor<'_>,
    version: u8,
) -> Result<SourceExchangeAnswer, SourceExchangeError> {
    let count = cursor.read_u16().ok_or(SourceExchangeError::Truncated)? as usize;
    if count > MAX_SOURCE_EXCHANGE_ENTRIES {
        return Err(SourceExchangeError::TooManyEntries);
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let endpoint = read_endpoint(cursor)?;
        let raw_server = read_endpoint(cursor)?;
        let user_hash = if version >= 2 {
            Some(cursor.read_hash16().ok_or(SourceExchangeError::Truncated)?)
        } else {
            None
        };
        let crypt_options = if version >= 4 {
            Some(cursor.read_u8().ok_or(SourceExchangeError::Truncated)?)
        } else {
            None
        };
        entries.push(SourceExchangeEntry {
            endpoint,
            server: (raw_server.ip != 0 && raw_server.port != 0).then_some(raw_server),
            user_hash,
            crypt_options,
        });
    }
    if !cursor.is_done() {
        return Err(SourceExchangeError::Truncated);
    }
    Ok(SourceExchangeAnswer { version, entries })
}

fn validate_version(version: u8) -> Result<(), SourceExchangeError> {
    if version == 0 || version > SOURCE_EXCHANGE_VERSION {
        return Err(SourceExchangeError::UnsupportedVersion(version));
    }
    Ok(())
}

fn read_expected_hash(
    cursor: &mut Cursor<'_>,
    expected_file_hash: Ed2kHash,
) -> Result<(), SourceExchangeError> {
    let hash = cursor.read_hash16().ok_or(SourceExchangeError::Truncated)?;
    if hash != expected_file_hash {
        return Err(SourceExchangeError::HashMismatch);
    }
    Ok(())
}

fn write_endpoint(out: &mut Vec<u8>, endpoint: PeerEndpoint) {
    out.extend_from_slice(&endpoint.ip.to_le_bytes());
    out.extend_from_slice(&endpoint.port.to_le_bytes());
}

fn read_endpoint(cursor: &mut Cursor<'_>) -> Result<PeerEndpoint, SourceExchangeError> {
    Ok(PeerEndpoint {
        ip: cursor.read_u32().ok_or(SourceExchangeError::Truncated)?,
        port: cursor.read_u16().ok_or(SourceExchangeError::Truncated)?,
    })
}

fn is_useful_endpoint(endpoint: PeerEndpoint) -> bool {
    endpoint.ip != 0 && endpoint.port != 0
}

fn is_loopback(ip: u32) -> bool {
    ip & 0xff == 127 || (ip >> 24) & 0xff == 127
}

impl From<SourceEndpoint> for PeerEndpoint {
    fn from(value: SourceEndpoint) -> Self {
        value.as_peer()
    }
}
