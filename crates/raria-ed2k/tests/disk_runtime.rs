use raria_ed2k::disk::Ed2kResumeSource;
use raria_ed2k::hash::ed2k_root_hash;
use raria_ed2k::opcode::PeerOpcode;
use raria_ed2k::packet::Protocol;
use raria_ed2k::peer::PeerEndpoint;
use raria_ed2k::runtime::{Ed2kDiskRuntime, Ed2kDiskRuntimeConfig};
use raria_ed2k::sharing::{SharedUploadRequest, UdpReaskResponse, build_udp_reask_frame};
use raria_ed2k::transfer::{PartPayloadKind, PartRange, ReceivedPart, TransferStatus};
use std::fs;

fn user(value: u8) -> [u8; 16] {
    [value; 16]
}

fn endpoint(value: u8) -> PeerEndpoint {
    PeerEndpoint {
        ip: u32::from(value),
        port: 4662,
    }
}

#[test]
fn disk_runtime_flushes_verified_part_and_serves_shared_upload() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let path = tempdir.path().join("file.bin");
    let data = b"verified bytes";
    let root_hash = ed2k_root_hash(data);
    let mut runtime = Ed2kDiskRuntime::new(Ed2kDiskRuntimeConfig {
        path: path.clone(),
        name: "file.bin".to_string(),
        file_size: data.len() as u64,
        root_hash,
        sharing_enabled: true,
        now_seconds: 10,
        ..Default::default()
    })
    .expect("runtime");

    let report = runtime
        .apply_part(ReceivedPart {
            range: PartRange {
                begin: 0,
                end: data.len() as u64,
            },
            data: data.to_vec(),
            kind: PartPayloadKind::Normal,
        })
        .expect("part");

    assert!(report.completed);
    assert_eq!(report.verified_ranges.len(), 1);
    assert_eq!(fs::read(&path).expect("file"), data);
    assert_eq!(runtime.shared_store().list().len(), 1);

    let request = SharedUploadRequest {
        endpoint: endpoint(1),
        user_hash: Some(user(1)),
        file_hash: root_hash,
        now_seconds: 11,
    };
    let response = runtime.request_upload(request);
    assert_eq!(response.opcode, u8::from(PeerOpcode::AcceptUploadRequest));
    let part = runtime
        .build_upload_part(root_hash, PartRange { begin: 0, end: 8 }, false)
        .expect("upload part");
    assert_eq!(part.protocol, Protocol::Edonkey);
    assert_eq!(part.opcode, u8::from(PeerOpcode::SendingPart));
    assert!(matches!(
        build_udp_reask_frame(runtime.handle_udp_reask(endpoint(1), root_hash)).opcode,
        opcode if opcode == u8::from(PeerOpcode::ReaskAck)
    ));
}

#[test]
fn disk_runtime_requeues_corrupt_part_without_sharing_completion() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let path = tempdir.path().join("file.bin");
    let good = b"truth";
    let mut runtime = Ed2kDiskRuntime::new(Ed2kDiskRuntimeConfig {
        path,
        name: "file.bin".to_string(),
        file_size: good.len() as u64,
        root_hash: ed2k_root_hash(good),
        sharing_enabled: true,
        now_seconds: 20,
        ..Default::default()
    })
    .expect("runtime");

    let report = runtime
        .apply_part(ReceivedPart {
            range: PartRange {
                begin: 0,
                end: good.len() as u64,
            },
            data: b"trash".to_vec(),
            kind: PartPayloadKind::Normal,
        })
        .expect("corrupt part is reported");

    assert!(!report.completed);
    assert_eq!(report.requeue_ranges.len(), 1);
    assert!(runtime.shared_store().list().is_empty());
}

#[test]
fn disk_runtime_snapshot_restores_sources_credits_and_transfer_state() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let path = tempdir.path().join("file.bin");
    let data = b"resume";
    let root_hash = ed2k_root_hash(data);
    let mut runtime = Ed2kDiskRuntime::new(Ed2kDiskRuntimeConfig {
        path: path.clone(),
        name: "file.bin".to_string(),
        file_size: data.len() as u64,
        root_hash,
        sharing_enabled: false,
        now_seconds: 30,
        ..Default::default()
    })
    .expect("runtime");
    runtime
        .apply_part(ReceivedPart {
            range: PartRange {
                begin: 0,
                end: data.len() as u64,
            },
            data: data.to_vec(),
            kind: PartPayloadKind::Normal,
        })
        .expect("part");

    let source = Ed2kResumeSource {
        endpoint: "203.0.113.9:4662".to_string(),
        last_seen_seconds: 31,
        queue_rank: Some(7),
    };
    let snapshot = runtime.snapshot(
        vec![source.clone()],
        vec![(user(9), 10, 20)],
        TransferStatus::Downloading,
    );
    let restored = Ed2kDiskRuntime::from_snapshot(
        Ed2kDiskRuntimeConfig {
            path,
            name: "file.bin".to_string(),
            file_size: data.len() as u64,
            root_hash,
            sharing_enabled: false,
            now_seconds: 32,
            ..Default::default()
        },
        snapshot,
    )
    .expect("restored");

    assert_eq!(restored.verified_ranges().len(), 1);
    assert_eq!(restored.resume_sources(), &[source]);
    assert_eq!(restored.transfer_status(), TransferStatus::Downloading);
    assert_eq!(restored.credit_snapshot(), vec![(user(9), 10, 20)]);
}

#[test]
fn disk_runtime_udp_reask_reports_missing_or_unknown_upload_state() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let path = tempdir.path().join("file.bin");
    let data = b"share";
    let root_hash = ed2k_root_hash(data);
    let mut runtime = Ed2kDiskRuntime::new(Ed2kDiskRuntimeConfig {
        path,
        name: "file.bin".to_string(),
        file_size: data.len() as u64,
        root_hash,
        sharing_enabled: true,
        now_seconds: 40,
        ..Default::default()
    })
    .expect("runtime");

    assert_eq!(
        runtime.handle_udp_reask(endpoint(1), root_hash),
        UdpReaskResponse::FileNotFound
    );
    runtime
        .apply_part(ReceivedPart {
            range: PartRange {
                begin: 0,
                end: data.len() as u64,
            },
            data: data.to_vec(),
            kind: PartPayloadKind::Normal,
        })
        .expect("part");
    assert_eq!(
        runtime.handle_udp_reask(endpoint(2), root_hash),
        UdpReaskResponse::QueueFull
    );
}
