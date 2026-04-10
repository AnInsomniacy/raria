// Segment checkpoint integration tests.
//
// These tests verify that segment states can be:
// 1. Saved to the store during/after download
// 2. Restored from the store after crash
// 3. Used to skip already-completed segments on resume
// 4. Correctly tracked across checkpoint/restore cycles

#[cfg(test)]
mod tests {
    use raria_core::job::{Gid, Job};
    use raria_core::persist::Store;
    use raria_core::segment::{SegmentState, SegmentStatus};
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    /// Complete segments should survive store checkpoint and restore.
    #[test]
    fn checkpoint_and_restore_completed_segments() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Store::open(tmp.path()).unwrap();

        let gid = Gid::from_raw(42);

        // Simulate: 4 segments, 2 completed, 2 in progress
        let segments = [
            SegmentState {
                start: 0,
                end: 1_000_000,
                downloaded: 1_000_000,
                etag: None,
                status: SegmentStatus::Done,
            },
            SegmentState {
                start: 1_000_000,
                end: 2_000_000,
                downloaded: 2_000_000 - 1_000_000,
                etag: Some("abc123".into()),
                status: SegmentStatus::Done,
            },
            SegmentState {
                start: 2_000_000,
                end: 3_000_000,
                downloaded: 500_000, // partial
                etag: None,
                status: SegmentStatus::Active,
            },
            SegmentState {
                start: 3_000_000,
                end: 4_000_000,
                downloaded: 0,
                etag: None,
                status: SegmentStatus::Pending,
            },
        ];

        // Checkpoint all segments
        for (i, seg) in segments.iter().enumerate() {
            store.put_segment(gid, i as u32, seg).unwrap();
        }

        // Restore from store
        let restored = store.list_segments(gid).unwrap();
        assert_eq!(restored.len(), 4);

        // Verify completed segments
        let (_, seg0) = &restored[0];
        assert!(seg0.is_done());
        assert_eq!(seg0.downloaded, 1_000_000);

        // Verify partial segment has correct resume offset
        let (_, seg2) = &restored[2];
        assert!(!seg2.is_done());
        assert_eq!(seg2.resume_offset(), 2_500_000); // start + downloaded
        assert_eq!(seg2.remaining(), 500_000);
    }

    /// After crash (simulated by reopening store), segments should be recoverable.
    #[test]
    fn segments_survive_store_reopen() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        let gid = Gid::from_raw(99);

        // Phase 1: Write segments and close store
        {
            let store = Store::open(&path).unwrap();
            store
                .put_segment(
                    gid,
                    0,
                    &SegmentState {
                        start: 0,
                        end: 10_000,
                        downloaded: 5_000,
                        etag: Some("etag-value".into()),
                        status: SegmentStatus::Active,
                    },
                )
                .unwrap();
            // Store dropped here — simulates crash
        }

        // Phase 2: Reopen store and verify
        {
            let store = Store::open(&path).unwrap();
            let restored = store.get_segment(gid, 0).unwrap();
            assert!(restored.is_some());
            let seg = restored.unwrap();
            assert_eq!(seg.start, 0);
            assert_eq!(seg.end, 10_000);
            assert_eq!(seg.downloaded, 5_000);
            assert_eq!(seg.etag.as_deref(), Some("etag-value"));
            assert_eq!(seg.resume_offset(), 5_000);
        }
    }

    /// Removing a job's segments should clean up all associated segment records.
    #[test]
    fn remove_segments_cleans_all() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Store::open(tmp.path()).unwrap();

        let gid_a = Gid::from_raw(10);
        let gid_b = Gid::from_raw(20);

        // Write segments for two different jobs
        for i in 0..3 {
            store
                .put_segment(
                    gid_a,
                    i,
                    &SegmentState {
                        start: i as u64 * 1000,
                        end: (i as u64 + 1) * 1000,
                        downloaded: 1000,
                        etag: None,
                        status: SegmentStatus::Done,
                    },
                )
                .unwrap();
        }
        store
            .put_segment(
                gid_b,
                0,
                &SegmentState {
                    start: 0,
                    end: 5000,
                    downloaded: 0,
                    etag: None,
                    status: SegmentStatus::Pending,
                },
            )
            .unwrap();

        // Remove gid_a segments only
        let removed = store.remove_segments(gid_a).unwrap();
        assert_eq!(removed, 3);

        // gid_a segments gone
        assert!(store.list_segments(gid_a).unwrap().is_empty());

        // gid_b segments untouched
        assert_eq!(store.list_segments(gid_b).unwrap().len(), 1);
    }

    /// Filtering done segments for resume: only non-done segments need re-downloading.
    #[test]
    fn filter_segments_for_resume() {
        let segments = [
            SegmentState {
                start: 0,
                end: 100,
                downloaded: 100,
                etag: None,
                status: SegmentStatus::Done,
            },
            SegmentState {
                start: 100,
                end: 200,
                downloaded: 50,
                etag: None,
                status: SegmentStatus::Active,
            },
            SegmentState {
                start: 200,
                end: 300,
                downloaded: 0,
                etag: None,
                status: SegmentStatus::Pending,
            },
        ];

        let to_resume: Vec<_> = segments.iter().filter(|s| !s.is_done()).collect();
        assert_eq!(to_resume.len(), 2);
        assert_eq!(to_resume[0].resume_offset(), 150); // start=100, downloaded=50
        assert_eq!(to_resume[1].resume_offset(), 200); // start=200, downloaded=0
    }

    /// Job + segments checkpoint creates a complete crash-recovery snapshot.
    #[test]
    fn full_job_and_segments_checkpoint() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Store::open(tmp.path()).unwrap();

        let mut job = Job::new_range(
            vec!["https://example.com/big.zip".into()],
            PathBuf::from("/tmp/big.zip"),
        );
        job.total_size = Some(4_000_000);
        job.downloaded = 2_000_000;
        let gid = job.gid;

        // Save job
        store.put_job(&job).unwrap();

        // Save segments
        let segments = [
            SegmentState {
                start: 0,
                end: 2_000_000,
                downloaded: 2_000_000,
                etag: None,
                status: SegmentStatus::Done,
            },
            SegmentState {
                start: 2_000_000,
                end: 4_000_000,
                downloaded: 0,
                etag: None,
                status: SegmentStatus::Pending,
            },
        ];
        for (i, seg) in segments.iter().enumerate() {
            store.put_segment(gid, i as u32, seg).unwrap();
        }

        // Restore everything
        let recovered_job = store.get_job(gid).unwrap().unwrap();
        let recovered_segs = store.list_segments(gid).unwrap();

        assert_eq!(recovered_job.total_size, Some(4_000_000));
        assert_eq!(recovered_job.downloaded, 2_000_000);
        assert_eq!(recovered_segs.len(), 2);

        // Only segment 1 needs downloading
        let to_resume: Vec<_> = recovered_segs
            .iter()
            .filter(|(_, s)| !s.is_done())
            .collect();
        assert_eq!(to_resume.len(), 1);
        assert_eq!(to_resume[0].0, 1); // segment_id
        assert_eq!(to_resume[0].1.resume_offset(), 2_000_000);
    }
}
