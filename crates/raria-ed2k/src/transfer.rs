//! ED2K part planning, compressed payload, retry, and resume ownership.

use crate::hash::{AICH_EMBLOCK_SIZE, ED2K_PART_SIZE, Ed2kHash};
use crate::opcode::PeerOpcode;
use crate::packet::{PacketFrame, Protocol};
use crate::wire::Cursor;

const REQUEST_PART_WIRE_RANGES: usize = 3;

/// ED2K part byte range.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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
