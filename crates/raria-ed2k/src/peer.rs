//! ED2K peer handshake, capability, queue, and request-state ownership.

use crate::hash::Ed2kHash;
use crate::opcode::ServerOpcode;
use crate::opcode::{EmuleOpcode, PeerOpcode};
use crate::packet::{PacketFrame, Protocol};
use crate::server::is_low_id;
use crate::tag::{Tag, TagError, TagName, TagValue, decode_tag_prefix, encode_tag};
use crate::wire::Cursor;

const HASH_SIZE: u8 = 16;
const EMULE_PROTOCOL_VERSION: u8 = 0x01;
const EDONKEY_CLIENT_VERSION: u32 = 0x3c;
const COMPATIBLE_CLIENT_RARIA: u8 = 0x03;
const RARIA_EMULE_VERSION: u32 = 3 << 17;
const CT_NAME: u8 = 0x01;
const CT_VERSION: u8 = 0x11;
const CT_EMULE_UDPPORTS: u8 = 0xf9;
const CT_EMULECOMPAT_OPTIONS: u8 = 0xef;
const CT_EMULE_MISCOPTIONS1: u8 = 0xfa;
const CT_EMULE_VERSION: u8 = 0xfb;
const CT_EMULE_MISCOPTIONS2: u8 = 0xfe;
const ET_COMPRESSION: u8 = 0x20;
const ET_UDPPORT: u8 = 0x21;
const ET_UDPVER: u8 = 0x22;
const ET_SOURCEEXCHANGE: u8 = 0x23;
const ET_EXTENDEDREQUEST: u8 = 0x25;
const ET_COMPATIBLECLIENT: u8 = 0x26;
const ET_FEATURES: u8 = 0x27;

/// Native peer endpoint encoded inside ED2K hello packets.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PeerEndpoint {
    /// IPv4 address as the ED2K wire integer.
    pub ip: u32,
    /// TCP port.
    pub port: u16,
}

/// Native ED2K peer identity used by hello and hello answer packets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerIdentity {
    /// Stable ED2K user hash.
    pub user_hash: Ed2kHash,
    /// Server-assigned client ID or zero before assignment.
    pub client_id: u32,
    /// TCP listen port.
    pub tcp_port: u16,
    /// UDP listen port.
    pub udp_port: u16,
    /// Kad UDP listen port when Kad is fully owned.
    pub kad_udp_port: u16,
    /// Last server endpoint advertised to the peer.
    pub server: Option<PeerEndpoint>,
    /// Native client display name.
    pub name: String,
}

impl PeerIdentity {
    /// Create a local identity template for tests and bootstrap state.
    pub fn default_for_name(name: impl Into<String>) -> Self {
        Self {
            user_hash: [0_u8; 16],
            client_id: 0,
            tcp_port: 0,
            udp_port: 0,
            kad_udp_port: 0,
            server: None,
            name: name.into(),
        }
    }
}

/// ED2K/eMule peer capability truth retained by raria.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct PeerCapabilities {
    /// AICH capability version.
    pub aich_version: u8,
    /// Unicode tag and filename support.
    pub unicode: bool,
    /// UDP peer protocol version.
    pub udp_version: u8,
    /// Zlib data compression version.
    pub data_compression_version: u8,
    /// Secure-ident support version.
    pub secure_ident_version: u8,
    /// Source Exchange v1 support version.
    pub source_exchange1_version: u8,
    /// Extended request support version.
    pub extended_requests_version: u8,
    /// Comment support version.
    pub accepts_comments: bool,
    /// Multipacket support.
    pub supports_multipacket: bool,
    /// Preview support.
    pub supports_preview: bool,
    /// Direct UDP callback support.
    pub supports_direct_udp_callback: bool,
    /// Captcha support.
    pub supports_captcha: bool,
    /// Source Exchange v2 support.
    pub supports_source_exchange2: bool,
    /// Required crypt-layer support.
    pub requires_crypt_layer: bool,
    /// Requested crypt-layer support.
    pub requests_crypt_layer: bool,
    /// Supported crypt-layer support.
    pub supports_crypt_layer: bool,
    /// Extended multipacket support.
    pub supports_extended_multipacket: bool,
    /// Large-file support.
    pub supports_large_files: bool,
    /// Kad version advertised to peers.
    pub kad_version: u8,
}

impl PeerCapabilities {
    /// Return the locally advertised raria ED2K peer capabilities.
    pub fn local() -> Self {
        Self {
            aich_version: 1,
            unicode: true,
            udp_version: 4,
            data_compression_version: 1,
            secure_ident_version: 0,
            source_exchange1_version: 3,
            extended_requests_version: 2,
            accepts_comments: false,
            supports_multipacket: false,
            supports_preview: false,
            supports_direct_udp_callback: false,
            supports_captcha: false,
            supports_source_exchange2: true,
            requires_crypt_layer: false,
            requests_crypt_layer: false,
            supports_crypt_layer: false,
            supports_extended_multipacket: false,
            supports_large_files: true,
            kad_version: 0,
        }
    }
}

/// Parsed peer hello metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPeerHello {
    /// Peer identity from the hello packet.
    pub identity: PeerIdentity,
    /// Peer capability metadata.
    pub capabilities: PeerCapabilities,
}

/// Parsed eMule info metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedEmuleInfo {
    /// eMule peer version byte.
    pub version: u8,
    /// eMule protocol version byte.
    pub protocol_version: u8,
    /// Peer UDP listen port.
    pub udp_port: u16,
    /// Peer capability metadata.
    pub capabilities: PeerCapabilities,
}

/// Peer handshake parse or encode error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PeerHandshakeError {
    /// The frame uses the wrong protocol marker.
    #[error("unexpected ED2K peer handshake protocol: {0:?}")]
    UnexpectedProtocol(Protocol),
    /// The frame uses the wrong opcode.
    #[error("unexpected ED2K peer handshake opcode: 0x{0:02x}")]
    UnexpectedOpcode(u8),
    /// Hello hash-size prefix is not the retained 16-byte hash size.
    #[error("invalid ED2K hello hash size: {0}")]
    InvalidHashSize(u8),
    /// The payload is malformed or truncated.
    #[error("truncated ED2K peer handshake payload")]
    Truncated,
    /// Tag encoding or decoding failed.
    #[error(transparent)]
    Tag(#[from] TagError),
    /// The encoded value exceeds retained wire limits.
    #[error("ED2K peer handshake value is too large")]
    ValueTooLarge,
}

/// Build a peer hello frame.
pub fn build_peer_hello(identity: &PeerIdentity) -> Result<PacketFrame, PeerHandshakeError> {
    Ok(PacketFrame {
        protocol: Protocol::Edonkey,
        opcode: PeerOpcode::Hello.into(),
        payload: encode_peer_hello_payload(identity, true)?,
    })
}

/// Build a peer hello answer frame.
pub fn build_peer_hello_answer(identity: &PeerIdentity) -> Result<PacketFrame, PeerHandshakeError> {
    Ok(PacketFrame {
        protocol: Protocol::Edonkey,
        opcode: PeerOpcode::HelloAnswer.into(),
        payload: encode_peer_hello_payload(identity, false)?,
    })
}

/// Parse a peer hello or hello answer frame.
pub fn parse_peer_hello(frame: &PacketFrame) -> Result<ParsedPeerHello, PeerHandshakeError> {
    if frame.protocol != Protocol::Edonkey {
        return Err(PeerHandshakeError::UnexpectedProtocol(frame.protocol));
    }
    let has_hash_size = match PeerOpcode::from_byte(frame.opcode) {
        Some(PeerOpcode::Hello) => true,
        Some(PeerOpcode::HelloAnswer) => false,
        _ => return Err(PeerHandshakeError::UnexpectedOpcode(frame.opcode)),
    };
    decode_peer_hello_payload(&frame.payload, has_hash_size)
}

/// Build a retained eMule info or info answer frame.
pub fn build_emule_info(udp_port: u16, answer: bool) -> Result<PacketFrame, PeerHandshakeError> {
    let caps = PeerCapabilities::local();
    let tags = vec![
        Tag::new(
            TagName::Id(ET_COMPRESSION),
            TagValue::UInt32(u32::from(caps.data_compression_version)),
        ),
        Tag::new(
            TagName::Id(ET_UDPPORT),
            TagValue::UInt32(u32::from(udp_port)),
        ),
        Tag::new(
            TagName::Id(ET_UDPVER),
            TagValue::UInt32(u32::from(caps.udp_version)),
        ),
        Tag::new(
            TagName::Id(ET_SOURCEEXCHANGE),
            TagValue::UInt32(u32::from(caps.source_exchange1_version)),
        ),
        Tag::new(
            TagName::Id(ET_EXTENDEDREQUEST),
            TagValue::UInt32(u32::from(caps.extended_requests_version)),
        ),
        Tag::new(
            TagName::Id(ET_FEATURES),
            TagValue::UInt32(u32::from(caps.secure_ident_version)),
        ),
        Tag::new(
            TagName::Id(ET_COMPATIBLECLIENT),
            TagValue::UInt32(u32::from(COMPATIBLE_CLIENT_RARIA)),
        ),
    ];
    let mut payload = Vec::new();
    payload.push(EDONKEY_CLIENT_VERSION as u8);
    payload.push(EMULE_PROTOCOL_VERSION);
    write_tags(&mut payload, &tags)?;
    Ok(PacketFrame {
        protocol: Protocol::Emule,
        opcode: if answer {
            EmuleOpcode::InfoAnswer.into()
        } else {
            EmuleOpcode::Info.into()
        },
        payload,
    })
}

/// Parse a retained eMule info or info answer frame.
pub fn parse_emule_info(frame: &PacketFrame) -> Result<ParsedEmuleInfo, PeerHandshakeError> {
    if frame.protocol != Protocol::Emule {
        return Err(PeerHandshakeError::UnexpectedProtocol(frame.protocol));
    }
    if frame.opcode != u8::from(EmuleOpcode::Info)
        && frame.opcode != u8::from(EmuleOpcode::InfoAnswer)
    {
        return Err(PeerHandshakeError::UnexpectedOpcode(frame.opcode));
    }
    let mut cursor = Cursor::new(&frame.payload);
    let version = cursor.read_u8().ok_or(PeerHandshakeError::Truncated)?;
    let protocol_version = cursor.read_u8().ok_or(PeerHandshakeError::Truncated)?;
    let tags = read_tags(&mut cursor)?;
    if !cursor.is_done() {
        return Err(PeerHandshakeError::Truncated);
    }
    let mut caps = PeerCapabilities::default();
    let mut udp_port = 0;
    for tag in tags {
        let TagName::Id(id) = tag.name else {
            continue;
        };
        let Some(value) = tag_u32(&tag.value) else {
            continue;
        };
        match id {
            ET_COMPRESSION => caps.data_compression_version = clamp_nibble(value),
            ET_UDPPORT => udp_port = value as u16,
            ET_UDPVER => caps.udp_version = clamp_nibble(value),
            ET_SOURCEEXCHANGE => caps.source_exchange1_version = clamp_nibble(value),
            ET_EXTENDEDREQUEST => caps.extended_requests_version = clamp_nibble(value),
            ET_FEATURES => {
                caps.secure_ident_version = (value & 0x03) as u8;
                caps.supports_preview = value & 0x80 != 0;
            }
            _ => {}
        }
    }
    Ok(ParsedEmuleInfo {
        version,
        protocol_version,
        udp_port,
        capabilities: caps,
    })
}

/// Peer request payload parse or encode error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PeerRequestError {
    /// The payload is malformed or truncated.
    #[error("truncated ED2K peer request payload")]
    Truncated,
    /// The payload file hash does not match the expected file.
    #[error("ED2K peer request hash mismatch")]
    HashMismatch,
    /// The encoded value exceeds retained wire limits.
    #[error("ED2K peer request value is too large")]
    ValueTooLarge,
}

/// Native peer request state phase.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PeerRequestPhase {
    /// Peer is connected and ready for the next request.
    Connected,
    /// Peer file status is known and hashset is being requested.
    RequestingHashset,
    /// Peer is queued remotely.
    OnQueue,
    /// Peer accepted the upload request and can receive part requests.
    Downloading,
    /// Peer has no needed parts for this task.
    NoNeededParts,
    /// Peer does not have the requested file.
    NoFile,
    /// Peer currently has no requested parts.
    OutOfParts,
    /// Transfer was cancelled.
    Cancelled,
    /// Peer failed because of a bad packet or disconnect.
    Failed,
}

/// Next action selected after a peer request-state transition.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PeerRequestAction {
    /// No immediate request should be sent.
    None,
    /// Request the remote ED2K hashset.
    RequestHashset,
    /// Request an upload slot.
    StartUpload,
}

/// Peer-owned requested byte range.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PeerRequestedRange {
    /// Inclusive range start offset.
    pub begin: u64,
    /// Exclusive range end offset.
    pub end: u64,
}

/// Failure reason that terminates or backs off a peer request flow.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PeerFailureKind {
    /// Remote peer does not have the requested file.
    NoFile,
    /// Remote peer has no useful parts now.
    OutOfParts,
    /// Remote or local side cancelled the transfer.
    Cancelled,
    /// Remote packet was malformed.
    BadPacket,
    /// Peer disconnected during the request flow.
    Disconnected,
}

/// Native ED2K peer file-request state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRequestState {
    /// Requested file hash.
    pub file_hash: Ed2kHash,
    /// Last remote part availability bitfield.
    pub part_status: Vec<bool>,
    /// Verified or pending remote piece hashes.
    pub piece_hashes: Vec<Ed2kHash>,
    /// Last remote queue rank.
    pub queue_rank: Option<u16>,
    /// Peer-owned requested ranges that must be released on failures.
    pub requested_ranges: Vec<PeerRequestedRange>,
    /// Current request phase.
    pub phase: PeerRequestPhase,
}

impl PeerRequestState {
    /// Create request state for one peer and file.
    pub fn new(file_hash: Ed2kHash) -> Self {
        Self {
            file_hash,
            part_status: Vec::new(),
            piece_hashes: Vec::new(),
            queue_rank: None,
            requested_ranges: Vec::new(),
            phase: PeerRequestPhase::Connected,
        }
    }

    /// Apply a remote file-status bitfield and choose the next request.
    pub fn apply_file_status(
        &mut self,
        part_status: Vec<bool>,
        hashset_required: bool,
    ) -> PeerRequestAction {
        let has_needed_part = part_status.iter().any(|available| *available);
        self.part_status = part_status;
        self.queue_rank = None;
        if !has_needed_part {
            self.phase = PeerRequestPhase::NoNeededParts;
            return PeerRequestAction::None;
        }
        if hashset_required {
            self.phase = PeerRequestPhase::RequestingHashset;
            return PeerRequestAction::RequestHashset;
        }
        self.phase = PeerRequestPhase::Connected;
        PeerRequestAction::StartUpload
    }

    /// Apply a remote hashset answer and choose the next request.
    pub fn apply_hashset_answer(&mut self, piece_hashes: Vec<Ed2kHash>) -> PeerRequestAction {
        self.piece_hashes = piece_hashes;
        self.phase = PeerRequestPhase::Connected;
        self.queue_rank = None;
        PeerRequestAction::StartUpload
    }

    /// Mark the peer as queued with a remote rank.
    pub fn mark_queued(&mut self, rank: u16) {
        self.queue_rank = Some(rank);
        self.phase = PeerRequestPhase::OnQueue;
        self.requested_ranges.clear();
    }

    /// Mark that the remote accepted the upload request.
    pub fn accept_upload(&mut self) {
        self.queue_rank = None;
        self.phase = PeerRequestPhase::Downloading;
    }

    /// Replace peer-owned requested ranges.
    pub fn record_requested_ranges(&mut self, ranges: Vec<PeerRequestedRange>) {
        self.requested_ranges = ranges;
    }

    /// Apply a terminal peer failure and release peer-owned ranges.
    pub fn fail(&mut self, kind: PeerFailureKind) {
        self.requested_ranges.clear();
        self.queue_rank = None;
        self.phase = match kind {
            PeerFailureKind::NoFile => PeerRequestPhase::NoFile,
            PeerFailureKind::OutOfParts => PeerRequestPhase::OutOfParts,
            PeerFailureKind::Cancelled => PeerRequestPhase::Cancelled,
            PeerFailureKind::BadPacket | PeerFailureKind::Disconnected => PeerRequestPhase::Failed,
        };
    }
}

/// Build an ED2K file-name request frame.
pub fn build_file_name_request(
    file_hash: Ed2kHash,
    local_part_status: &[bool],
    extended_requests_version: u8,
) -> Result<PacketFrame, PeerRequestError> {
    let mut payload = file_hash.to_vec();
    if extended_requests_version > 0 {
        write_bitfield(&mut payload, local_part_status)?;
        if extended_requests_version > 1 {
            payload.extend_from_slice(&0_u16.to_le_bytes());
        }
    }
    Ok(peer_frame(PeerOpcode::RequestFileName, payload))
}

/// Build an ED2K file-status request frame.
pub fn build_file_status_request(file_hash: Ed2kHash) -> PacketFrame {
    peer_frame(PeerOpcode::SetRequestedFileId, file_hash.to_vec())
}

/// Build an ED2K hashset request frame.
pub fn build_hashset_request(file_hash: Ed2kHash) -> PacketFrame {
    peer_frame(PeerOpcode::HashsetRequest, file_hash.to_vec())
}

/// Build an ED2K upload-slot request frame.
pub fn build_start_upload_request(file_hash: Ed2kHash) -> PacketFrame {
    peer_frame(PeerOpcode::StartUploadRequest, file_hash.to_vec())
}

/// Build an ED2K file-status answer frame.
pub fn build_file_status_answer(
    file_hash: Ed2kHash,
    part_status: &[bool],
) -> Result<PacketFrame, PeerRequestError> {
    let mut payload = file_hash.to_vec();
    write_bitfield(&mut payload, part_status)?;
    Ok(peer_frame(PeerOpcode::FileStatus, payload))
}

/// Parse an ED2K file-status payload.
pub fn parse_file_status(
    payload: &[u8],
    expected_file_hash: Ed2kHash,
) -> Result<Vec<bool>, PeerRequestError> {
    let mut cursor = Cursor::new(payload);
    read_expected_hash(&mut cursor, expected_file_hash)?;
    let bit_count = cursor.read_u16().ok_or(PeerRequestError::Truncated)? as usize;
    let byte_count = bit_count.div_ceil(8);
    let raw = cursor
        .read_exact(byte_count)
        .ok_or(PeerRequestError::Truncated)?;
    if !cursor.is_done() {
        return Err(PeerRequestError::Truncated);
    }
    Ok(read_bitfield(raw, bit_count))
}

/// Build an ED2K hashset answer frame.
pub fn build_hashset_answer(
    file_hash: Ed2kHash,
    piece_hashes: &[Ed2kHash],
) -> Result<PacketFrame, PeerRequestError> {
    if piece_hashes.len() > usize::from(u16::MAX) {
        return Err(PeerRequestError::ValueTooLarge);
    }
    let mut payload = file_hash.to_vec();
    payload.extend_from_slice(
        &u16::try_from(piece_hashes.len())
            .map_err(|_| PeerRequestError::ValueTooLarge)?
            .to_le_bytes(),
    );
    for hash in piece_hashes {
        payload.extend_from_slice(hash);
    }
    Ok(peer_frame(PeerOpcode::HashsetAnswer, payload))
}

/// Parse an ED2K hashset answer payload.
pub fn parse_hashset_answer(
    payload: &[u8],
    expected_file_hash: Ed2kHash,
) -> Result<Vec<Ed2kHash>, PeerRequestError> {
    let mut cursor = Cursor::new(payload);
    read_expected_hash(&mut cursor, expected_file_hash)?;
    let count = cursor.read_u16().ok_or(PeerRequestError::Truncated)? as usize;
    let mut hashes = Vec::with_capacity(count);
    for _ in 0..count {
        hashes.push(cursor.read_hash16().ok_or(PeerRequestError::Truncated)?);
    }
    if !cursor.is_done() {
        return Err(PeerRequestError::Truncated);
    }
    Ok(hashes)
}

/// Parse an ED2K queue-rank payload.
pub fn parse_queue_rank(payload: &[u8]) -> Result<u16, PeerRequestError> {
    match payload.len() {
        2 => Ok(u16::from_le_bytes([payload[0], payload[1]])),
        4 => {
            let value = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            u16::try_from(value).map_err(|_| PeerRequestError::ValueTooLarge)
        }
        _ => Err(PeerRequestError::Truncated),
    }
}

fn peer_frame(opcode: PeerOpcode, payload: Vec<u8>) -> PacketFrame {
    PacketFrame {
        protocol: Protocol::Edonkey,
        opcode: opcode.into(),
        payload,
    }
}

fn write_bitfield(out: &mut Vec<u8>, bitfield: &[bool]) -> Result<(), PeerRequestError> {
    if bitfield.len() > usize::from(u16::MAX) {
        return Err(PeerRequestError::ValueTooLarge);
    }
    out.extend_from_slice(
        &u16::try_from(bitfield.len())
            .map_err(|_| PeerRequestError::ValueTooLarge)?
            .to_le_bytes(),
    );
    let start = out.len();
    out.resize(start + bitfield.len().div_ceil(8), 0);
    for (index, available) in bitfield.iter().enumerate() {
        if *available {
            out[start + index / 8] |= 1 << (index & 7);
        }
    }
    Ok(())
}

fn read_bitfield(raw: &[u8], bit_count: usize) -> Vec<bool> {
    (0..bit_count)
        .map(|index| raw[index / 8] & (1 << (index & 7)) != 0)
        .collect()
}

fn read_expected_hash(
    cursor: &mut Cursor<'_>,
    expected_file_hash: Ed2kHash,
) -> Result<(), PeerRequestError> {
    let file_hash = cursor.read_hash16().ok_or(PeerRequestError::Truncated)?;
    if file_hash != expected_file_hash {
        return Err(PeerRequestError::HashMismatch);
    }
    Ok(())
}

fn encode_peer_hello_payload(
    identity: &PeerIdentity,
    include_hash_size: bool,
) -> Result<Vec<u8>, PeerHandshakeError> {
    let caps = PeerCapabilities::local();
    let udp_ports = (u32::from(identity.kad_udp_port) << 16) | u32::from(identity.udp_port);
    let tags = vec![
        Tag::new(
            TagName::Id(CT_NAME),
            TagValue::String(identity.name.clone()),
        ),
        Tag::new(
            TagName::Id(CT_VERSION),
            TagValue::UInt32(EDONKEY_CLIENT_VERSION),
        ),
        Tag::new(TagName::Id(CT_EMULE_UDPPORTS), TagValue::UInt32(udp_ports)),
        Tag::new(
            TagName::Id(CT_EMULE_VERSION),
            TagValue::UInt32((u32::from(COMPATIBLE_CLIENT_RARIA) << 24) | RARIA_EMULE_VERSION),
        ),
        Tag::new(
            TagName::Id(CT_EMULE_MISCOPTIONS1),
            TagValue::UInt32(encode_misc_options1(caps)),
        ),
        Tag::new(
            TagName::Id(CT_EMULE_MISCOPTIONS2),
            TagValue::UInt32(encode_misc_options2(caps)),
        ),
        Tag::new(TagName::Id(CT_EMULECOMPAT_OPTIONS), TagValue::UInt32(0)),
    ];
    let mut payload = Vec::new();
    if include_hash_size {
        payload.push(HASH_SIZE);
    }
    payload.extend_from_slice(&identity.user_hash);
    payload.extend_from_slice(&identity.client_id.to_le_bytes());
    payload.extend_from_slice(&identity.tcp_port.to_le_bytes());
    write_tags(&mut payload, &tags)?;
    let server = identity.server.unwrap_or(PeerEndpoint { ip: 0, port: 0 });
    payload.extend_from_slice(&server.ip.to_le_bytes());
    payload.extend_from_slice(&server.port.to_le_bytes());
    Ok(payload)
}

fn decode_peer_hello_payload(
    payload: &[u8],
    has_hash_size: bool,
) -> Result<ParsedPeerHello, PeerHandshakeError> {
    let mut cursor = Cursor::new(payload);
    if has_hash_size {
        let hash_size = cursor.read_u8().ok_or(PeerHandshakeError::Truncated)?;
        if hash_size != HASH_SIZE {
            return Err(PeerHandshakeError::InvalidHashSize(hash_size));
        }
    }
    let user_hash = cursor.read_hash16().ok_or(PeerHandshakeError::Truncated)?;
    let client_id = cursor.read_u32().ok_or(PeerHandshakeError::Truncated)?;
    let tcp_port = cursor.read_u16().ok_or(PeerHandshakeError::Truncated)?;
    let tags = read_tags(&mut cursor)?;
    let server_ip = cursor.read_u32().ok_or(PeerHandshakeError::Truncated)?;
    let server_port = cursor.read_u16().ok_or(PeerHandshakeError::Truncated)?;
    if !cursor.is_done() {
        return Err(PeerHandshakeError::Truncated);
    }
    let mut identity = PeerIdentity {
        user_hash,
        client_id,
        tcp_port,
        udp_port: 0,
        kad_udp_port: 0,
        server: (server_ip != 0 && server_port != 0).then_some(PeerEndpoint {
            ip: server_ip,
            port: server_port,
        }),
        name: String::new(),
    };
    let mut caps = PeerCapabilities::default();
    for tag in tags {
        let TagName::Id(id) = tag.name else {
            continue;
        };
        match id {
            CT_NAME => {
                if let TagValue::String(value) = tag.value {
                    identity.name = value;
                }
            }
            CT_EMULE_UDPPORTS => {
                if let Some(value) = tag_u32(&tag.value) {
                    identity.udp_port = value as u16;
                    identity.kad_udp_port = (value >> 16) as u16;
                }
            }
            CT_EMULE_MISCOPTIONS1 => {
                if let Some(value) = tag_u32(&tag.value) {
                    caps = parse_misc_options1(caps, value);
                }
            }
            CT_EMULE_MISCOPTIONS2 => {
                if let Some(value) = tag_u32(&tag.value) {
                    caps = parse_misc_options2(caps, value);
                }
            }
            _ => {}
        }
    }
    Ok(ParsedPeerHello {
        identity,
        capabilities: caps,
    })
}

fn write_tags(out: &mut Vec<u8>, tags: &[Tag]) -> Result<(), PeerHandshakeError> {
    out.extend_from_slice(
        &u32::try_from(tags.len())
            .map_err(|_| PeerHandshakeError::ValueTooLarge)?
            .to_le_bytes(),
    );
    for tag in tags {
        out.extend_from_slice(&encode_tag(tag)?);
    }
    Ok(())
}

fn read_tags(cursor: &mut Cursor<'_>) -> Result<Vec<Tag>, PeerHandshakeError> {
    let count = cursor.read_u32().ok_or(PeerHandshakeError::Truncated)?;
    let count = usize::try_from(count).map_err(|_| PeerHandshakeError::ValueTooLarge)?;
    let mut tags = Vec::with_capacity(count.min(64));
    for _ in 0..count {
        let (tag, consumed) = decode_tag_prefix(cursor.remaining_bytes())?;
        tags.push(tag);
        cursor
            .read_exact(consumed)
            .ok_or(PeerHandshakeError::Truncated)?;
    }
    Ok(tags)
}

fn tag_u32(value: &TagValue) -> Option<u32> {
    match value {
        TagValue::UInt8(value) => Some(u32::from(*value)),
        TagValue::UInt16(value) => Some(u32::from(*value)),
        TagValue::UInt32(value) => Some(*value),
        TagValue::UInt64(value) => u32::try_from(*value).ok(),
        _ => None,
    }
}

fn encode_misc_options1(caps: PeerCapabilities) -> u32 {
    ((u32::from(caps.aich_version) & 0x07) << 29)
        | (u32::from(caps.unicode) << 28)
        | ((u32::from(caps.udp_version) & 0x0f) << 24)
        | ((u32::from(caps.data_compression_version) & 0x0f) << 20)
        | ((u32::from(caps.secure_ident_version) & 0x0f) << 16)
        | ((u32::from(caps.source_exchange1_version) & 0x0f) << 12)
        | ((u32::from(caps.extended_requests_version) & 0x0f) << 8)
        | (u32::from(caps.accepts_comments) << 4)
        | (u32::from(caps.supports_multipacket) << 1)
        | u32::from(caps.supports_preview)
}

fn parse_misc_options1(mut caps: PeerCapabilities, value: u32) -> PeerCapabilities {
    caps.aich_version = ((value >> 29) & 0x07) as u8;
    caps.unicode = value & (1 << 28) != 0;
    caps.udp_version = ((value >> 24) & 0x0f) as u8;
    caps.data_compression_version = ((value >> 20) & 0x0f) as u8;
    caps.secure_ident_version = ((value >> 16) & 0x0f) as u8;
    caps.source_exchange1_version = ((value >> 12) & 0x0f) as u8;
    caps.extended_requests_version = ((value >> 8) & 0x0f) as u8;
    caps.accepts_comments = ((value >> 4) & 0x0f) != 0;
    caps.supports_multipacket = value & (1 << 1) != 0;
    caps.supports_preview = value & 1 != 0;
    caps
}

fn encode_misc_options2(caps: PeerCapabilities) -> u32 {
    (u32::from(caps.supports_direct_udp_callback) << 12)
        | (u32::from(caps.supports_captcha) << 11)
        | (u32::from(caps.supports_source_exchange2) << 10)
        | (u32::from(caps.requires_crypt_layer) << 9)
        | (u32::from(caps.requests_crypt_layer) << 8)
        | (u32::from(caps.supports_crypt_layer) << 7)
        | (u32::from(caps.supports_extended_multipacket) << 5)
        | (u32::from(caps.supports_large_files) << 4)
        | (u32::from(caps.kad_version) & 0x0f)
}

fn parse_misc_options2(mut caps: PeerCapabilities, value: u32) -> PeerCapabilities {
    caps.supports_direct_udp_callback = value & (1 << 12) != 0;
    caps.supports_captcha = value & (1 << 11) != 0;
    caps.supports_source_exchange2 = value & (1 << 10) != 0;
    caps.requires_crypt_layer = value & (1 << 9) != 0;
    caps.requests_crypt_layer = value & (1 << 8) != 0;
    caps.supports_crypt_layer = value & (1 << 7) != 0;
    caps.supports_extended_multipacket = value & (1 << 5) != 0;
    caps.supports_large_files = value & (1 << 4) != 0;
    caps.kad_version = (value & 0x0f) as u8;
    caps.requests_crypt_layer &= caps.supports_crypt_layer;
    caps.requires_crypt_layer &= caps.requests_crypt_layer;
    caps
}

fn clamp_nibble(value: u32) -> u8 {
    (value & 0x0f) as u8
}

/// Endpoint accepted through a server-mediated LowID callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackEndpoint {
    /// ED2K client ID associated with the callback.
    pub client_id: u32,
    /// Callback IP as the ED2K wire integer.
    pub ip: u32,
    /// Callback TCP port.
    pub tcp_port: u16,
    /// Optional crypt option bits reported by the server.
    pub crypt_options: Option<u8>,
    /// Optional peer user hash reported by the server.
    pub user_hash: Option<[u8; 16]>,
}

impl CallbackEndpoint {
    /// Parse a server-mediated callback endpoint payload.
    pub fn parse_server_payload(
        client_id: u32,
        payload: &[u8],
    ) -> Result<Self, CallbackParseError> {
        if payload.len() != 6 && payload.len() < 23 {
            return Err(CallbackParseError::InvalidPayload);
        }
        let ip = u32::from_le_bytes(
            payload[0..4]
                .try_into()
                .map_err(|_| CallbackParseError::InvalidPayload)?,
        );
        let tcp_port = u16::from_le_bytes(
            payload[4..6]
                .try_into()
                .map_err(|_| CallbackParseError::InvalidPayload)?,
        );
        let (crypt_options, user_hash) = if payload.len() >= 23 {
            let mut hash = [0_u8; 16];
            hash.copy_from_slice(&payload[7..23]);
            (Some(payload[6]), Some(hash))
        } else {
            (None, None)
        };
        Ok(Self {
            client_id,
            ip,
            tcp_port,
            crypt_options,
            user_hash,
        })
    }
}

/// Server callback endpoint parse error.
#[derive(Debug, Copy, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallbackParseError {
    /// Payload is malformed or truncated.
    #[error("invalid ED2K callback payload")]
    InvalidPayload,
}

/// LowID callback state.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LowIdCallbackState {
    /// Callback is not required for this peer.
    NotNeeded,
    /// LowID peer has not requested a callback yet.
    Needed,
    /// Callback request was sent to the server.
    Requested {
        /// Request timestamp in caller-owned monotonic seconds.
        requested_at: u64,
    },
    /// Server accepted the callback and returned a reachable endpoint.
    Accepted {
        /// Acceptance timestamp in caller-owned monotonic seconds.
        accepted_at: u64,
    },
    /// Server reported callback failure.
    Failed {
        /// Failure timestamp in caller-owned monotonic seconds.
        failed_at: u64,
    },
    /// Callback wait expired.
    TimedOut {
        /// Timeout timestamp in caller-owned monotonic seconds.
        timed_out_at: u64,
    },
    /// Peer cannot be reached through supported callback paths.
    Impossible,
    /// Callback path completed and no longer blocks scheduling.
    Completed {
        /// Completion timestamp in caller-owned monotonic seconds.
        completed_at: u64,
    },
}

/// Native peer scheduling state for reachability decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerSchedulingState {
    /// ED2K client ID or LowID.
    pub client_id: u32,
    /// Last known TCP port.
    pub tcp_port: u16,
    /// Whether the peer has a LowID-shaped ID.
    pub low_id: bool,
    /// LowID callback state.
    pub callback_state: LowIdCallbackState,
    /// Last callback endpoint accepted by the server.
    pub callback_endpoint: Option<CallbackEndpoint>,
}

impl PeerSchedulingState {
    /// Create scheduling state from a server or peer source record.
    pub fn from_source(client_id: u32, tcp_port: u16) -> Self {
        let low_id = is_low_id(client_id);
        Self {
            client_id,
            tcp_port,
            low_id,
            callback_state: if low_id {
                LowIdCallbackState::Needed
            } else {
                LowIdCallbackState::NotNeeded
            },
            callback_endpoint: None,
        }
    }

    /// Return whether the peer can be scheduled for a direct TCP connection.
    pub fn can_connect_directly(&self, _now_seconds: u64) -> bool {
        if !self.low_id {
            return true;
        }
        matches!(
            self.callback_state,
            LowIdCallbackState::Accepted { .. } | LowIdCallbackState::Completed { .. }
        ) && self.callback_endpoint.is_some()
    }

    /// Build and mark a server-mediated callback request for a LowID peer.
    pub fn request_server_callback(&mut self, now_seconds: u64) -> Option<PacketFrame> {
        if !self.low_id || matches!(self.callback_state, LowIdCallbackState::Requested { .. }) {
            return None;
        }
        self.callback_state = LowIdCallbackState::Requested {
            requested_at: now_seconds,
        };
        Some(PacketFrame {
            protocol: Protocol::Edonkey,
            opcode: ServerOpcode::CallbackRequest.into(),
            payload: self.client_id.to_le_bytes().to_vec(),
        })
    }

    /// Accept a server-mediated callback endpoint.
    pub fn accept_server_callback(&mut self, endpoint: CallbackEndpoint, now_seconds: u64) {
        self.callback_endpoint = Some(endpoint);
        self.callback_state = LowIdCallbackState::Accepted {
            accepted_at: now_seconds,
        };
    }

    /// Mark server-mediated callback failure.
    pub fn fail_server_callback(&mut self, now_seconds: u64) {
        self.callback_endpoint = None;
        self.callback_state = LowIdCallbackState::Failed {
            failed_at: now_seconds,
        };
    }

    /// Expire a pending callback request when the wait exceeds `timeout_seconds`.
    pub fn expire_callback(&mut self, now_seconds: u64, timeout_seconds: u64) -> bool {
        let LowIdCallbackState::Requested { requested_at } = self.callback_state else {
            return false;
        };
        if now_seconds.saturating_sub(requested_at) <= timeout_seconds {
            return false;
        }
        self.callback_endpoint = None;
        self.callback_state = LowIdCallbackState::TimedOut {
            timed_out_at: now_seconds,
        };
        true
    }

    /// Mark the callback path completed.
    pub fn complete_callback(&mut self, now_seconds: u64) {
        self.callback_state = LowIdCallbackState::Completed {
            completed_at: now_seconds,
        };
    }
}

/// Server-mediated callback capability truth.
pub struct ServerMediatedCallback;

impl ServerMediatedCallback {
    /// Return whether direct UDP callback is implemented and advertised.
    pub fn supports_direct_udp_callback() -> bool {
        false
    }

    /// Return whether Kad buddy callback is implemented and advertised.
    pub fn supports_kad_buddy_callback() -> bool {
        false
    }

    /// Return whether required-crypt callback is implemented and advertised.
    pub fn supports_required_crypt_callback() -> bool {
        false
    }
}
