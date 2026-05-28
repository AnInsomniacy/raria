use flate2::Compression;
use flate2::write::ZlibEncoder;
use raria_ed2k::hash::Ed2kHash;
use raria_ed2k::opcode::PeerOpcode;
use raria_ed2k::packet::{PacketFrame, Protocol};
use raria_ed2k::transfer::{
    CompressedPartInflater, PartPayloadKind, PartRange, PeerTransferState, TransferFailureKind,
    TransferPartError, TransferStatus, build_cancel_transfer, parse_part_payload,
};
use std::io::Write;

#[test]
fn normal_part_payload_validates_hash_range_length_and_ownership() {
    let hash = [0x11; 16];
    let frame = normal_part_frame(hash, 10, 14, b"data", false);
    let mut inflater = CompressedPartInflater::default();

    let part = parse_part_payload(
        &frame,
        hash,
        &[PartRange { begin: 10, end: 14 }],
        100,
        &mut inflater,
        32,
    )
    .expect("valid normal part");

    assert_eq!(part.kind, PartPayloadKind::Normal);
    assert_eq!(part.range, PartRange { begin: 10, end: 14 });
    assert_eq!(part.data, b"data");

    assert_eq!(
        parse_part_payload(
            &frame,
            [0x22; 16],
            &[PartRange { begin: 10, end: 14 }],
            100,
            &mut inflater,
            32,
        ),
        Err(TransferPartError::HashMismatch)
    );
    assert_eq!(
        parse_part_payload(
            &frame,
            hash,
            &[PartRange { begin: 20, end: 24 }],
            100,
            &mut inflater,
            32,
        ),
        Err(TransferPartError::RangeNotOwned)
    );

    let bad_length = normal_part_frame(hash, 10, 15, b"data", false);
    assert_eq!(
        parse_part_payload(
            &bad_length,
            hash,
            &[PartRange { begin: 10, end: 15 }],
            100,
            &mut inflater,
            32,
        ),
        Err(TransferPartError::PayloadLengthMismatch)
    );
}

#[test]
fn i64_normal_part_payload_preserves_large_offsets() {
    let hash = [0x33; 16];
    let begin = u64::from(u32::MAX) + 9;
    let end = begin + 5;
    let frame = normal_part_frame(hash, begin, end, b"large", true);
    let mut inflater = CompressedPartInflater::default();

    let part = parse_part_payload(
        &frame,
        hash,
        &[PartRange { begin, end }],
        end + 10,
        &mut inflater,
        16,
    )
    .expect("valid i64 part");

    assert_eq!(part.kind, PartPayloadKind::Normal);
    assert_eq!(part.range, PartRange { begin, end });
    assert_eq!(part.data, b"large");
}

#[test]
fn compressed_part_payload_inflates_chunks_with_owned_offsets() {
    let hash = [0x44; 16];
    let plain: Vec<u8> = (0..220_000).map(|index| b'A' + (index % 5) as u8).collect();
    let compressed = zlib_compress(&plain);
    let split = compressed.len() / 2;
    let mut inflater = CompressedPartInflater::default();

    let first = parse_part_payload(
        &compressed_part_frame(
            hash,
            0,
            compressed.len() as u32,
            &compressed[..split],
            false,
        ),
        hash,
        &[PartRange {
            begin: 0,
            end: plain.len() as u64,
        }],
        plain.len() as u64,
        &mut inflater,
        plain.len(),
    )
    .expect("first compressed chunk");
    assert_eq!(first.kind, PartPayloadKind::Compressed);
    assert_eq!(first.range.begin, 0);
    assert!(first.range.end > 0);
    assert_eq!(first.data, plain[..first.data.len()]);
    assert!(inflater.is_active());

    let second = parse_part_payload(
        &compressed_part_frame(
            hash,
            0,
            compressed.len() as u32,
            &compressed[split..],
            false,
        ),
        hash,
        &[PartRange {
            begin: 0,
            end: plain.len() as u64,
        }],
        plain.len() as u64,
        &mut inflater,
        plain.len(),
    )
    .expect("second compressed chunk");
    assert_eq!(second.range.begin, first.range.end);
    assert_eq!(
        [first.data, second.data].concat(),
        plain,
        "inflated chunks must preserve block ownership"
    );
    assert!(!inflater.is_active());

    let bad = compressed_part_frame(hash, 0, 1, &compressed[..split], false);
    assert_eq!(
        parse_part_payload(
            &bad,
            hash,
            &[PartRange {
                begin: 0,
                end: plain.len() as u64,
            }],
            plain.len() as u64,
            &mut inflater,
            plain.len(),
        ),
        Err(TransferPartError::PayloadLengthMismatch)
    );
}

#[test]
fn transfer_state_clears_requested_ranges_on_cancel_timeout_and_retry_failures() {
    let mut state = PeerTransferState::new(10);
    state.record_requested_ranges(vec![PartRange { begin: 10, end: 20 }], 12);
    assert_eq!(state.status, TransferStatus::Downloading);
    assert_eq!(state.requested_ranges.len(), 1);

    let cancel = build_cancel_transfer();
    assert_eq!(cancel.protocol, Protocol::Edonkey);
    assert_eq!(cancel.opcode, u8::from(PeerOpcode::CancelTransfer));
    assert!(cancel.payload.is_empty());

    assert!(!state.expire_stalled(16, 5));
    assert!(state.expire_stalled(18, 5));
    assert_eq!(state.status, TransferStatus::BackingOff { retry_at: 23 });
    assert!(state.requested_ranges.is_empty());

    state.record_requested_ranges(vec![PartRange { begin: 20, end: 30 }], 30);
    state.apply_failure(TransferFailureKind::RemoteCancel, 31);
    assert_eq!(state.status, TransferStatus::OnQueue);
    assert!(state.requested_ranges.is_empty());

    state.record_requested_ranges(vec![PartRange { begin: 30, end: 40 }], 40);
    state.apply_failure(TransferFailureKind::CorruptBlock, 41);
    assert_eq!(state.status, TransferStatus::BackingOff { retry_at: 46 });
    assert!(state.requested_ranges.is_empty());

    state.apply_failure(TransferFailureKind::NoNeededParts, 50);
    assert_eq!(state.status, TransferStatus::NoNeededParts);
}

fn normal_part_frame(
    hash: Ed2kHash,
    begin: u64,
    end: u64,
    data: &[u8],
    use_i64_offsets: bool,
) -> PacketFrame {
    let mut payload = hash.to_vec();
    if use_i64_offsets {
        payload.extend_from_slice(&begin.to_le_bytes());
        payload.extend_from_slice(&end.to_le_bytes());
    } else {
        payload.extend_from_slice(&(begin as u32).to_le_bytes());
        payload.extend_from_slice(&(end as u32).to_le_bytes());
    }
    payload.extend_from_slice(data);
    PacketFrame {
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
    }
}

fn compressed_part_frame(
    hash: Ed2kHash,
    begin: u64,
    total_compressed_len: u32,
    data: &[u8],
    use_i64_offsets: bool,
) -> PacketFrame {
    let mut payload = hash.to_vec();
    if use_i64_offsets {
        payload.extend_from_slice(&begin.to_le_bytes());
    } else {
        payload.extend_from_slice(&(begin as u32).to_le_bytes());
    }
    payload.extend_from_slice(&total_compressed_len.to_le_bytes());
    payload.extend_from_slice(data);
    PacketFrame {
        protocol: Protocol::Emule,
        opcode: if use_i64_offsets {
            PeerOpcode::CompressedPartI64.into()
        } else {
            PeerOpcode::CompressedPart.into()
        },
        payload,
    }
}

fn zlib_compress(input: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(input).expect("compress input");
    encoder.finish().expect("finish compression")
}
