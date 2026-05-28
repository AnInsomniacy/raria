// raria-core: Native persistence layer using redb.

use crate::native::{
    NativeEd2kIdentityRow, NativeEd2kKadBootstrapRow, NativeEd2kKadRoutingRow, NativeEd2kResumeRow,
    NativeEd2kServerBootstrapRow, NativeSegmentRow, NativeStoreMetadata, NativeTaskRow, TaskId,
};
use crate::segment::SegmentState;
use anyhow::{Context, Result};
use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;
use std::sync::Arc;

/// Table: global_state — stores retained native key-value state.
const GLOBAL_STATE_TABLE: TableDefinition<&str, &str> = TableDefinition::new("global_state");

/// Table: native_metadata — stores versioned native store metadata.
const NATIVE_METADATA_TABLE: TableDefinition<&str, &str> = TableDefinition::new("native_metadata");

/// Table: native_tasks — stores versioned native task rows keyed by task id.
const NATIVE_TASKS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("native_tasks");

/// Table: native_segments — stores versioned segment rows keyed by native task id and segment id.
const NATIVE_SEGMENTS_TABLE: TableDefinition<(&str, u32), &str> =
    TableDefinition::new("native_segments");

/// Table: ed2k_identities — stores versioned native ED2K identity rows.
const ED2K_IDENTITIES_TABLE: TableDefinition<&str, &str> = TableDefinition::new("ed2k_identities");

/// Table: ed2k_server_bootstrap — stores versioned native ED2K server bootstrap rows.
const ED2K_SERVER_BOOTSTRAP_TABLE: TableDefinition<&str, &str> =
    TableDefinition::new("ed2k_server_bootstrap");

/// Table: ed2k_kad_bootstrap — stores versioned native ED2K Kad bootstrap rows.
const ED2K_KAD_BOOTSTRAP_TABLE: TableDefinition<&str, &str> =
    TableDefinition::new("ed2k_kad_bootstrap");

/// Table: ed2k_kad_routing — stores versioned native ED2K Kad routing rows.
const ED2K_KAD_ROUTING_TABLE: TableDefinition<&str, &str> =
    TableDefinition::new("ed2k_kad_routing");

/// Table: ed2k_resume — stores versioned native ED2K resume rows.
const ED2K_RESUME_TABLE: TableDefinition<&str, &str> = TableDefinition::new("ed2k_resume");

/// Persistent storage for raria state.
#[derive(Clone)]
pub struct Store {
    db: Arc<Database>,
}

impl Store {
    /// Open or create a store at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let db = Database::create(path).context("failed to open redb database")?;

        // Ensure all tables exist.
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(GLOBAL_STATE_TABLE)?;
            let mut metadata = write_txn.open_table(NATIVE_METADATA_TABLE)?;
            if metadata.get("store")?.is_none() {
                let row = NativeStoreMetadata::new(format!("store_{:016x}", rand::random::<u64>()));
                let json = serde_json::to_string(&row)?;
                metadata.insert("store", json.as_str())?;
            }
            let _ = write_txn.open_table(NATIVE_TASKS_TABLE)?;
            let _ = write_txn.open_table(NATIVE_SEGMENTS_TABLE)?;
            let _ = write_txn.open_table(ED2K_IDENTITIES_TABLE)?;
            let _ = write_txn.open_table(ED2K_SERVER_BOOTSTRAP_TABLE)?;
            let _ = write_txn.open_table(ED2K_KAD_BOOTSTRAP_TABLE)?;
            let _ = write_txn.open_table(ED2K_KAD_ROUTING_TABLE)?;
            let _ = write_txn.open_table(ED2K_RESUME_TABLE)?;
        }
        write_txn.commit()?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Insert or update a segment state by native task id.
    pub fn put_native_segment(
        &self,
        task_id: &TaskId,
        seg_id: u32,
        state: &SegmentState,
    ) -> Result<()> {
        let row = NativeSegmentRow::from_segment_state(format!("segment_{seg_id}"), state);
        row.validate_version()
            .context("unsupported native segment row version")?;
        let json = serde_json::to_string(&row)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(NATIVE_SEGMENTS_TABLE)?;
            table.insert((task_id.as_str(), seg_id), json.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Retrieve a segment state by native task id.
    pub fn get_native_segment(
        &self,
        task_id: &TaskId,
        seg_id: u32,
    ) -> Result<Option<SegmentState>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(NATIVE_SEGMENTS_TABLE)?;
        match table.get((task_id.as_str(), seg_id))? {
            Some(guard) => {
                let row: NativeSegmentRow = serde_json::from_str(guard.value())?;
                Ok(Some(
                    row.to_segment_state()
                        .context("unsupported native segment row version")?,
                ))
            }
            None => Ok(None),
        }
    }

    /// List all segments for a native task.
    pub fn list_native_segments(&self, task_id: &TaskId) -> Result<Vec<(u32, SegmentState)>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(NATIVE_SEGMENTS_TABLE)?;
        let mut segments = Vec::new();
        for entry in table.iter()? {
            let (key, value) = entry?;
            let (row_task_id, seg_id) = key.value();
            if row_task_id == task_id.as_str() {
                let row: NativeSegmentRow = serde_json::from_str(value.value())?;
                let state = row
                    .to_segment_state()
                    .context("unsupported native segment row version")?;
                segments.push((seg_id, state));
            }
        }
        Ok(segments)
    }

    /// Remove all segments for a native task.
    pub fn remove_native_segments(&self, task_id: &TaskId) -> Result<u32> {
        let write_txn = self.db.begin_write()?;
        let mut count = 0u32;
        {
            let mut table = write_txn.open_table(NATIVE_SEGMENTS_TABLE)?;
            let keys: Vec<(String, u32)> = {
                let mut keys = Vec::new();
                for entry in table.iter()? {
                    let (key, _) = entry?;
                    let (row_task_id, seg_id) = key.value();
                    if row_task_id == task_id.as_str() {
                        keys.push((row_task_id.to_string(), seg_id));
                    }
                }
                keys
            };
            for (row_task_id, seg_id) in keys {
                table.remove((row_task_id.as_str(), seg_id))?;
                count += 1;
            }
        }
        write_txn.commit()?;
        Ok(count)
    }

    // ── Global State ──────────────────────────────────────────────────

    /// Set a global state key-value pair.
    pub fn put_global(&self, key: &str, value: &str) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(GLOBAL_STATE_TABLE)?;
            table.insert(key, value)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get a global state value.
    pub fn get_global(&self, key: &str) -> Result<Option<String>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(GLOBAL_STATE_TABLE)?;
        match table.get(key)? {
            Some(guard) => Ok(Some(guard.value().to_string())),
            None => Ok(None),
        }
    }

    // ── Native Store Schema ──────────────────────────────────────────

    /// Return the native store metadata row.
    pub fn native_metadata(&self) -> Result<NativeStoreMetadata> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(NATIVE_METADATA_TABLE)?;
        let value = table
            .get("store")?
            .context("native store metadata is missing")?;
        let metadata: NativeStoreMetadata = serde_json::from_str(value.value())?;
        Ok(metadata)
    }

    /// Insert or update a native task row.
    pub fn put_native_task(&self, row: &NativeTaskRow) -> Result<()> {
        row.validate_version()
            .context("unsupported native task row version")?;
        let json = serde_json::to_string(row)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(NATIVE_TASKS_TABLE)?;
            table.insert(row.task_id.as_str(), json.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Retrieve a native task row by task id.
    pub fn get_native_task(&self, task_id: &TaskId) -> Result<Option<NativeTaskRow>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(NATIVE_TASKS_TABLE)?;
        match table.get(task_id.as_str())? {
            Some(guard) => {
                let row: NativeTaskRow = serde_json::from_str(guard.value())?;
                row.validate_version()
                    .context("unsupported native task row version")?;
                Ok(Some(row))
            }
            None => Ok(None),
        }
    }

    /// List all native task rows.
    pub fn list_native_tasks(&self) -> Result<Vec<NativeTaskRow>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(NATIVE_TASKS_TABLE)?;
        let mut rows = Vec::new();
        for entry in table.iter()? {
            let (_, value) = entry?;
            let row: NativeTaskRow = serde_json::from_str(value.value())?;
            row.validate_version()
                .context("unsupported native task row version")?;
            rows.push(row);
        }
        Ok(rows)
    }

    /// Insert or update a native ED2K identity row.
    pub fn put_ed2k_identity(&self, row: &NativeEd2kIdentityRow) -> Result<()> {
        row.validate_version()
            .context("unsupported native ED2K identity row version")?;
        let json = serde_json::to_string(row)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(ED2K_IDENTITIES_TABLE)?;
            table.insert(row.profile_id.as_str(), json.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Retrieve a native ED2K identity row by profile id.
    pub fn get_ed2k_identity(&self, profile_id: &str) -> Result<Option<NativeEd2kIdentityRow>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(ED2K_IDENTITIES_TABLE)?;
        match table.get(profile_id)? {
            Some(guard) => {
                let row: NativeEd2kIdentityRow = serde_json::from_str(guard.value())?;
                row.validate_version()
                    .context("unsupported native ED2K identity row version")?;
                Ok(Some(row))
            }
            None => Ok(None),
        }
    }

    /// Insert or update native ED2K server bootstrap state.
    pub fn put_ed2k_server_bootstrap(&self, row: &NativeEd2kServerBootstrapRow) -> Result<()> {
        row.validate_version()
            .context("unsupported native ED2K server bootstrap row version")?;
        let json = serde_json::to_string(row)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(ED2K_SERVER_BOOTSTRAP_TABLE)?;
            table.insert(row.profile_id.as_str(), json.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Retrieve native ED2K server bootstrap state by profile id.
    pub fn get_ed2k_server_bootstrap(
        &self,
        profile_id: &str,
    ) -> Result<Option<NativeEd2kServerBootstrapRow>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(ED2K_SERVER_BOOTSTRAP_TABLE)?;
        match table.get(profile_id)? {
            Some(guard) => {
                let row: NativeEd2kServerBootstrapRow = serde_json::from_str(guard.value())?;
                row.validate_version()
                    .context("unsupported native ED2K server bootstrap row version")?;
                Ok(Some(row))
            }
            None => Ok(None),
        }
    }

    /// Insert or update native ED2K Kad bootstrap state.
    pub fn put_ed2k_kad_bootstrap(&self, row: &NativeEd2kKadBootstrapRow) -> Result<()> {
        row.validate_version()
            .context("unsupported native ED2K Kad bootstrap row version")?;
        let json = serde_json::to_string(row)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(ED2K_KAD_BOOTSTRAP_TABLE)?;
            table.insert(row.profile_id.as_str(), json.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Retrieve native ED2K Kad bootstrap state by profile id.
    pub fn get_ed2k_kad_bootstrap(
        &self,
        profile_id: &str,
    ) -> Result<Option<NativeEd2kKadBootstrapRow>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(ED2K_KAD_BOOTSTRAP_TABLE)?;
        match table.get(profile_id)? {
            Some(guard) => {
                let row: NativeEd2kKadBootstrapRow = serde_json::from_str(guard.value())?;
                row.validate_version()
                    .context("unsupported native ED2K Kad bootstrap row version")?;
                Ok(Some(row))
            }
            None => Ok(None),
        }
    }

    /// Insert or update native ED2K Kad routing state.
    pub fn put_ed2k_kad_routing(&self, row: &NativeEd2kKadRoutingRow) -> Result<()> {
        row.validate_version()
            .context("unsupported native ED2K Kad routing row version")?;
        let json = serde_json::to_string(row)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(ED2K_KAD_ROUTING_TABLE)?;
            table.insert(row.profile_id.as_str(), json.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Retrieve native ED2K Kad routing state by profile id.
    pub fn get_ed2k_kad_routing(
        &self,
        profile_id: &str,
    ) -> Result<Option<NativeEd2kKadRoutingRow>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(ED2K_KAD_ROUTING_TABLE)?;
        match table.get(profile_id)? {
            Some(guard) => {
                let row: NativeEd2kKadRoutingRow = serde_json::from_str(guard.value())?;
                row.validate_version()
                    .context("unsupported native ED2K Kad routing row version")?;
                Ok(Some(row))
            }
            None => Ok(None),
        }
    }

    /// Insert or update native ED2K resume state.
    pub fn put_ed2k_resume(&self, row: &NativeEd2kResumeRow) -> Result<()> {
        row.validate_version()
            .context("unsupported native ED2K resume row version")?;
        let json = serde_json::to_string(row)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(ED2K_RESUME_TABLE)?;
            table.insert(row.task_id.as_str(), json.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Retrieve native ED2K resume state by task id.
    pub fn get_ed2k_resume(&self, task_id: &TaskId) -> Result<Option<NativeEd2kResumeRow>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(ED2K_RESUME_TABLE)?;
        match table.get(task_id.as_str())? {
            Some(guard) => {
                let row: NativeEd2kResumeRow = serde_json::from_str(guard.value())?;
                row.validate_version()
                    .context("unsupported native ED2K resume row version")?;
                Ok(Some(row))
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::TaskLifecycle;
    use crate::segment::SegmentStatus;

    fn temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let store = Store::open(&db_path).unwrap();
        (store, dir)
    }

    #[test]
    fn store_opens_and_creates_tables() {
        let (_store, _dir) = temp_store();
        // If we got here, tables were created successfully.
    }

    #[test]
    fn native_segment_roundtrips() {
        let (store, _dir) = temp_store();
        let task_id = TaskId::new();
        let seg = SegmentState {
            start: 0,
            end: 1000,
            downloaded: 500,
            etag: Some("abc".into()),
            status: SegmentStatus::Active,
        };

        store.put_native_segment(&task_id, 0, &seg).unwrap();
        let native_recovered = store
            .get_native_segment(&task_id, 0)
            .unwrap()
            .expect("native segment exists");
        assert_eq!(native_recovered.start, 0);
        assert_eq!(native_recovered.end, 1000);
        assert_eq!(native_recovered.downloaded, 500);
        assert_eq!(native_recovered.etag.as_deref(), Some("abc"));
    }

    #[test]
    fn native_segments_are_keyed_by_task_id() {
        let (store, _dir) = temp_store();
        let task_a = TaskId::new();
        let task_b = TaskId::new();

        let seg = SegmentState {
            start: 0,
            end: 100,
            downloaded: 0,
            etag: None,
            status: SegmentStatus::Pending,
        };

        store.put_native_segment(&task_a, 0, &seg).unwrap();
        store.put_native_segment(&task_a, 1, &seg).unwrap();
        store.put_native_segment(&task_b, 0, &seg).unwrap();

        assert_eq!(store.list_native_segments(&task_a).unwrap().len(), 2);
        assert_eq!(store.list_native_segments(&task_b).unwrap().len(), 1);
        assert_eq!(store.remove_native_segments(&task_a).unwrap(), 2);
        assert!(store.list_native_segments(&task_a).unwrap().is_empty());
        assert_eq!(store.list_native_segments(&task_b).unwrap().len(), 1);
    }

    #[test]
    fn global_state_put_get() {
        let (store, _dir) = temp_store();
        store.put_global("next_gid", "42").unwrap();
        let val = store.get_global("next_gid").unwrap().expect("exists");
        assert_eq!(val, "42");
    }

    #[test]
    fn global_state_missing_returns_none() {
        let (store, _dir) = temp_store();
        assert!(store.get_global("nonexistent").unwrap().is_none());
    }

    #[test]
    fn global_state_overwrite() {
        let (store, _dir) = temp_store();
        store.put_global("key", "v1").unwrap();
        store.put_global("key", "v2").unwrap();
        let val = store.get_global("key").unwrap().unwrap();
        assert_eq!(val, "v2");
    }

    #[test]
    fn native_metadata_is_created_when_store_opens() {
        let (store, _dir) = temp_store();

        let metadata = store.native_metadata().unwrap();

        assert_eq!(
            metadata.schema_version,
            NativeStoreMetadata::CURRENT_SCHEMA_VERSION
        );
        assert!(metadata.store_id.starts_with("store_"));
    }

    #[test]
    fn native_task_rows_roundtrip_by_task_id() {
        let (store, _dir) = temp_store();
        let task_id = TaskId::new();
        let row = NativeTaskRow::new(task_id.clone(), TaskLifecycle::Queued);

        store.put_native_task(&row).unwrap();
        let recovered = store
            .get_native_task(&task_id)
            .unwrap()
            .expect("native task row");

        assert_eq!(recovered.task_id, task_id);
        assert_eq!(recovered.lifecycle, TaskLifecycle::Queued);
        assert_eq!(recovered.row_version, NativeTaskRow::CURRENT_ROW_VERSION);
    }

    #[test]
    fn ed2k_identity_rows_roundtrip_by_profile() {
        let (store, _dir) = temp_store();
        let row = crate::native::NativeEd2kIdentityRow::new(
            "default",
            [0x11; crate::native::ED2K_CLIENT_IDENTITY_BYTES],
        );

        store.put_ed2k_identity(&row).unwrap();
        let recovered = store
            .get_ed2k_identity("default")
            .unwrap()
            .expect("ED2K identity row");

        assert_eq!(
            recovered.row_version,
            crate::native::NativeEd2kIdentityRow::CURRENT_ROW_VERSION
        );
        assert_eq!(recovered.profile_id, "default");
        assert_eq!(
            recovered.client_hash,
            [0x11; crate::native::ED2K_CLIENT_IDENTITY_BYTES]
        );
    }

    #[test]
    fn ed2k_bootstrap_rows_roundtrip_by_profile() {
        let (store, _dir) = temp_store();
        let server_row = crate::native::NativeEd2kServerBootstrapRow::new(
            "default",
            vec![crate::native::NativeEd2kServerBootstrapEntry {
                host: "1.2.3.4".into(),
                port: 4661,
                name: Some("Peer Server".into()),
                description: None,
                users: Some(10),
                files: Some(20),
                max_users: None,
                soft_files: None,
                hard_files: None,
                udp_flags: Some(7),
                low_id_users: None,
                udp_key: Some(0x11223344),
                tcp_obfuscation_port: None,
                udp_obfuscation_port: None,
            }],
        );
        let kad_row = crate::native::NativeEd2kKadBootstrapRow::new(
            "default",
            vec![crate::native::NativeEd2kKadBootstrapContact {
                id: [0x55; crate::native::ED2K_CLIENT_IDENTITY_BYTES],
                host: "203.0.113.1".into(),
                udp_port: 4672,
                tcp_port: 4662,
                version: 8,
                verified: true,
            }],
        );

        store.put_ed2k_server_bootstrap(&server_row).unwrap();
        store.put_ed2k_kad_bootstrap(&kad_row).unwrap();

        let recovered_servers = store
            .get_ed2k_server_bootstrap("default")
            .unwrap()
            .expect("server bootstrap row");
        let recovered_kad = store
            .get_ed2k_kad_bootstrap("default")
            .unwrap()
            .expect("Kad bootstrap row");

        assert_eq!(recovered_servers, server_row);
        assert_eq!(recovered_kad, kad_row);
    }

    #[test]
    fn ed2k_resume_rows_roundtrip_by_task() {
        let (store, _dir) = temp_store();
        let task_id = TaskId::new();
        let row = crate::native::NativeEd2kResumeRow::new(
            task_id.clone(),
            12,
            [0x11; 16],
            Vec::new(),
            Some([0x22; 20]),
            vec![crate::native::ByteRange::new(0, 12).unwrap()],
            vec![crate::native::ByteRange::new(4, 8).unwrap()],
            vec![crate::native::NativeEd2kResumeSourceRow {
                endpoint: "198.51.100.7:4662".to_string(),
                last_seen_seconds: 120,
                queue_rank: Some(42),
            }],
        );

        store.put_ed2k_resume(&row).unwrap();
        let loaded = store
            .get_ed2k_resume(&task_id)
            .unwrap()
            .expect("ED2K resume row");

        assert_eq!(loaded.task_id, task_id);
        assert_eq!(loaded.file_size, 12);
        assert_eq!(loaded.root_hash, [0x11; 16]);
        assert_eq!(loaded.aich_root, Some([0x22; 20]));
        assert_eq!(loaded.verified_ranges, row.verified_ranges);
        assert_eq!(loaded.requeue_ranges, row.requeue_ranges);
        assert_eq!(loaded.sources, row.sources);
        assert_eq!(
            loaded.row_version,
            crate::native::NativeEd2kResumeRow::CURRENT_ROW_VERSION
        );
    }

    #[test]
    fn ed2k_kad_routing_rows_roundtrip_by_profile() {
        let (store, _dir) = temp_store();
        let row = crate::native::NativeEd2kKadRoutingRow::new(
            "default",
            r#"{"selfId":[0],"lastBootstrapSeconds":30}"#,
        );

        store.put_ed2k_kad_routing(&row).unwrap();
        let loaded = store
            .get_ed2k_kad_routing("default")
            .unwrap()
            .expect("ED2K Kad routing row");

        assert_eq!(loaded.profile_id, "default");
        assert_eq!(loaded.routing_snapshot_json, row.routing_snapshot_json);
        assert_eq!(
            loaded.row_version,
            crate::native::NativeEd2kKadRoutingRow::CURRENT_ROW_VERSION
        );
    }

    #[test]
    fn list_native_task_rows_returns_all_rows() {
        let (store, _dir) = temp_store();
        let first = NativeTaskRow::new(TaskId::new(), TaskLifecycle::Queued);
        let second = NativeTaskRow::new(TaskId::new(), TaskLifecycle::Paused);

        store.put_native_task(&first).unwrap();
        store.put_native_task(&second).unwrap();

        let rows = store.list_native_tasks().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.task_id == first.task_id));
        assert!(rows.iter().any(|row| row.task_id == second.task_id));
    }

    #[test]
    fn segment_checkpoint_resume_cycle() {
        use crate::segment::{init_segment_states, plan_segments};

        let (store, _dir) = temp_store();
        let task_id = TaskId::new();
        let total_size = 10_000u64;
        let num_segments = 4;

        // Plan segments.
        let ranges = plan_segments(total_size, num_segments);
        let segments = init_segment_states(&ranges);
        assert_eq!(segments.len(), 4);

        // Simulate partial download: segments 0 and 1 done, segment 2 partial.
        store
            .put_native_segment(
                &task_id,
                0,
                &SegmentState {
                    start: segments[0].start,
                    end: segments[0].end,
                    downloaded: segments[0].size(),
                    etag: None,
                    status: SegmentStatus::Done,
                },
            )
            .unwrap();
        store
            .put_native_segment(
                &task_id,
                1,
                &SegmentState {
                    start: segments[1].start,
                    end: segments[1].end,
                    downloaded: segments[1].size(),
                    etag: None,
                    status: SegmentStatus::Done,
                },
            )
            .unwrap();
        store
            .put_native_segment(
                &task_id,
                2,
                &SegmentState {
                    start: segments[2].start,
                    end: segments[2].end,
                    downloaded: 500,
                    etag: Some("abc123".into()),
                    status: SegmentStatus::Active,
                },
            )
            .unwrap();

        // Now simulate resume: re-plan segments and merge persisted state.
        let fresh_ranges = plan_segments(total_size, num_segments);
        let mut fresh_segments = init_segment_states(&fresh_ranges);

        let persisted = store.list_native_segments(&task_id).unwrap();
        assert_eq!(persisted.len(), 3); // Only 3 segments were checkpointed.

        for (seg_id, persisted_state) in &persisted {
            if let Some(seg) = fresh_segments.get_mut(*seg_id as usize) {
                if persisted_state.downloaded > 0 && persisted_state.downloaded <= seg.size() {
                    seg.downloaded = persisted_state.downloaded;
                }
            }
        }

        // Verify merged state.
        assert_eq!(fresh_segments[0].downloaded, fresh_segments[0].size()); // Done.
        assert_eq!(fresh_segments[1].downloaded, fresh_segments[1].size()); // Done.
        assert_eq!(fresh_segments[2].downloaded, 500); // Partial.
        assert_eq!(fresh_segments[3].downloaded, 0); // Not checkpointed.
    }

    #[test]
    fn segment_checkpoint_idempotent_update() {
        let (store, _dir) = temp_store();
        let task_id = TaskId::new();

        let seg_v1 = SegmentState {
            start: 0,
            end: 1000,
            downloaded: 100,
            etag: None,
            status: SegmentStatus::Active,
        };
        store.put_native_segment(&task_id, 0, &seg_v1).unwrap();

        let seg_v2 = SegmentState {
            start: 0,
            end: 1000,
            downloaded: 750,
            etag: None,
            status: SegmentStatus::Active,
        };
        store.put_native_segment(&task_id, 0, &seg_v2).unwrap();

        let recovered = store.get_native_segment(&task_id, 0).unwrap().unwrap();
        assert_eq!(recovered.downloaded, 750);
    }
}
