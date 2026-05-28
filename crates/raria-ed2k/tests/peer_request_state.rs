use raria_ed2k::opcode::PeerOpcode;
use raria_ed2k::packet::Protocol;
use raria_ed2k::peer::{
    PeerFailureKind, PeerRequestAction, PeerRequestError, PeerRequestPhase, PeerRequestState,
    PeerRequestedRange, build_file_name_request, build_file_status_answer,
    build_file_status_request, build_hashset_answer, build_hashset_request,
    build_start_upload_request, parse_file_status, parse_hashset_answer, parse_queue_rank,
};

#[test]
fn file_request_flow_builds_plain_fallback_packets_and_parses_answers() {
    let hash = [0x33; 16];
    let request =
        build_file_name_request(hash, &[true, false, true, false, true], 2).expect("file request");
    assert_eq!(request.protocol, Protocol::Edonkey);
    assert_eq!(request.opcode, u8::from(PeerOpcode::RequestFileName));
    assert_eq!(&request.payload[0..16], &hash);
    assert_eq!(
        u16::from_le_bytes(request.payload[16..18].try_into().unwrap()),
        5
    );
    assert_eq!(request.payload[18], 0b0001_0101);
    assert_eq!(
        u16::from_le_bytes(request.payload[19..21].try_into().unwrap()),
        0
    );

    let status_request = build_file_status_request(hash);
    assert_eq!(
        status_request.opcode,
        u8::from(PeerOpcode::SetRequestedFileId)
    );
    assert_eq!(status_request.payload, hash);

    let hashset_request = build_hashset_request(hash);
    assert_eq!(hashset_request.opcode, u8::from(PeerOpcode::HashsetRequest));
    assert_eq!(hashset_request.payload, hash);

    let start_upload = build_start_upload_request(hash);
    assert_eq!(
        start_upload.opcode,
        u8::from(PeerOpcode::StartUploadRequest)
    );
    assert_eq!(start_upload.payload, hash);

    let status = build_file_status_answer(hash, &[true, false, true, true]).expect("status");
    assert_eq!(status.opcode, u8::from(PeerOpcode::FileStatus));
    assert_eq!(
        parse_file_status(&status.payload, hash).expect("parsed status"),
        vec![true, false, true, true]
    );

    let piece_hashes = vec![[0x44; 16], [0x55; 16]];
    let hashset = build_hashset_answer(hash, &piece_hashes).expect("hashset");
    assert_eq!(hashset.opcode, u8::from(PeerOpcode::HashsetAnswer));
    assert_eq!(
        parse_hashset_answer(&hashset.payload, hash).expect("parsed hashset"),
        piece_hashes
    );
}

#[test]
fn peer_request_state_tracks_queue_and_releases_owned_ranges_on_failures() {
    let hash = [0x66; 16];
    let mut state = PeerRequestState::new(hash);

    assert_eq!(
        state.apply_file_status(vec![true, false, true], true),
        PeerRequestAction::RequestHashset
    );
    assert_eq!(state.phase, PeerRequestPhase::RequestingHashset);
    assert_eq!(state.part_status, vec![true, false, true]);

    assert_eq!(
        state.apply_hashset_answer(vec![[0x77; 16], [0x88; 16]]),
        PeerRequestAction::StartUpload
    );
    assert_eq!(state.phase, PeerRequestPhase::Connected);
    assert_eq!(state.piece_hashes.len(), 2);

    state.mark_queued(42);
    assert_eq!(state.phase, PeerRequestPhase::OnQueue);
    assert_eq!(state.queue_rank, Some(42));

    state.accept_upload();
    state.record_requested_ranges(vec![PeerRequestedRange {
        begin: 0,
        end: 18_432,
    }]);
    assert_eq!(state.phase, PeerRequestPhase::Downloading);
    assert_eq!(state.requested_ranges.len(), 1);

    state.fail(PeerFailureKind::OutOfParts);
    assert_eq!(state.phase, PeerRequestPhase::OutOfParts);
    assert!(state.requested_ranges.is_empty());
    assert_eq!(state.queue_rank, None);

    state.record_requested_ranges(vec![PeerRequestedRange {
        begin: 18_432,
        end: 36_864,
    }]);
    state.fail(PeerFailureKind::BadPacket);
    assert_eq!(state.phase, PeerRequestPhase::Failed);
    assert!(state.requested_ranges.is_empty());
}

#[test]
fn malformed_request_payloads_return_typed_errors() {
    let hash = [0x99; 16];
    let wrong_hash = [0xaa; 16];
    let status = build_file_status_answer(hash, &[true, false]).expect("status");
    assert_eq!(
        parse_file_status(&status.payload, wrong_hash),
        Err(PeerRequestError::HashMismatch)
    );

    let mut truncated = status.payload.clone();
    truncated.pop();
    assert_eq!(
        parse_file_status(&truncated, hash),
        Err(PeerRequestError::Truncated)
    );

    let mut bad_hashset = hash.to_vec();
    bad_hashset.extend_from_slice(&2_u16.to_le_bytes());
    bad_hashset.extend_from_slice(&[0xbb; 16]);
    assert_eq!(
        parse_hashset_answer(&bad_hashset, hash),
        Err(PeerRequestError::Truncated)
    );

    assert_eq!(
        parse_queue_rank(&70000_u32.to_le_bytes()),
        Err(PeerRequestError::ValueTooLarge)
    );
    assert_eq!(parse_queue_rank(&[1]), Err(PeerRequestError::Truncated));
}
