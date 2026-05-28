use raria_ed2k::hash::{aich_root_hash, ed2k_root_hash};
use raria_ed2k::peer::PeerEndpoint;
use raria_ed2k::sharing::{
    SharedFileInput, SharedFileOrigin, SharedFileStore, SharedPublishConfig, SharingError,
};
use raria_ed2k::transfer::PartRange;
use std::fs;

fn hash(value: u8) -> [u8; 16] {
    [value; 16]
}

#[test]
fn completed_files_enter_shared_metadata_and_replace_duplicate_hashes() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let path = tempdir.path().join("linux.iso");
    let bytes = b"verified payload";
    fs::write(&path, bytes).expect("fixture");

    let mut store = SharedFileStore::default();
    let root_hash = ed2k_root_hash(bytes);
    let aich_root = aich_root_hash(bytes);
    let file = store
        .add_or_replace(SharedFileInput {
            path: path.clone(),
            name: "linux.iso".to_string(),
            size: bytes.len() as u64,
            root_hash,
            part_hashes: Vec::new(),
            aich_root: Some(aich_root),
            origin: SharedFileOrigin::CompletedDownload,
            verified_ranges: vec![PartRange {
                begin: 0,
                end: bytes.len() as u64,
            }],
            now_seconds: 10,
        })
        .expect("shared file");

    assert_eq!(file.path, path);
    assert_eq!(file.name, "linux.iso");
    assert_eq!(file.size, bytes.len() as u64);
    assert_eq!(file.root_hash, root_hash);
    assert_eq!(file.aich_root, Some(aich_root));
    assert_eq!(store.list().len(), 1);

    store
        .add_or_replace(SharedFileInput {
            path: tempdir.path().join("renamed.iso"),
            name: "renamed.iso".to_string(),
            size: bytes.len() as u64,
            root_hash,
            part_hashes: Vec::new(),
            aich_root: Some(aich_root),
            origin: SharedFileOrigin::ImportedFile,
            verified_ranges: vec![PartRange {
                begin: 0,
                end: bytes.len() as u64,
            }],
            now_seconds: 20,
        })
        .expect("replacement");

    assert_eq!(store.list().len(), 1);
    assert_eq!(store.find_by_hash(&root_hash).unwrap().name, "renamed.iso");
    assert_eq!(store.find_by_hash(&root_hash).unwrap().updated_seconds, 20);
}

#[test]
fn invalid_shared_metadata_is_rejected() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let path = tempdir.path().join("empty-name.bin");

    let input = SharedFileInput {
        path,
        name: " ".to_string(),
        size: 8,
        root_hash: hash(1),
        part_hashes: Vec::new(),
        aich_root: None,
        origin: SharedFileOrigin::ImportedFile,
        verified_ranges: vec![PartRange { begin: 0, end: 8 }],
        now_seconds: 1,
    };

    assert_eq!(
        SharedFileStore::default().add_or_replace(input),
        Err(SharingError::InvalidName)
    );
}

#[test]
fn publish_metadata_respects_sharing_policy() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let path = tempdir.path().join("shared.bin");
    fs::write(&path, b"published").expect("fixture");
    let root_hash = ed2k_root_hash(b"published");
    let source_id = hash(9);
    let mut store = SharedFileStore::default();
    store
        .add_or_replace(SharedFileInput {
            path,
            name: "shared.bin".to_string(),
            size: 9,
            root_hash,
            part_hashes: Vec::new(),
            aich_root: Some(aich_root_hash(b"published")),
            origin: SharedFileOrigin::CompletedDownload,
            verified_ranges: vec![PartRange { begin: 0, end: 9 }],
            now_seconds: 1,
        })
        .expect("shared file");

    let disabled = store.publish_records(SharedPublishConfig {
        sharing_enabled: false,
        source_endpoint: PeerEndpoint {
            ip: 0x0102_0304,
            port: 4662,
        },
        source_id,
    });
    assert!(disabled.is_empty());

    let enabled = store.publish_records(SharedPublishConfig {
        sharing_enabled: true,
        source_endpoint: PeerEndpoint {
            ip: 0x0102_0304,
            port: 4662,
        },
        source_id,
    });
    assert_eq!(enabled.len(), 1);
    assert_eq!(enabled[0].file_hash, root_hash);
    assert_eq!(enabled[0].name, "shared.bin");
    assert!(enabled[0].kad_payload.is_some());
    assert!(!enabled[0].server_tags.is_empty());
}

#[test]
fn shared_reads_are_limited_to_verified_ranges() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let path = tempdir.path().join("shared.bin");
    fs::write(&path, b"0123456789").expect("fixture");
    let root_hash = ed2k_root_hash(b"0123456789");
    let mut store = SharedFileStore::default();
    store
        .add_or_replace(SharedFileInput {
            path,
            name: "shared.bin".to_string(),
            size: 10,
            root_hash,
            part_hashes: Vec::new(),
            aich_root: None,
            origin: SharedFileOrigin::CompletedDownload,
            verified_ranges: vec![PartRange { begin: 2, end: 8 }],
            now_seconds: 1,
        })
        .expect("shared file");

    assert_eq!(
        store
            .read_verified_range(&root_hash, PartRange { begin: 3, end: 7 })
            .expect("read"),
        b"3456"
    );
    assert_eq!(
        store.read_verified_range(&root_hash, PartRange { begin: 0, end: 4 }),
        Err(SharingError::UnverifiedRange)
    );
    assert_eq!(
        store.read_verified_range(&root_hash, PartRange { begin: 9, end: 11 }),
        Err(SharingError::InvalidRange)
    );
}
