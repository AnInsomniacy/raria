//! ED2K part planning, compressed payload, retry, and resume ownership.

use crate::hash::{AICH_EMBLOCK_SIZE, ED2K_PART_SIZE, Ed2kHash};
use crate::opcode::PeerOpcode;
use crate::packet::{PacketFrame, Protocol};
use crate::wire::Cursor;
use flate2::{Decompress, FlushDecompress, Status};
use serde::{Deserialize, Serialize};

const REQUEST_PART_WIRE_RANGES: usize = 3;
const DEFAULT_TRANSFER_RETRY_DELAY_SECONDS: u64 = 5;

/// ED2K part byte range.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartRange {
    /// Inclusive range start offset.
    pub begin: u64,
    /// Exclusive range end offset.
    pub end: u64,
}

/// Input for native ED2K part request planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartPlanInput {
    /// Total file size in bytes.
    pub file_size: u64,
    /// Locally completed byte ranges.
    pub completed_ranges: Vec<PartRange>,
    /// Globally requested byte ranges owned by other peers.
    pub globally_requested: Vec<PartRange>,
    /// Ranges already requested from this peer.
    pub peer_requested: Vec<PartRange>,
    /// Optional remote ED2K part availability bitfield.
    pub remote_part_status: Vec<bool>,
    /// Maximum new ranges to return.
    pub max_new_ranges: usize,
}

/// Transfer planning and request-parts codec error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransferPlanError {
    /// Range is empty, inverted, or outside the known file size.
    #[error("invalid ED2K part range")]
    InvalidRange,
    /// More ranges were provided than the retained wire format supports.
    #[error("too many ED2K part ranges")]
    TooManyRanges,
    /// Offset cannot be represented by the selected wire format.
    #[error("ED2K part offset is too large for selected request format")]
    OffsetTooLarge,
    /// Payload is malformed or truncated.
    #[error("truncated ED2K part request payload")]
    Truncated,
    /// Payload file hash does not match the expected file.
    #[error("ED2K part request hash mismatch")]
    HashMismatch,
}

/// Decoded ED2K part payload kind.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PartPayloadKind {
    /// Plain OP_SENDINGPART or OP_SENDINGPART_I64 bytes.
    Normal,
    /// Zlib compressed OP_COMPRESSEDPART or OP_COMPRESSEDPART_I64 bytes.
    Compressed,
}

/// Decoded and verified ED2K part payload ready for the disk owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedPart {
    /// Decoded native byte range.
    pub range: PartRange,
    /// Decoded payload bytes for the range.
    pub data: Vec<u8>,
    /// Source wire payload kind.
    pub kind: PartPayloadKind,
}

/// ED2K part payload validation error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransferPartError {
    /// Packet opcode is not a retained part-payload opcode.
    #[error("unexpected ED2K part payload opcode")]
    UnexpectedOpcode,
    /// Packet protocol marker does not match the retained opcode form.
    #[error("invalid ED2K part payload protocol")]
    InvalidProtocol,
    /// Payload is malformed or truncated.
    #[error("truncated ED2K part payload")]
    Truncated,
    /// Payload file hash does not match the expected file.
    #[error("ED2K part payload hash mismatch")]
    HashMismatch,
    /// Payload range is empty, inverted, or outside the known file size.
    #[error("invalid ED2K part payload range")]
    InvalidRange,
    /// Payload range is not owned by the peer request state.
    #[error("ED2K part payload range is not owned")]
    RangeNotOwned,
    /// Payload byte length does not match declared transfer metadata.
    #[error("ED2K part payload length mismatch")]
    PayloadLengthMismatch,
    /// Decoded part payload exceeds the caller-owned safety limit.
    #[error("ED2K part payload too large")]
    PayloadTooLarge,
    /// Compressed payload could not be inflated safely.
    #[error("invalid ED2K compressed part payload")]
    InvalidCompression,
}

/// Stateful inflater for ED2K compressed part chunks.
#[derive(Default)]
pub struct CompressedPartInflater {
    decoder: Option<Decompress>,
    block_begin: u64,
    inflated_len: u64,
    total_compressed_len: u32,
    consumed_compressed_len: u32,
}

impl CompressedPartInflater {
    /// Return whether a compressed stream is active across chunks.
    pub fn is_active(&self) -> bool {
        self.decoder.is_some()
    }

    /// Reset the active compressed stream.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// ED2K peer transfer status.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TransferStatus {
    /// No active part request is in flight.
    Idle,
    /// One or more requested ranges are in flight.
    Downloading,
    /// Remote peer cancelled or returned us to queue.
    OnQueue,
    /// Remote peer has no useful parts for this task.
    NoNeededParts,
    /// Peer is temporarily delayed after a retriable failure.
    BackingOff {
        /// Retry time in caller-owned monotonic seconds.
        retry_at: u64,
    },
    /// Peer failed without an immediate retry path.
    Failed,
}

/// ED2K transfer failure classification.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TransferFailureKind {
    /// Remote peer sent OP_CANCELTRANSFER.
    RemoteCancel,
    /// Remote peer sent OP_OUTOFPARTREQS.
    OutOfParts,
    /// Remote peer reported no file.
    NoFile,
    /// Packet failed structural validation.
    BadPacket,
    /// Decoded block failed integrity or ownership validation.
    CorruptBlock,
    /// Requested ranges stalled beyond the timeout.
    Timeout,
    /// Peer disconnected while ranges were in flight.
    Disconnected,
    /// Peer has no needed parts.
    NoNeededParts,
}

/// Peer-owned ED2K transfer state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerTransferState {
    /// Current transfer status.
    pub status: TransferStatus,
    /// Ranges currently owned by this peer.
    pub requested_ranges: Vec<PartRange>,
    /// Last transfer activity timestamp in caller-owned monotonic seconds.
    pub last_activity_seconds: u64,
}

impl PeerTransferState {
    /// Create an idle peer transfer state.
    pub fn new(now_seconds: u64) -> Self {
        Self {
            status: TransferStatus::Idle,
            requested_ranges: Vec::new(),
            last_activity_seconds: now_seconds,
        }
    }

    /// Replace peer-owned requested ranges and mark the peer downloading.
    pub fn record_requested_ranges(&mut self, ranges: Vec<PartRange>, now_seconds: u64) {
        self.requested_ranges = ranges;
        self.status = TransferStatus::Downloading;
        self.last_activity_seconds = now_seconds;
    }

    /// Expire stalled requests, release ranges, and enter bounded retry.
    pub fn expire_stalled(&mut self, now_seconds: u64, timeout_seconds: u64) -> bool {
        if !matches!(self.status, TransferStatus::Downloading)
            || now_seconds.saturating_sub(self.last_activity_seconds) <= timeout_seconds
        {
            return false;
        }
        self.apply_failure(TransferFailureKind::Timeout, now_seconds);
        true
    }

    /// Apply a transfer failure and release peer-owned ranges.
    pub fn apply_failure(&mut self, kind: TransferFailureKind, now_seconds: u64) {
        self.requested_ranges.clear();
        self.last_activity_seconds = now_seconds;
        self.status = match kind {
            TransferFailureKind::RemoteCancel | TransferFailureKind::OutOfParts => {
                TransferStatus::OnQueue
            }
            TransferFailureKind::NoNeededParts => TransferStatus::NoNeededParts,
            TransferFailureKind::NoFile => TransferStatus::Failed,
            TransferFailureKind::BadPacket
            | TransferFailureKind::CorruptBlock
            | TransferFailureKind::Timeout
            | TransferFailureKind::Disconnected => TransferStatus::BackingOff {
                retry_at: now_seconds + DEFAULT_TRANSFER_RETRY_DELAY_SECONDS,
            },
        };
    }
}

/// Return whether two byte ranges overlap.
pub fn ranges_overlap(lhs: PartRange, rhs: PartRange) -> bool {
    lhs.begin < rhs.end && rhs.begin < lhs.end
}

/// Plan ED2K block ranges for one peer.
pub fn plan_part_requests(input: &PartPlanInput) -> Result<Vec<PartRange>, TransferPlanError> {
    if input.file_size == 0 {
        return Ok(Vec::new());
    }
    let max_new = input.max_new_ranges.min(REQUEST_PART_WIRE_RANGES);
    if max_new == 0 {
        return Ok(Vec::new());
    }
    validate_ranges(input.file_size, &input.completed_ranges)?;
    validate_ranges(input.file_size, &input.globally_requested)?;
    validate_ranges(input.file_size, &input.peer_requested)?;

    let mut planned = Vec::new();
    let mut part_index = 0_u64;
    let mut part_begin = 0_u64;
    while planned.len() < max_new && part_begin < input.file_size {
        let part_end = (part_begin + ED2K_PART_SIZE).min(input.file_size);
        if input
            .remote_part_status
            .get(part_index as usize)
            .is_none_or(|available| *available)
        {
            plan_part_blocks(input, part_begin, part_end, &mut planned, max_new);
        }
        part_index += 1;
        part_begin = part_end;
    }
    Ok(planned)
}

/// Build an ED2K request-parts frame.
pub fn build_part_request(
    file_hash: Ed2kHash,
    ranges: &[PartRange],
    use_i64_offsets: bool,
) -> Result<PacketFrame, TransferPlanError> {
    if ranges.len() > REQUEST_PART_WIRE_RANGES {
        return Err(TransferPlanError::TooManyRanges);
    }
    for range in ranges {
        validate_range(*range, u64::MAX)?;
        if !use_i64_offsets
            && (range.begin > u64::from(u32::MAX) || range.end > u64::from(u32::MAX))
        {
            return Err(TransferPlanError::OffsetTooLarge);
        }
    }
    let mut payload = file_hash.to_vec();
    if use_i64_offsets {
        for index in 0..REQUEST_PART_WIRE_RANGES {
            payload.extend_from_slice(
                &ranges
                    .get(index)
                    .map_or(0, |range| range.begin)
                    .to_le_bytes(),
            );
        }
        for index in 0..REQUEST_PART_WIRE_RANGES {
            payload
                .extend_from_slice(&ranges.get(index).map_or(0, |range| range.end).to_le_bytes());
        }
    } else {
        for index in 0..REQUEST_PART_WIRE_RANGES {
            let value = ranges.get(index).map_or(0, |range| range.begin);
            payload.extend_from_slice(&(value as u32).to_le_bytes());
        }
        for index in 0..REQUEST_PART_WIRE_RANGES {
            let value = ranges.get(index).map_or(0, |range| range.end);
            payload.extend_from_slice(&(value as u32).to_le_bytes());
        }
    }
    Ok(PacketFrame {
        protocol: if use_i64_offsets {
            Protocol::Emule
        } else {
            Protocol::Edonkey
        },
        opcode: if use_i64_offsets {
            PeerOpcode::RequestPartsI64.into()
        } else {
            PeerOpcode::RequestParts.into()
        },
        payload,
    })
}

/// Parse an ED2K request-parts payload.
pub fn parse_part_request(
    payload: &[u8],
    expected_file_hash: Ed2kHash,
    use_i64_offsets: bool,
) -> Result<Vec<PartRange>, TransferPlanError> {
    let mut cursor = Cursor::new(payload);
    let hash = cursor.read_hash16().ok_or(TransferPlanError::Truncated)?;
    if hash != expected_file_hash {
        return Err(TransferPlanError::HashMismatch);
    }
    let expected_len = 16 + REQUEST_PART_WIRE_RANGES * if use_i64_offsets { 16 } else { 8 };
    if payload.len() != expected_len {
        return Err(TransferPlanError::Truncated);
    }
    let mut begins = [0_u64; REQUEST_PART_WIRE_RANGES];
    let mut ends = [0_u64; REQUEST_PART_WIRE_RANGES];
    for begin in &mut begins {
        *begin = read_offset(&mut cursor, use_i64_offsets)?;
    }
    for end in &mut ends {
        *end = read_offset(&mut cursor, use_i64_offsets)?;
    }
    let mut ranges = Vec::new();
    for index in 0..REQUEST_PART_WIRE_RANGES {
        if ends[index] <= begins[index] {
            continue;
        }
        ranges.push(PartRange {
            begin: begins[index],
            end: ends[index],
        });
    }
    Ok(ranges)
}

/// Build an ED2K transfer-cancel frame.
pub fn build_cancel_transfer() -> PacketFrame {
    PacketFrame {
        protocol: Protocol::Edonkey,
        opcode: PeerOpcode::CancelTransfer.into(),
        payload: Vec::new(),
    }
}

/// Parse and validate a normal or compressed ED2K part payload.
pub fn parse_part_payload(
    frame: &PacketFrame,
    expected_file_hash: Ed2kHash,
    owned_ranges: &[PartRange],
    file_size: u64,
    inflater: &mut CompressedPartInflater,
    max_output: usize,
) -> Result<ReceivedPart, TransferPartError> {
    match PeerOpcode::from_byte(frame.opcode).ok_or(TransferPartError::UnexpectedOpcode)? {
        PeerOpcode::SendingPart => {
            if frame.protocol != Protocol::Edonkey {
                return Err(TransferPartError::InvalidProtocol);
            }
            parse_normal_part_payload(
                &frame.payload,
                expected_file_hash,
                false,
                owned_ranges,
                file_size,
            )
        }
        PeerOpcode::SendingPartI64 => {
            if frame.protocol != Protocol::Emule {
                return Err(TransferPartError::InvalidProtocol);
            }
            parse_normal_part_payload(
                &frame.payload,
                expected_file_hash,
                true,
                owned_ranges,
                file_size,
            )
        }
        PeerOpcode::CompressedPart => {
            if frame.protocol != Protocol::Emule {
                return Err(TransferPartError::InvalidProtocol);
            }
            parse_compressed_part_payload(
                &frame.payload,
                expected_file_hash,
                false,
                owned_ranges,
                file_size,
                inflater,
                max_output,
            )
        }
        PeerOpcode::CompressedPartI64 => {
            if frame.protocol != Protocol::Emule {
                return Err(TransferPartError::InvalidProtocol);
            }
            parse_compressed_part_payload(
                &frame.payload,
                expected_file_hash,
                true,
                owned_ranges,
                file_size,
                inflater,
                max_output,
            )
        }
        _ => Err(TransferPartError::UnexpectedOpcode),
    }
}

fn plan_part_blocks(
    input: &PartPlanInput,
    part_begin: u64,
    part_end: u64,
    planned: &mut Vec<PartRange>,
    max_new: usize,
) {
    let mut begin = part_begin;
    while planned.len() < max_new && begin < part_end {
        let end = (begin + AICH_EMBLOCK_SIZE).min(part_end);
        let range = PartRange { begin, end };
        if range_available(input, planned, range) {
            planned.push(range);
        }
        begin = end;
    }
}

fn range_available(input: &PartPlanInput, planned: &[PartRange], range: PartRange) -> bool {
    !input
        .completed_ranges
        .iter()
        .chain(input.globally_requested.iter())
        .chain(input.peer_requested.iter())
        .chain(planned.iter())
        .any(|existing| ranges_overlap(*existing, range))
}

fn validate_ranges(file_size: u64, ranges: &[PartRange]) -> Result<(), TransferPlanError> {
    for range in ranges {
        validate_range(*range, file_size)?;
    }
    Ok(())
}

fn validate_range(range: PartRange, file_size: u64) -> Result<(), TransferPlanError> {
    if range.end <= range.begin || range.end > file_size {
        return Err(TransferPlanError::InvalidRange);
    }
    Ok(())
}

fn read_offset(cursor: &mut Cursor<'_>, use_i64_offsets: bool) -> Result<u64, TransferPlanError> {
    if use_i64_offsets {
        cursor.read_u64().ok_or(TransferPlanError::Truncated)
    } else {
        cursor
            .read_u32()
            .map(u64::from)
            .ok_or(TransferPlanError::Truncated)
    }
}

fn parse_normal_part_payload(
    payload: &[u8],
    expected_file_hash: Ed2kHash,
    use_i64_offsets: bool,
    owned_ranges: &[PartRange],
    file_size: u64,
) -> Result<ReceivedPart, TransferPartError> {
    let mut cursor = Cursor::new(payload);
    read_part_hash(&mut cursor, expected_file_hash)?;
    let begin = read_part_offset(&mut cursor, use_i64_offsets)?;
    let end = read_part_offset(&mut cursor, use_i64_offsets)?;
    let range = PartRange { begin, end };
    validate_part_range(range, file_size)?;
    ensure_owned(owned_ranges, range)?;
    let data = cursor.remaining_bytes();
    if u64::try_from(data.len()).map_err(|_| TransferPartError::PayloadTooLarge)? != end - begin {
        return Err(TransferPartError::PayloadLengthMismatch);
    }
    Ok(ReceivedPart {
        range,
        data: data.to_vec(),
        kind: PartPayloadKind::Normal,
    })
}

fn parse_compressed_part_payload(
    payload: &[u8],
    expected_file_hash: Ed2kHash,
    use_i64_offsets: bool,
    owned_ranges: &[PartRange],
    file_size: u64,
    inflater: &mut CompressedPartInflater,
    max_output: usize,
) -> Result<ReceivedPart, TransferPartError> {
    let mut cursor = Cursor::new(payload);
    read_part_hash(&mut cursor, expected_file_hash)?;
    let block_begin = read_part_offset(&mut cursor, use_i64_offsets)?;
    let total_compressed_len = cursor.read_u32().ok_or(TransferPartError::Truncated)?;
    let compressed = cursor.remaining_bytes();
    if total_compressed_len == 0
        || compressed.is_empty()
        || compressed.len() > total_compressed_len as usize
    {
        return Err(TransferPartError::PayloadLengthMismatch);
    }
    let block_probe_end = block_begin
        .checked_add(1)
        .ok_or(TransferPartError::InvalidRange)?;
    validate_part_range(
        PartRange {
            begin: block_begin,
            end: block_probe_end,
        },
        file_size,
    )?;
    inflate_compressed_chunk(
        inflater,
        block_begin,
        total_compressed_len,
        compressed,
        owned_ranges,
        file_size,
        max_output,
    )
}

fn inflate_compressed_chunk(
    inflater: &mut CompressedPartInflater,
    block_begin: u64,
    total_compressed_len: u32,
    compressed: &[u8],
    owned_ranges: &[PartRange],
    file_size: u64,
    max_output: usize,
) -> Result<ReceivedPart, TransferPartError> {
    if inflater
        .decoder
        .as_ref()
        .is_some_and(|_| inflater.block_begin != block_begin)
    {
        inflater.reset();
    }
    if inflater.decoder.is_none() {
        inflater.decoder = Some(Decompress::new(true));
        inflater.block_begin = block_begin;
        inflater.inflated_len = 0;
        inflater.total_compressed_len = total_compressed_len;
        inflater.consumed_compressed_len = 0;
    }
    if inflater.total_compressed_len != total_compressed_len {
        inflater.reset();
        return Err(TransferPartError::PayloadLengthMismatch);
    }
    let compressed_len =
        u32::try_from(compressed.len()).map_err(|_| TransferPartError::PayloadTooLarge)?;
    if inflater
        .consumed_compressed_len
        .checked_add(compressed_len)
        .is_none_or(|value| value > total_compressed_len)
    {
        inflater.reset();
        return Err(TransferPartError::PayloadLengthMismatch);
    }

    let begin = block_begin
        .checked_add(inflater.inflated_len)
        .ok_or(TransferPartError::InvalidRange)?;
    let owner = owner_for_begin(owned_ranges, begin).ok_or(TransferPartError::RangeNotOwned)?;
    let remaining_owned = owner.end.saturating_sub(begin);
    let output_limit = usize::try_from(remaining_owned)
        .unwrap_or(usize::MAX)
        .min(max_output);
    if output_limit == 0 {
        inflater.reset();
        return Err(TransferPartError::RangeNotOwned);
    }

    let decoder = inflater
        .decoder
        .as_mut()
        .ok_or(TransferPartError::InvalidCompression)?;
    let before_in = decoder.total_in();
    let before_out = decoder.total_out();
    let mut output = vec![0_u8; output_limit.saturating_add(1)];
    let status = decoder
        .decompress(compressed, &mut output, FlushDecompress::Sync)
        .map_err(|_| TransferPartError::InvalidCompression)?;
    let consumed = decoder.total_in() - before_in;
    let produced = decoder.total_out() - before_out;
    if consumed != compressed.len() as u64 {
        inflater.reset();
        return Err(TransferPartError::InvalidCompression);
    }
    if produced > output_limit as u64 {
        inflater.reset();
        return Err(TransferPartError::PayloadTooLarge);
    }
    inflater.consumed_compressed_len += compressed_len;
    inflater.inflated_len = inflater
        .inflated_len
        .checked_add(produced)
        .ok_or(TransferPartError::InvalidRange)?;

    let end = begin
        .checked_add(produced)
        .ok_or(TransferPartError::InvalidRange)?;
    validate_part_range(
        PartRange {
            begin: block_begin,
            end: end.max(
                block_begin
                    .checked_add(1)
                    .ok_or(TransferPartError::InvalidRange)?,
            ),
        },
        file_size,
    )?;
    let complete = status == Status::StreamEnd;
    if complete && inflater.consumed_compressed_len != total_compressed_len {
        inflater.reset();
        return Err(TransferPartError::PayloadLengthMismatch);
    }
    if !complete && inflater.consumed_compressed_len == total_compressed_len {
        inflater.reset();
        return Err(TransferPartError::InvalidCompression);
    }
    output.truncate(produced as usize);
    if complete {
        inflater.reset();
    }
    Ok(ReceivedPart {
        range: PartRange { begin, end },
        data: output,
        kind: PartPayloadKind::Compressed,
    })
}

fn read_part_hash(
    cursor: &mut Cursor<'_>,
    expected_file_hash: Ed2kHash,
) -> Result<(), TransferPartError> {
    let hash = cursor.read_hash16().ok_or(TransferPartError::Truncated)?;
    if hash != expected_file_hash {
        return Err(TransferPartError::HashMismatch);
    }
    Ok(())
}

fn read_part_offset(
    cursor: &mut Cursor<'_>,
    use_i64_offsets: bool,
) -> Result<u64, TransferPartError> {
    if use_i64_offsets {
        cursor.read_u64().ok_or(TransferPartError::Truncated)
    } else {
        cursor
            .read_u32()
            .map(u64::from)
            .ok_or(TransferPartError::Truncated)
    }
}

fn validate_part_range(range: PartRange, file_size: u64) -> Result<(), TransferPartError> {
    if range.end <= range.begin || range.end > file_size {
        return Err(TransferPartError::InvalidRange);
    }
    Ok(())
}

fn ensure_owned(owned_ranges: &[PartRange], range: PartRange) -> Result<(), TransferPartError> {
    if owned_ranges
        .iter()
        .any(|owned| owned.begin <= range.begin && owned.end >= range.end)
    {
        return Ok(());
    }
    Err(TransferPartError::RangeNotOwned)
}

fn owner_for_begin(owned_ranges: &[PartRange], begin: u64) -> Option<PartRange> {
    owned_ranges
        .iter()
        .copied()
        .find(|owned| owned.begin <= begin && owned.end > begin)
}
