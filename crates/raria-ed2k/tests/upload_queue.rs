use raria_ed2k::hash::ed2k_root_hash;
use raria_ed2k::opcode::PeerOpcode;
use raria_ed2k::packet::Protocol;
use raria_ed2k::peer::PeerEndpoint;
use raria_ed2k::sharing::{
    SharedFileInput, SharedFileOrigin, SharedFileStore, SharedPartError, SharedUploadRequest,
    UdpReaskResponse, UploadDecision, UploadQueue, build_shared_part_frame, build_udp_reask_frame,
    build_upload_response_frame,
};
use raria_ed2k::transfer::{
    CompressedPartInflater, PartPayloadKind, PartRange, parse_part_payload,
};
use std::fs;

fn endpoint(value: u8) -> PeerEndpoint {
    PeerEndpoint {
        ip: u32::from(value),
        port: 4662,
    }
}

fn user(value: u8) -> [u8; 16] {
    [value; 16]
}

fn shared_store(bytes: &[u8]) -> (tempfile::TempDir, SharedFileStore, [u8; 16]) {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let path = tempdir.path().join("shared.bin");
    fs::write(&path, bytes).expect("fixture");
    let root_hash = ed2k_root_hash(bytes);
    let mut store = SharedFileStore::default();
    store
        .add_or_replace(SharedFileInput {
            path,
            name: "shared.bin".to_string(),
            size: bytes.len() as u64,
            root_hash,
            part_hashes: Vec::new(),
            aich_root: None,
            origin: SharedFileOrigin::CompletedDownload,
            verified_ranges: vec![PartRange {
                begin: 0,
                end: bytes.len() as u64,
            }],
            now_seconds: 1,
        })
        .expect("shared file");
    (tempdir, store, root_hash)
}

fn request(
    endpoint: PeerEndpoint,
    user_hash: [u8; 16],
    file_hash: [u8; 16],
) -> SharedUploadRequest {
    SharedUploadRequest {
        endpoint,
        user_hash: Some(user_hash),
        file_hash,
        now_seconds: 10,
    }
}

#[test]
fn upload_queue_enforces_slots_ranks_duplicates_and_queue_limits() {
    let (_tempdir, store, file_hash) = shared_store(b"upload queue");
    let mut queue = UploadQueue::new(1, 2);

    assert_eq!(
        queue.request_upload(&store, request(endpoint(1), user(1), file_hash)),
        UploadDecision::Accepted
    );
    assert_eq!(
        queue.request_upload(&store, request(endpoint(2), user(2), file_hash)),
        UploadDecision::Queued { rank: 1 }
    );
    assert_eq!(
        queue.request_upload(&store, request(endpoint(2), user(2), file_hash)),
        UploadDecision::Queued { rank: 1 }
    );
    assert_eq!(
        queue.request_upload(&store, request(endpoint(3), user(3), file_hash)),
        UploadDecision::Queued { rank: 2 }
    );
    assert_eq!(
        queue.request_upload(&store, request(endpoint(4), user(4), file_hash)),
        UploadDecision::QueueFull
    );
    assert_eq!(
        queue.request_upload(&store, request(endpoint(5), user(2), file_hash)),
        UploadDecision::DuplicatePeer
    );

    assert!(queue.remove(endpoint(1)));
    assert!(queue.is_uploading(endpoint(2)));
    assert_eq!(queue.queue_rank(endpoint(3)), Some(1));
}

#[test]
fn tcp_upload_responses_and_shared_part_frames_use_shared_store_truth() {
    let (_tempdir, store, file_hash) = shared_store(b"0123456789");
    let mut queue = UploadQueue::new(1, 8);

    let accepted = queue.request_upload(&store, request(endpoint(1), user(1), file_hash));
    let frame = build_upload_response_frame(accepted);
    assert_eq!(frame.protocol, Protocol::Edonkey);
    assert_eq!(frame.opcode, u8::from(PeerOpcode::AcceptUploadRequest));
    assert!(frame.payload.is_empty());

    let queued = queue.request_upload(&store, request(endpoint(2), user(2), file_hash));
    let frame = build_upload_response_frame(queued);
    assert_eq!(frame.opcode, u8::from(PeerOpcode::QueueRank));
    assert_eq!(frame.payload, 1_u16.to_le_bytes());

    let missing = build_upload_response_frame(
        queue.request_upload(&store, request(endpoint(3), user(3), user(9))),
    );
    assert_eq!(missing.opcode, u8::from(PeerOpcode::FileRequestNoFile));

    let part = build_shared_part_frame(&store, file_hash, PartRange { begin: 2, end: 6 }, false)
        .expect("part frame");
    assert_eq!(part.opcode, u8::from(PeerOpcode::SendingPart));
    let parsed = parse_part_payload(
        &part,
        file_hash,
        &[PartRange { begin: 2, end: 6 }],
        10,
        &mut CompressedPartInflater::default(),
        1024,
    )
    .expect("parsed part");
    assert_eq!(parsed.kind, PartPayloadKind::Normal);
    assert_eq!(parsed.data, b"2345");

    assert_eq!(
        build_shared_part_frame(&store, file_hash, PartRange { begin: 9, end: 11 }, false),
        Err(SharedPartError::ReadFailed)
    );
}

#[test]
fn udp_reask_responses_are_truthful_for_waiting_uploading_unknown_and_missing_files() {
    let (_tempdir, store, file_hash) = shared_store(b"udp reask");
    let mut queue = UploadQueue::new(1, 8);
    queue.request_upload(&store, request(endpoint(1), user(1), file_hash));
    queue.request_upload(&store, request(endpoint(2), user(2), file_hash));

    assert_eq!(
        queue.handle_udp_reask(&store, endpoint(1), file_hash),
        UdpReaskResponse::Ack { rank: 0 }
    );
    assert_eq!(
        queue.handle_udp_reask(&store, endpoint(2), file_hash),
        UdpReaskResponse::Ack { rank: 1 }
    );
    assert_eq!(
        queue.handle_udp_reask(&store, endpoint(3), file_hash),
        UdpReaskResponse::QueueFull
    );
    assert_eq!(
        queue.handle_udp_reask(&store, endpoint(1), user(9)),
        UdpReaskResponse::FileNotFound
    );

    let ack = build_udp_reask_frame(UdpReaskResponse::Ack { rank: 1 });
    assert_eq!(ack.protocol, Protocol::Emule);
    assert_eq!(ack.opcode, u8::from(PeerOpcode::ReaskAck));
    assert_eq!(ack.payload, 1_u16.to_le_bytes());
    assert_eq!(
        build_udp_reask_frame(UdpReaskResponse::QueueFull).opcode,
        u8::from(PeerOpcode::QueueFull)
    );
}
