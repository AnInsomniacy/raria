use raria_ed2k::hash::ed2k_root_hash;
use raria_ed2k::peer::PeerCapabilities;
use raria_ed2k::peer::PeerEndpoint;
use raria_ed2k::sharing::{
    PeerCreditStore, SharedFileInput, SharedFileOrigin, SharedFileStore, SharedUploadRequest,
    UploadDecision, UploadQueue,
};
use raria_ed2k::transfer::PartRange;
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
fn credit_store_accumulates_counters_and_roundtrips_snapshots() {
    let mut credits = PeerCreditStore::default();
    credits.add_uploaded(user(1), 512);
    credits.add_downloaded(user(1), 2 * 1024 * 1024);
    credits.add_uploaded(user(2), 0);
    credits.add_downloaded(user(2), 0);

    assert_eq!(credits.list().len(), 1);
    assert_eq!(credits.list()[0].user_hash, user(1));
    assert_eq!(credits.list()[0].uploaded_bytes, 512);
    assert_eq!(credits.list()[0].downloaded_bytes, 2 * 1024 * 1024);
    assert!(credits.score_ratio(&user(1)) > 1.0);
    assert_eq!(credits.score_ratio(&user(9)), 1.0);

    let restored = PeerCreditStore::from_snapshot(credits.snapshot()).expect("snapshot");
    assert_eq!(restored.list(), credits.list());
}

#[test]
fn credit_score_influences_waiting_order_within_bounded_limits() {
    let (_tempdir, store, file_hash) = shared_store(b"credit queue");
    let mut queue = UploadQueue::new(1, 8);
    queue.credits_mut().add_downloaded(user(3), 4 * 1024 * 1024);

    assert_eq!(
        queue.request_upload(&store, request(endpoint(1), user(1), file_hash)),
        UploadDecision::Accepted
    );
    assert_eq!(
        queue.request_upload(&store, request(endpoint(2), user(2), file_hash)),
        UploadDecision::Queued { rank: 1 }
    );
    assert_eq!(
        queue.request_upload(&store, request(endpoint(3), user(3), file_hash)),
        UploadDecision::Queued { rank: 1 }
    );
    assert_eq!(queue.queue_rank(endpoint(2)), Some(2));
}

#[test]
fn secure_ident_remains_unadvertised_without_signature_flow() {
    let capabilities = PeerCapabilities::local();

    assert_eq!(capabilities.secure_ident_version, 0);
    assert!(!capabilities.supports_crypt_layer);
    assert!(!capabilities.requests_crypt_layer);
    assert!(!capabilities.requires_crypt_layer);
}
