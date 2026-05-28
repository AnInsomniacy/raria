use raria_ed2k::disk::{Ed2kDiskState, Ed2kDiskStateError, Ed2kResumeSource};
use raria_ed2k::hash::ed2k_root_hash;
use raria_ed2k::transfer::PartRange;

#[test]
fn disk_state_marks_parts_verified_only_after_hash_match() {
    let data = b"verified";
    let mut state = Ed2kDiskState::new(data.len() as u64, ed2k_root_hash(data), Vec::new(), None)
        .expect("disk state");

    state
        .stage_write(PartRange { begin: 0, end: 4 }, &data[..4])
        .expect("first write");
    assert!(state.flush_part(0).is_err());
    assert!(state.verified_ranges().is_empty());

    state
        .stage_write(
            PartRange {
                begin: 4,
                end: data.len() as u64,
            },
            &data[4..],
        )
        .expect("second write");
    let verified = state.flush_part(0).expect("verified part");

    assert_eq!(
        verified,
        PartRange {
            begin: 0,
            end: data.len() as u64,
        }
    );
    assert_eq!(state.verified_ranges(), &[verified]);
    assert!(state.requeue_ranges().is_empty());
}

#[test]
fn corrupt_part_requeues_range_without_marking_completion() {
    let good = b"truth";
    let mut state = Ed2kDiskState::new(good.len() as u64, ed2k_root_hash(good), Vec::new(), None)
        .expect("disk state");

    state
        .stage_write(
            PartRange {
                begin: 0,
                end: good.len() as u64,
            },
            b"trash",
        )
        .expect("write corrupt bytes");

    assert_eq!(
        state.flush_part(0),
        Err(Ed2kDiskStateError::PartHashMismatch { part_index: 0 })
    );
    assert!(state.verified_ranges().is_empty());
    assert_eq!(
        state.requeue_ranges(),
        &[PartRange {
            begin: 0,
            end: good.len() as u64,
        }]
    );
}

#[test]
fn resume_snapshot_restores_verified_parts_aich_and_sources() {
    let data = b"resume-state";
    let aich_root = Some([0x44; 20]);
    let source = Ed2kResumeSource {
        endpoint: "198.51.100.7:4662".to_string(),
        last_seen_seconds: 120,
        queue_rank: Some(42),
    };
    let mut state = Ed2kDiskState::new(
        data.len() as u64,
        ed2k_root_hash(data),
        Vec::new(),
        aich_root,
    )
    .expect("disk state");
    state
        .stage_write(
            PartRange {
                begin: 0,
                end: data.len() as u64,
            },
            data,
        )
        .expect("write bytes");
    state.flush_part(0).expect("verified part");

    let snapshot = state.to_resume_snapshot(vec![source.clone()]);
    let restored = Ed2kDiskState::from_resume_snapshot(snapshot).expect("restored state");

    assert_eq!(
        restored.verified_ranges(),
        &[PartRange {
            begin: 0,
            end: data.len() as u64,
        }]
    );
    assert_eq!(restored.aich_root(), aich_root.as_ref());
    assert_eq!(restored.resume_sources(), &[source]);
}

#[test]
fn resume_snapshot_rejects_wrong_root_hash() {
    let data = b"resume-state";
    let mut state =
        Ed2kDiskState::new(data.len() as u64, [0x99; 16], Vec::new(), None).expect("disk state");
    state
        .stage_write(
            PartRange {
                begin: 0,
                end: data.len() as u64,
            },
            data,
        )
        .expect("write bytes");

    assert_eq!(
        state.flush_part(0),
        Err(Ed2kDiskStateError::PartHashMismatch { part_index: 0 })
    );
}
