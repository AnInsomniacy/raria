use raria_ed2k::hash::{AICH_EMBLOCK_SIZE, ED2K_PART_SIZE};
use raria_ed2k::opcode::PeerOpcode;
use raria_ed2k::packet::Protocol;
use raria_ed2k::transfer::{
    PartPlanInput, PartRange, TransferPlanError, build_part_request, parse_part_request,
    plan_part_requests, ranges_overlap,
};

#[test]
fn planner_selects_available_non_overlapping_ed2k_block_ranges() {
    let file_size = ED2K_PART_SIZE + AICH_EMBLOCK_SIZE * 2;
    let input = PartPlanInput {
        file_size,
        completed_ranges: vec![PartRange {
            begin: 0,
            end: AICH_EMBLOCK_SIZE,
        }],
        globally_requested: vec![PartRange {
            begin: AICH_EMBLOCK_SIZE * 2,
            end: AICH_EMBLOCK_SIZE * 3,
        }],
        peer_requested: vec![PartRange {
            begin: AICH_EMBLOCK_SIZE * 3,
            end: AICH_EMBLOCK_SIZE * 4,
        }],
        remote_part_status: vec![true, false],
        max_new_ranges: 3,
    };

    let ranges = plan_part_requests(&input).expect("planned ranges");

    assert_eq!(
        ranges,
        vec![
            PartRange {
                begin: AICH_EMBLOCK_SIZE,
                end: AICH_EMBLOCK_SIZE * 2,
            },
            PartRange {
                begin: AICH_EMBLOCK_SIZE * 4,
                end: AICH_EMBLOCK_SIZE * 5,
            },
            PartRange {
                begin: AICH_EMBLOCK_SIZE * 5,
                end: AICH_EMBLOCK_SIZE * 6,
            },
        ]
    );
    assert!(ranges.iter().all(|range| range.end <= ED2K_PART_SIZE));
    assert!(
        ranges
            .iter()
            .all(|range| !ranges_overlap(*range, input.completed_ranges[0]))
    );
    assert!(
        ranges
            .iter()
            .all(|range| !ranges_overlap(*range, input.globally_requested[0]))
    );
    assert!(
        ranges
            .iter()
            .all(|range| !ranges_overlap(*range, input.peer_requested[0]))
    );
}

#[test]
fn planner_handles_last_partial_block_and_remote_part_status() {
    let file_size = ED2K_PART_SIZE + 17;
    let input = PartPlanInput {
        file_size,
        completed_ranges: Vec::new(),
        globally_requested: Vec::new(),
        peer_requested: Vec::new(),
        remote_part_status: vec![false, true],
        max_new_ranges: 3,
    };

    let ranges = plan_part_requests(&input).expect("planned ranges");

    assert_eq!(
        ranges,
        vec![PartRange {
            begin: ED2K_PART_SIZE,
            end: ED2K_PART_SIZE + 17,
        }]
    );
}

#[test]
fn request_parts_payload_roundtrips_legacy_and_i64_offsets() {
    let hash = [0x44; 16];
    let small_ranges = vec![
        PartRange { begin: 10, end: 20 },
        PartRange { begin: 30, end: 40 },
    ];

    let small = build_part_request(hash, &small_ranges, false).expect("small request");
    assert_eq!(small.protocol, Protocol::Edonkey);
    assert_eq!(small.opcode, u8::from(PeerOpcode::RequestParts));
    assert_eq!(small.payload.len(), 16 + 3 * 4 * 2);
    assert_eq!(
        parse_part_request(&small.payload, hash, false).expect("small parsed"),
        small_ranges
    );

    let large_ranges = vec![PartRange {
        begin: u64::from(u32::MAX) + 1,
        end: u64::from(u32::MAX) + 1 + AICH_EMBLOCK_SIZE,
    }];
    let large = build_part_request(hash, &large_ranges, true).expect("i64 request");
    assert_eq!(large.protocol, Protocol::Emule);
    assert_eq!(large.opcode, u8::from(PeerOpcode::RequestPartsI64));
    assert_eq!(large.payload.len(), 16 + 3 * 8 * 2);
    assert_eq!(
        parse_part_request(&large.payload, hash, true).expect("i64 parsed"),
        large_ranges
    );
}

#[test]
fn invalid_part_ranges_and_payloads_return_typed_errors() {
    let hash = [0x55; 16];
    assert_eq!(
        build_part_request(hash, &[PartRange { begin: 10, end: 10 }], false,),
        Err(TransferPlanError::InvalidRange)
    );
    assert_eq!(
        build_part_request(
            hash,
            &[PartRange {
                begin: u64::from(u32::MAX) + 1,
                end: u64::from(u32::MAX) + 2,
            }],
            false,
        ),
        Err(TransferPlanError::OffsetTooLarge)
    );
    assert_eq!(
        build_part_request(hash, &[PartRange { begin: 1, end: 2 }; 4], true),
        Err(TransferPlanError::TooManyRanges)
    );

    let mut bad = hash.to_vec();
    bad.extend_from_slice(&[0; 8]);
    assert_eq!(
        parse_part_request(&bad, hash, false),
        Err(TransferPlanError::Truncated)
    );
    assert_eq!(
        parse_part_request(&bad, [0x66; 16], false),
        Err(TransferPlanError::HashMismatch)
    );
}
