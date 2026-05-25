#[cfg(test)]
mod tests {
    use raria_core::native::TaskId;
    use raria_core::persist::Store;
    use raria_core::segment::{SegmentState, SegmentStatus};
    use tempfile::NamedTempFile;

    fn segment(start: u64, end: u64, downloaded: u64, status: SegmentStatus) -> SegmentState {
        SegmentState {
            start,
            end,
            downloaded,
            etag: None,
            status,
        }
    }

    #[test]
    fn native_segments_survive_store_reopen() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        let task_id = TaskId::new();

        {
            let store = Store::open(&path).unwrap();
            store
                .put_native_segment(
                    &task_id,
                    0,
                    &segment(0, 10_000, 5_000, SegmentStatus::Active),
                )
                .unwrap();
        }

        {
            let store = Store::open(&path).unwrap();
            let restored = store
                .get_native_segment(&task_id, 0)
                .unwrap()
                .expect("native segment");
            assert_eq!(restored.start, 0);
            assert_eq!(restored.end, 10_000);
            assert_eq!(restored.downloaded, 5_000);
            assert_eq!(restored.resume_offset(), 5_000);
        }
    }

    #[test]
    fn remove_native_segments_cleans_one_task_only() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Store::open(tmp.path()).unwrap();
        let task_a = TaskId::new();
        let task_b = TaskId::new();

        for index in 0..3 {
            store
                .put_native_segment(
                    &task_a,
                    index,
                    &segment(
                        u64::from(index) * 1000,
                        u64::from(index + 1) * 1000,
                        1000,
                        SegmentStatus::Done,
                    ),
                )
                .unwrap();
        }
        store
            .put_native_segment(&task_b, 0, &segment(0, 5000, 0, SegmentStatus::Pending))
            .unwrap();

        assert_eq!(store.remove_native_segments(&task_a).unwrap(), 3);
        assert!(store.list_native_segments(&task_a).unwrap().is_empty());
        assert_eq!(store.list_native_segments(&task_b).unwrap().len(), 1);
    }

    #[test]
    fn native_segment_resume_merge_uses_checkpointed_offsets() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Store::open(tmp.path()).unwrap();
        let task_id = TaskId::new();

        store
            .put_native_segment(&task_id, 0, &segment(0, 100, 100, SegmentStatus::Done))
            .unwrap();
        store
            .put_native_segment(&task_id, 1, &segment(100, 200, 50, SegmentStatus::Active))
            .unwrap();

        let restored = store.list_native_segments(&task_id).unwrap();
        let resumable: Vec<_> = restored
            .iter()
            .filter(|(_, state)| !state.is_done())
            .collect();

        assert_eq!(resumable.len(), 1);
        assert_eq!(resumable[0].0, 1);
        assert_eq!(resumable[0].1.resume_offset(), 150);
    }
}
