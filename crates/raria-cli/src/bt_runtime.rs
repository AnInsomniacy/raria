use anyhow::{Context, Result};
use base64::Engine as Base64Engine;
use raria_bt::service::{
    BtMetadata, BtService, BtServiceConfig, BtSource, BtStatus, PieceSelectionStrategy,
};
use raria_bt::torrent_meta::TorrentMeta;
use raria_core::config::BtPieceStrategy;
use raria_core::engine::Engine;
use raria_core::job::{BtCompletionDisposition, BtFile, BtPeer, Gid, Job, Status};
use raria_core::logging::emit_structured_log;
use raria_core::native::TaskId;
use raria_core::progress::DownloadEvent;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

fn map_bt_files(files: Vec<raria_bt::service::BtFileInfo>) -> Vec<BtFile> {
    files
        .into_iter()
        .map(|file| BtFile {
            index: file.index,
            path: file.path,
            length: file.size,
            completed_length: file.completed_length,
            selected: file.selected,
        })
        .collect()
}

fn map_bt_peers(peers: Vec<raria_bt::service::BtPeerInfo>) -> Vec<BtPeer> {
    peers
        .into_iter()
        .map(|peer| BtPeer {
            addr: peer.addr,
            ip: peer.ip,
            port: peer.port,
            download_speed: peer.download_speed,
            upload_speed: peer.upload_speed,
            seeder: peer.seeder,
        })
        .collect()
}

fn handle_bt_cancellation(engine: &Engine, gid: Gid) {
    if let Some(job) = engine.registry.get(gid) {
        info!(%gid, status = ?job.status, "preserving BT job status on cancellation");
    } else {
        warn!(%gid, "BT job missing while handling cancellation");
    }
}

fn bt_service_config(engine: &Engine) -> BtServiceConfig {
    BtServiceConfig {
        socks_proxy_url: engine
            .config
            .proxy
            .clone()
            .filter(|proxy| proxy.starts_with("socks5://")),
        dht_config_filename: engine.config.bt_dht_config_file.clone(),
        session_persistence_dir: Some(native_bt_session_persistence_dir(
            &engine.config.session_file,
        )),
        enable_pex: engine.config.bt_enable_pex,
        piece_selection_strategy: match engine.config.bt_piece_strategy {
            BtPieceStrategy::Current => PieceSelectionStrategy::Current,
            BtPieceStrategy::RarestFirst => PieceSelectionStrategy::RarestFirst,
        },
        ..Default::default()
    }
}

pub(crate) fn native_bt_session_persistence_dir(session_file: &std::path::Path) -> PathBuf {
    let parent = session_file
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let stem = session_file
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("raria.session");
    parent.join(format!("{stem}.bt-session"))
}

pub(crate) fn create_bt_service(engine: &Engine, download_dir: PathBuf) -> Result<Arc<BtService>> {
    BtService::with_config(download_dir, bt_service_config(engine))
        .map(Arc::new)
        .context("failed to create BtService")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BtCompletionAction {
    None,
    EnterSeeding,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BtStatusSyncOutcome {
    completion_action: BtCompletionAction,
    seed_ratio: Option<f64>,
    seed_time: Option<u64>,
    idle_download_timeout: Option<u64>,
}

fn sync_bt_status_into_job(
    job: &mut Job,
    status: &BtStatus,
    bt_files: Option<Vec<BtFile>>,
    bt_peers: Option<Vec<BtPeer>>,
) -> Result<BtStatusSyncOutcome> {
    job.downloaded = status.downloaded;
    job.download_speed = status.download_speed;
    job.upload_speed = status.upload_speed;
    job.connections = status.num_peers;
    if status.total_size > 0 {
        job.total_size = Some(status.total_size);
    }

    if let Some(bt_files) = bt_files {
        job.bt_files = Some(bt_files);
    }
    if let Some(bt_peers) = bt_peers {
        job.bt_peers = Some(bt_peers);
    }

    let bt = job.bt.get_or_insert_with(Default::default);
    if !status.info_hash.is_empty() {
        bt.info_hash = Some(status.info_hash.clone());
    }
    if let Some(torrent_name) = status.torrent_name.as_ref() {
        bt.torrent_name = Some(torrent_name.clone());
    }
    if let Some(announce_list) = status.announce_list.as_ref() {
        bt.announce_list = Some(announce_list.clone());
    }
    bt.uploaded = Some(status.uploaded);
    bt.num_seeders = Some(status.num_seeders);
    if let Some(piece_length) = status.piece_length {
        bt.piece_length = Some(piece_length);
    }
    if let Some(num_pieces) = status.num_pieces {
        bt.num_pieces = Some(num_pieces);
    }

    let completion_action = if status.is_complete && job.status == Status::Active {
        if job.options.seed_ratio.is_some() || job.options.seed_time.is_some() {
            job.record_bt_download_complete(BtCompletionDisposition::Seed)
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            BtCompletionAction::EnterSeeding
        } else {
            BtCompletionAction::Complete
        }
    } else {
        BtCompletionAction::None
    };

    Ok(BtStatusSyncOutcome {
        completion_action,
        seed_ratio: job.options.seed_ratio,
        seed_time: job.options.seed_time,
        idle_download_timeout: job.options.bt_idle_download_timeout,
    })
}

fn sync_bt_metadata_into_job(job: &mut Job, metadata: &BtMetadata) {
    job.total_size = Some(metadata.total_size);
    job.bt_files = Some(
        metadata
            .files
            .iter()
            .map(|file| BtFile {
                index: file.index,
                path: file.path.clone(),
                length: file.size,
                completed_length: 0,
                selected: file.selected,
            })
            .collect(),
    );
    let bt = job.bt.get_or_insert_with(Default::default);
    bt.info_hash = Some(metadata.info_hash.clone());
    bt.torrent_name = metadata.torrent_name.clone();
    bt.piece_length = Some(metadata.piece_length);
    bt.num_pieces = Some(metadata.num_pieces);
}

fn sync_bt_job_from_metadata(engine: &Engine, gid: Gid, metadata: &BtMetadata) -> Result<()> {
    engine
        .registry
        .update(gid, |job| sync_bt_metadata_into_job(job, metadata))
        .context("BT job not found in registry")?;
    if let Some(task_id) = engine.task_id_for_gid(gid) {
        engine.publish_native_bt_metadata_resolved(
            &task_id,
            &metadata.info_hash,
            metadata.torrent_name.as_deref(),
            Some(metadata.total_size),
            Some(metadata.piece_length),
            Some(metadata.num_pieces),
        )?;
    }
    Ok(())
}

fn sync_bt_job_from_status(
    engine: &Engine,
    gid: Gid,
    status: &BtStatus,
    bt_files: Option<Vec<BtFile>>,
    bt_peers: Option<Vec<BtPeer>>,
) -> Result<BtStatusSyncOutcome> {
    engine
        .registry
        .update(gid, |job| {
            sync_bt_status_into_job(job, status, bt_files, bt_peers)
        })
        .context("BT job not found in registry")?
        .and_then(|outcome| {
            if !status.info_hash.is_empty() {
                if let Some(task_id) = engine.task_id_for_gid(gid) {
                    engine.publish_native_bt_metadata_resolved(
                        &task_id,
                        &status.info_hash,
                        status.torrent_name.as_deref(),
                        (status.total_size > 0).then_some(status.total_size),
                        status.piece_length,
                        status.num_pieces,
                    )?;
                    if outcome.completion_action == BtCompletionAction::EnterSeeding {
                        engine.publish_native_bt_seeding_started(
                            &task_id,
                            status.uploaded,
                            status.num_peers,
                            Some(status.num_seeders),
                        )?;
                    }
                    for peer in engine.native_task_peers(&task_id)? {
                        engine.publish_native_bt_peer_updated(&task_id, peer)?;
                    }
                    for tracker in engine.native_task_trackers(&task_id)? {
                        engine.publish_native_bt_tracker_updated(&task_id, tracker)?;
                    }
                }
            }
            Ok(outcome)
        })
}

async fn cleanup_unselected_bt_files(
    output_dir: &std::path::Path,
    torrent_bytes: &[u8],
    selected_files: &[usize],
) -> Result<()> {
    let meta = TorrentMeta::from_bytes(torrent_bytes)?;
    let selected = selected_files.iter().copied().collect::<HashSet<_>>();
    for (index, file) in meta.files.iter().enumerate() {
        if !selected.contains(&index) {
            let path = output_dir.join(&file.path);
            match tokio::fs::remove_file(&path).await {
                Ok(()) => {
                    info!(file = %path.display(), "removed unselected BT file after completion");
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    warn!(file = %path.display(), %error, "failed to remove unselected BT file");
                }
            }
        }
    }
    Ok(())
}

fn persist_bt_job(engine: &Engine, gid: Gid) {
    if let Some(store) = engine.store() {
        if let Some(job) = engine.registry.get(gid) {
            let row = raria_core::native::NativeTaskRow::from_runtime_job(&job);
            if let Err(error) = store.put_native_task(&row) {
                warn!(%gid, task_id = %job.task_id, error = %error, "failed to persist BT native task row");
            }
        }
    }
}

fn should_stop_seeding(
    downloaded_bytes: u64,
    uploaded_bytes: u64,
    seed_ratio: Option<f64>,
    seed_time_minutes: Option<u64>,
    seeding_started_at: Instant,
    now: Instant,
) -> bool {
    if let Some(ratio) = seed_ratio {
        if downloaded_bytes > 0 && (uploaded_bytes as f64 / downloaded_bytes as f64) >= ratio {
            return true;
        }
    }
    if let Some(minutes) = seed_time_minutes {
        if now.duration_since(seeding_started_at) >= Duration::from_secs(minutes.saturating_mul(60))
        {
            return true;
        }
    }
    false
}

fn should_stop_idle_bt_download(
    download_speed: u64,
    idle_timeout_seconds: Option<u64>,
    last_download_activity_at: &mut Instant,
    now: Instant,
) -> bool {
    let Some(timeout_seconds) = idle_timeout_seconds else {
        return false;
    };
    if timeout_seconds == 0 {
        return false;
    }
    if download_speed > 0 {
        *last_download_activity_at = now;
        return false;
    }
    now.duration_since(*last_download_activity_at) >= Duration::from_secs(timeout_seconds)
}

fn derive_bt_web_seed_uris(job: &Job, primary_uri: &str) -> Option<Vec<String>> {
    let mut uris = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();

    let mut maybe_push = |candidate: &str| {
        let trimmed = candidate.trim();
        if trimmed.is_empty() || trimmed == primary_uri {
            return;
        }

        let Ok(parsed) = url::Url::parse(trimmed) else {
            return;
        };
        let scheme = parsed.scheme();
        if !matches!(scheme, "http" | "https" | "ftp" | "ftps" | "sftp") {
            return;
        }

        if seen.insert(trimmed.to_string()) {
            uris.push(trimmed.to_string());
        }
    };

    if let Some(explicit) = job.options.bt_web_seed_uris.as_ref() {
        for uri in explicit {
            maybe_push(uri);
        }
    }

    for uri in &job.uris {
        maybe_push(uri);
    }

    (!uris.is_empty()).then_some(uris)
}

fn selected_files_changed(current: Option<&[usize]>, next: &[usize]) -> bool {
    current
        .map(|current| {
            current.len() != next.len() || current.iter().any(|file| !next.contains(file))
        })
        .unwrap_or(!next.is_empty())
}

fn is_remote_torrent_metadata_uri(uri: &str) -> bool {
    url::Url::parse(uri)
        .map(|parsed| {
            matches!(parsed.scheme(), "http" | "https")
                && parsed.path().to_ascii_lowercase().ends_with(".torrent")
        })
        .unwrap_or(false)
}

async fn fetch_remote_torrent_metadata(uri: &str) -> Result<Vec<u8>> {
    let response = reqwest::Client::new()
        .get(uri)
        .send()
        .await
        .with_context(|| format!("failed to fetch torrent metadata from {uri}"))?
        .error_for_status()
        .with_context(|| format!("torrent metadata request failed for {uri}"))?;
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("failed to read torrent metadata from {uri}"))?;
    Ok(bytes.to_vec())
}

pub(crate) async fn run_bt_download(
    engine: Arc<Engine>,
    task_id: TaskId,
    cancel: CancellationToken,
    bt_service: Arc<BtService>,
) -> Result<()> {
    let gid = engine
        .gid_for_task_id(&task_id)
        .context("BT task runtime bridge missing")?;
    let job = engine
        .registry
        .get(gid)
        .context("BT job not found in registry")?;

    let uri_str = job.uris.first().context("BT job has no URIs")?;
    info!(%gid, "daemon: starting BT download");
    emit_structured_log(
        "INFO",
        "raria::bt",
        "daemon: starting BT download",
        [("task_id", task_id.to_string())],
    );

    let source = if uri_str.starts_with("magnet:") {
        BtSource::Magnet(uri_str.clone())
    } else if let Some(b64) = uri_str.strip_prefix("torrent:base64:") {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .context("failed to decode torrent base64")?;
        BtSource::TorrentBytes(bytes)
    } else if is_remote_torrent_metadata_uri(uri_str) {
        BtSource::TorrentBytes(fetch_remote_torrent_metadata(uri_str).await?)
    } else {
        BtSource::TorrentFile(PathBuf::from(uri_str))
    };

    let web_seed_uris = derive_bt_web_seed_uris(&job, uri_str);
    let selected_files_for_cleanup = job.options.bt_selected_files.clone();
    let delete_unselected_files_on_completion =
        job.options.bt_delete_unselected_files_on_completion;
    let torrent_bytes_opt = match &source {
        BtSource::TorrentBytes(bytes) => Some(bytes.clone()),
        BtSource::TorrentFile(path) => match std::fs::read(path) {
            Ok(b) => Some(b),
            Err(e) => {
                warn!(%gid, error = %e, "could not read torrent file for metadata-dependent BT preprocessing");
                None
            }
        },
        BtSource::Magnet(_) => {
            // Magnet URIs don't carry torrent metadata until resolution.
            None
        }
    };

    // WebSeed pre-download: if URIs are available and we have torrent bytes,
    // download files via HTTP/FTP/SFTP before librqbit starts so that its
    // initial_check discovers them as already-complete pieces on disk.
    if let Some(ws_uris) = &web_seed_uris {
        if let Some(torrent_bytes) = &torrent_bytes_opt {
            match TorrentMeta::from_bytes(torrent_bytes) {
                Ok(mut meta) => {
                    meta.merge_web_seed_uris(ws_uris);
                    if !meta.web_seed_uris.is_empty() {
                        let ws_config = raria_bt::webseed::WebSeedConfig {
                            max_connections: 4,
                            timeout: Duration::from_secs(60),
                            cancel: cancel.clone(),
                        };
                        let output_dir = bt_service.output_dir().clone();
                        info!(%gid, uris = meta.web_seed_uris.len(), "starting WebSeed pre-download");
                        match raria_bt::webseed::pre_download(&meta, &output_dir, &ws_config).await
                        {
                            Ok(result) => {
                                info!(
                                    %gid,
                                    verified = result.pieces_verified,
                                    failed = result.pieces_failed,
                                    bytes = result.bytes_downloaded,
                                    "WebSeed pre-download complete"
                                );
                            }
                            Err(e) => {
                                warn!(%gid, error = %e, "WebSeed pre-download failed, continuing with BT only");
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(%gid, error = %e, "failed to parse torrent for WebSeed, skipping pre-download");
                }
            }
        }
    }

    if job.options.bt_metadata_only {
        let metadata = bt_service
            .inspect_metadata(
                source,
                job.options.bt_selected_files.clone(),
                job.options.bt_trackers.clone(),
            )
            .await
            .context("failed to inspect BT metadata")?;
        sync_bt_job_from_metadata(engine.as_ref(), gid, &metadata)?;
        engine.pause_native_task(&task_id)?;
        persist_bt_job(engine.as_ref(), gid);
        return Ok(());
    }

    if uri_str.starts_with("magnet:") {
        match bt_service
            .inspect_metadata(
                source.clone(),
                job.options.bt_selected_files.clone(),
                job.options.bt_trackers.clone(),
            )
            .await
        {
            Ok(metadata) => {
                sync_bt_job_from_metadata(engine.as_ref(), gid, &metadata)?;
                persist_bt_job(engine.as_ref(), gid);
            }
            Err(error) => {
                debug_assert!(
                    error.to_string().contains("metadata-only"),
                    "unexpected magnet metadata inspection error: {error:#}"
                );
            }
        }
    }

    let handle = bt_service
        .add(
            source,
            gid,
            job.options.bt_selected_files.clone(),
            job.options.bt_trackers.clone(),
            web_seed_uris.is_some(),
        )
        .await
        .context("failed to add torrent to BtService")?;

    info!(%gid, torrent_id = handle.torrent_id, "BT download started");
    emit_structured_log(
        "INFO",
        "raria::bt",
        "BT download started",
        [
            ("task_id", task_id.to_string()),
            ("torrent_id", handle.torrent_id.to_string()),
        ],
    );
    let mut seeding_started_at: Option<Instant> = None;
    let mut last_download_activity_at = Instant::now();
    let mut applied_selected_files = job.options.bt_selected_files.clone();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!(%gid, "BT download cancelled");
                emit_structured_log(
                    "INFO",
                    "raria::bt",
                    "BT download cancelled",
                    [("task_id", task_id.to_string())],
                );
                let _ = bt_service.pause(&handle).await;
                handle_bt_cancellation(engine.as_ref(), gid);
                return Ok(());
            }
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
                let next_selected_files = engine
                    .registry
                    .get(gid)
                    .and_then(|job| job.options.bt_selected_files.clone());
                if let Some(next_selected_files) = next_selected_files.as_ref() {
                    if selected_files_changed(applied_selected_files.as_deref(), next_selected_files) {
                        bt_service
                            .update_selected_files(&handle, next_selected_files)
                            .await
                            .with_context(|| {
                                format!("failed to update selected BT files for GID {gid}")
                            })?;
                        applied_selected_files = Some(next_selected_files.clone());
                    }
                }
                match bt_service.status(&handle).await {
                    Ok(status) => {
                        let bt_files = bt_service.file_list(&handle).await.ok().map(map_bt_files);
                        let bt_peers = bt_service.peer_list(&handle).await.ok().map(map_bt_peers);
                        let outcome = sync_bt_job_from_status(
                            engine.as_ref(),
                            gid,
                            &status,
                            bt_files,
                            bt_peers,
                        )?;
                        persist_bt_job(engine.as_ref(), gid);

                        match outcome.completion_action {
                            BtCompletionAction::EnterSeeding => {
                                seeding_started_at.get_or_insert_with(Instant::now);
                                info!(%gid, "BT payload complete; entering seeding");
                                emit_structured_log(
                                    "INFO",
                                    "raria::bt",
                                    "BT payload complete; entering seeding",
                                    [("task_id", task_id.to_string())],
                                );
                                engine
                                    .event_bus
                                    .publish(DownloadEvent::BtDownloadComplete { gid });
                            }
                            BtCompletionAction::Complete => {
                                if delete_unselected_files_on_completion {
                                    if let (Some(torrent_bytes), Some(selected_files)) =
                                        (torrent_bytes_opt.as_deref(), selected_files_for_cleanup.as_deref())
                                    {
                                        cleanup_unselected_bt_files(
                                            bt_service.output_dir(),
                                            torrent_bytes,
                                            selected_files,
                                        )
                                        .await?;
                                    }
                                }
                                engine.complete_job(gid)?;
                                return Ok(());
                            }
                            BtCompletionAction::None => {}
                        }

                        if !status.is_complete
                            && should_stop_idle_bt_download(
                                status.download_speed,
                                outcome.idle_download_timeout,
                                &mut last_download_activity_at,
                                Instant::now(),
                            )
                        {
                            engine.fail_job(
                                gid,
                                "BitTorrent download stopped after configured idle timeout",
                            )?;
                            return Ok(());
                        }

                        if status.is_complete
                            && engine
                                .registry
                                .get(gid)
                                .map(|job| job.status == Status::Seeding)
                                .unwrap_or(false)
                        {
                            let now = Instant::now();
                            let started = seeding_started_at.get_or_insert(now);
                            if should_stop_seeding(
                                status.downloaded,
                                status.uploaded,
                                outcome.seed_ratio,
                                outcome.seed_time,
                                *started,
                                now,
                            ) {
                                engine.complete_job(gid)?;
                                return Ok(());
                            }
                        }
                    }
                    Err(error) => {
                        warn!(%gid, error = %error, "BT status check failed");
                        emit_structured_log(
                            "WARN",
                            "raria::bt",
                            "BT status check failed",
                            [("task_id", task_id.to_string()), ("error", error.to_string())],
                        );
                        let _ = engine.fail_job(gid, &error.to_string());
                        return Ok(());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BtCompletionAction, bt_service_config, cleanup_unselected_bt_files,
        derive_bt_web_seed_uris, handle_bt_cancellation, map_bt_files, map_bt_peers,
        native_bt_session_persistence_dir, selected_files_changed, should_stop_idle_bt_download,
        should_stop_seeding, sync_bt_job_from_status, sync_bt_status_into_job,
    };
    use crate::bt_runtime::PieceSelectionStrategy;
    use librqbit::{CreateTorrentOptions, create_torrent};
    use raria_bt::service::{BtFileInfo, BtMetadata, BtMetadataFile, BtPeerInfo, BtStatus};
    use raria_core::config::{BtPieceStrategy, GlobalConfig, JobOptions};
    use raria_core::engine::{AddUriSpec, Engine};
    use raria_core::job::{BtPeer, BtSnapshot, Job, Status};
    use raria_core::native::{
        NativeEventData, NativeEventType, NativePeerSnapshot, NativeTrackerSnapshot,
    };
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn insert_active_bt_job(engine: &Engine, options: JobOptions) -> raria_core::job::Gid {
        let mut job = Job::new_bt_with_options(
            vec!["magnet:?xt=urn:btih:feedface".into()],
            PathBuf::from("/tmp/bt-fixture"),
            options,
        );
        let gid = job.gid;
        job.status = Status::Active;
        engine.registry.insert(job).expect("insert bt job");
        gid
    }

    fn sample_bt_status() -> BtStatus {
        BtStatus {
            total_size: 4096,
            downloaded: 2048,
            uploaded: 512,
            download_speed: 128,
            upload_speed: 64,
            num_peers: 3,
            num_seeders: 2,
            is_complete: false,
            info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
            torrent_name: Some("fixture.iso".into()),
            announce_list: Some(vec!["http://tracker.example/announce".into()]),
            piece_length: Some(1024),
            num_pieces: Some(4),
        }
    }

    #[test]
    fn bt_file_info_maps_to_core_bt_file() {
        let files = vec![BtFileInfo {
            index: 2,
            path: PathBuf::from("disc/file.bin"),
            size: 1234,
            completed_length: 321,
            selected: true,
        }];

        let mapped = map_bt_files(files);
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].index, 2);
        assert_eq!(mapped[0].path, PathBuf::from("disc/file.bin"));
        assert_eq!(mapped[0].length, 1234);
        assert_eq!(mapped[0].completed_length, 321);
        assert!(mapped[0].selected);
    }

    #[test]
    fn bt_peer_info_maps_to_core_bt_peer() {
        let peers = vec![BtPeerInfo {
            addr: "127.0.0.1:6881".into(),
            ip: "127.0.0.1".into(),
            port: 6881,
            download_speed: 123,
            upload_speed: 0,
            seeder: true,
        }];

        let mapped = map_bt_peers(peers);
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].addr, "127.0.0.1:6881");
        assert_eq!(mapped[0].ip, "127.0.0.1");
        assert_eq!(mapped[0].port, 6881);
        assert_eq!(mapped[0].download_speed, 123);
        assert!(mapped[0].seeder);
    }

    #[test]
    fn sync_bt_metadata_populates_native_job_state_without_payload_progress() {
        let engine = Engine::new(GlobalConfig::default());
        let gid = insert_active_bt_job(&engine, JobOptions::default());
        let metadata = BtMetadata {
            info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
            torrent_name: Some("metadata-fixture".into()),
            total_size: 4096,
            piece_length: 1024,
            num_pieces: 4,
            files: vec![BtMetadataFile {
                index: 0,
                path: PathBuf::from("fixture.bin"),
                size: 4096,
                selected: true,
            }],
        };

        super::sync_bt_job_from_metadata(&engine, gid, &metadata).expect("sync BT metadata");

        let job = engine.registry.get(gid).expect("job");
        assert_eq!(job.downloaded, 0);
        assert_eq!(job.total_size, Some(4096));
        assert_eq!(
            job.bt_files.as_ref().expect("files")[0].path,
            PathBuf::from("fixture.bin")
        );
        let bt = job.bt.expect("BT snapshot");
        assert_eq!(bt.info_hash.as_deref(), Some(metadata.info_hash.as_str()));
        assert_eq!(bt.torrent_name.as_deref(), Some("metadata-fixture"));
        assert_eq!(bt.piece_length, Some(1024));
        assert_eq!(bt.num_pieces, Some(4));
    }

    #[test]
    fn bt_service_config_forwards_only_socks5_proxy() {
        let config = GlobalConfig {
            proxy: Some("socks5://127.0.0.1:1080".into()),
            ..Default::default()
        };
        let engine = Engine::new(config);
        let bt_config = bt_service_config(&engine);
        assert_eq!(
            bt_config.socks_proxy_url.as_deref(),
            Some("socks5://127.0.0.1:1080")
        );

        let config = GlobalConfig {
            proxy: Some("http://127.0.0.1:8080".into()),
            ..Default::default()
        };
        let engine = Engine::new(config);
        let bt_config = bt_service_config(&engine);
        assert!(bt_config.socks_proxy_url.is_none());
    }

    #[test]
    fn bt_service_config_forwards_piece_strategy() {
        let engine = Engine::new(GlobalConfig {
            bt_piece_strategy: BtPieceStrategy::RarestFirst,
            ..Default::default()
        });
        let bt_config = bt_service_config(&engine);
        assert_eq!(
            bt_config.piece_selection_strategy,
            PieceSelectionStrategy::RarestFirst
        );
    }

    #[test]
    fn bt_service_config_forwards_native_pex_policy() {
        let engine = Engine::new(GlobalConfig {
            bt_enable_pex: false,
            ..Default::default()
        });
        let bt_config = bt_service_config(&engine);

        assert!(!bt_config.enable_pex);
    }

    #[test]
    fn bt_service_config_binds_fastresume_to_native_session_path() {
        let session_file = PathBuf::from("/tmp/raria-fixture.session.redb");
        let engine = Engine::new(GlobalConfig {
            session_file: session_file.clone(),
            ..Default::default()
        });
        let bt_config = bt_service_config(&engine);

        assert_eq!(
            bt_config.session_persistence_dir,
            Some(PathBuf::from("/tmp/raria-fixture.session.redb.bt-session"))
        );
        assert_eq!(
            native_bt_session_persistence_dir(&session_file),
            PathBuf::from("/tmp/raria-fixture.session.redb.bt-session")
        );
    }

    #[test]
    fn derive_bt_web_seed_uris_merges_explicit_and_job_uri_candidates() {
        let mut job = Job::new_bt(
            vec![
                "magnet:?xt=urn:btih:feedface".into(),
                "https://job.example/mirror.iso".into(),
                "http://job.example/fallback.iso".into(),
                "https://job.example/mirror.iso".into(),
                "udp://tracker.example/announce".into(),
            ],
            PathBuf::from("/tmp/downloads"),
        );
        job.options.bt_web_seed_uris = Some(vec![
            "https://explicit.example/seed.iso".into(),
            "ftp://explicit.example/seed.iso".into(),
            "ftps://explicit.example/secure.iso".into(),
            "not-a-uri".into(),
            "https://job.example/mirror.iso".into(),
        ]);

        let derived = derive_bt_web_seed_uris(&job, "magnet:?xt=urn:btih:feedface")
            .expect("should derive mixed-source seed URIs");
        assert_eq!(
            derived,
            vec![
                "https://explicit.example/seed.iso".to_string(),
                "ftp://explicit.example/seed.iso".to_string(),
                "ftps://explicit.example/secure.iso".to_string(),
                "https://job.example/mirror.iso".to_string(),
                "http://job.example/fallback.iso".to_string(),
            ]
        );
    }

    #[test]
    fn derive_bt_web_seed_uris_returns_none_without_aux_sources() {
        let job = Job::new_bt(
            vec!["magnet:?xt=urn:btih:feedface".into()],
            PathBuf::from("/tmp/downloads"),
        );
        assert!(
            derive_bt_web_seed_uris(&job, "magnet:?xt=urn:btih:feedface").is_none(),
            "no auxiliary URI should produce no derived web-seed list"
        );
    }

    #[test]
    fn remote_torrent_metadata_detection_is_limited_to_http_torrent_uris() {
        assert!(super::is_remote_torrent_metadata_uri(
            "https://metadata.example/file.iso.torrent"
        ));
        assert!(super::is_remote_torrent_metadata_uri(
            "http://metadata.example/file.iso.torrent?token=abc"
        ));
        assert!(!super::is_remote_torrent_metadata_uri(
            "/tmp/file.iso.torrent"
        ));
        assert!(!super::is_remote_torrent_metadata_uri(
            "ftp://metadata.example/file.iso.torrent"
        ));
        assert!(!super::is_remote_torrent_metadata_uri(
            "https://metadata.example/file.iso"
        ));
    }

    #[test]
    fn selected_files_changed_uses_set_semantics_for_live_bt_updates() {
        assert!(!selected_files_changed(Some(&[1, 3]), &[3, 1]));
        assert!(selected_files_changed(Some(&[1, 3]), &[1]));
        assert!(selected_files_changed(None, &[1]));
        assert!(!selected_files_changed(None, &[]));
    }

    #[tokio::test]
    async fn cleanup_unselected_bt_files_removes_only_unselected_paths() {
        let source = tempfile::tempdir().expect("source tempdir");
        let payload_dir = source.path().join("payload");
        std::fs::create_dir(&payload_dir).expect("create payload dir");
        std::fs::write(payload_dir.join("file-a.bin"), b"aaaa").expect("write selected payload");
        std::fs::write(payload_dir.join("file-b.bin"), b"bbbb").expect("write unselected payload");
        let torrent = create_torrent(
            &payload_dir,
            CreateTorrentOptions {
                name: Some("payload"),
                piece_length: Some(4),
            },
        )
        .await
        .expect("create torrent");

        let output = tempfile::tempdir().expect("output tempdir");
        std::fs::write(output.path().join("file-a.bin"), b"aaaa").expect("write selected output");
        std::fs::write(output.path().join("file-b.bin"), b"bbbb").expect("write unselected output");

        cleanup_unselected_bt_files(
            output.path(),
            &torrent.as_bytes().expect("torrent bytes"),
            &[1],
        )
        .await
        .expect("cleanup unselected files");

        assert!(output.path().join("file-a.bin").is_file());
        assert!(!output.path().join("file-b.bin").exists());
    }

    #[test]
    fn sync_bt_job_from_status_populates_bt_snapshot_fields() {
        let engine = Engine::new(GlobalConfig::default());
        let gid = insert_active_bt_job(&engine, JobOptions::default());
        let task_id = engine.task_id_for_gid(gid).expect("native task id");
        let mut events = engine.native_event_bus.subscribe();

        let outcome = sync_bt_job_from_status(&engine, gid, &sample_bt_status(), None, None)
            .expect("sync bt status");
        assert_eq!(outcome.completion_action, BtCompletionAction::None);

        let job = engine.registry.get(gid).expect("job in registry");
        let bt = job.bt.expect("bt snapshot should exist");
        assert_eq!(job.total_size, Some(4096));
        assert_eq!(job.downloaded, 2048);
        assert_eq!(job.upload_speed, 64);
        assert_eq!(
            bt.info_hash.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert_eq!(bt.torrent_name.as_deref(), Some("fixture.iso"));
        assert_eq!(
            bt.announce_list.as_deref(),
            Some(&["http://tracker.example/announce".to_string()][..])
        );
        assert_eq!(bt.uploaded, Some(512));
        assert_eq!(bt.num_seeders, Some(2));
        assert_eq!(bt.piece_length, Some(1024));
        assert_eq!(bt.num_pieces, Some(4));

        let event = events.try_recv().expect("native metadata event");
        assert_eq!(event.task_id.as_ref(), Some(&task_id));
        assert_eq!(event.event_type, NativeEventType::TaskBtMetadataResolved);
        assert_eq!(
            event.data,
            NativeEventData::BtMetadata {
                info_hash: "0123456789abcdef0123456789abcdef01234567".to_string(),
                name: Some("fixture.iso".to_string()),
                total_bytes: Some(4096),
                piece_length: Some(1024),
                piece_count: Some(4),
            }
        );
    }

    #[test]
    fn sync_bt_job_from_status_enters_seeding_once_when_seed_controls_exist() {
        let engine = Engine::new(GlobalConfig::default());
        let gid = insert_active_bt_job(
            &engine,
            JobOptions {
                seed_ratio: Some(1.5),
                ..Default::default()
            },
        );
        let task_id = engine.task_id_for_gid(gid).expect("native task id");
        let mut events = engine.native_event_bus.subscribe();

        let mut status = sample_bt_status();
        status.is_complete = true;
        status.downloaded = status.total_size;

        let first = sync_bt_job_from_status(&engine, gid, &status, None, None)
            .expect("first bt sync should succeed");
        assert_eq!(first.completion_action, BtCompletionAction::EnterSeeding);

        let first_job = engine.registry.get(gid).expect("job in registry");
        assert_eq!(first_job.status, Status::Seeding);
        assert!(first_job.bt_download_complete_emitted());
        let _metadata = events.try_recv().expect("native metadata event");
        let seeding = events.try_recv().expect("native seeding event");
        assert_eq!(seeding.task_id.as_ref(), Some(&task_id));
        assert_eq!(seeding.event_type, NativeEventType::TaskBtSeedingStarted);
        assert_eq!(
            seeding.data,
            NativeEventData::BtSeeding {
                uploaded_bytes: 512,
                peer_count: 3,
                seeder_count: Some(2),
            }
        );

        let second = sync_bt_job_from_status(&engine, gid, &status, None, None)
            .expect("second bt sync should succeed");
        assert_eq!(second.completion_action, BtCompletionAction::None);
    }

    #[tokio::test]
    async fn sync_bt_job_from_status_notifies_scheduler_when_entering_seeding() {
        let engine = Engine::new(GlobalConfig::default());
        let gid = insert_active_bt_job(
            &engine,
            JobOptions {
                seed_ratio: Some(1.5),
                ..Default::default()
            },
        );
        let work_notify = engine.work_notify();

        let mut status = sample_bt_status();
        status.is_complete = true;
        status.downloaded = status.total_size;

        let outcome = sync_bt_job_from_status(&engine, gid, &status, None, None)
            .expect("bt sync should enter seeding");

        assert_eq!(outcome.completion_action, BtCompletionAction::EnterSeeding);
        tokio::time::timeout(std::time::Duration::from_secs(1), work_notify.notified())
            .await
            .expect("seeding transition should wake scheduler");
    }

    #[test]
    fn sync_bt_job_from_status_publishes_native_peer_and_tracker_events() {
        let engine = Engine::new(GlobalConfig::default());
        let gid = insert_active_bt_job(&engine, JobOptions::default());
        let task_id = engine.task_id_for_gid(gid).expect("native task id");
        let mut events = engine.native_event_bus.subscribe();

        let peers = vec![BtPeer {
            addr: "203.0.113.7:6881".to_string(),
            ip: "203.0.113.7".to_string(),
            port: 6881,
            download_speed: 1024,
            upload_speed: 256,
            seeder: true,
        }];
        sync_bt_job_from_status(&engine, gid, &sample_bt_status(), None, Some(peers))
            .expect("sync bt status");

        let _metadata = events.try_recv().expect("native metadata event");
        let peer = events.try_recv().expect("native peer event");
        assert_eq!(peer.task_id.as_ref(), Some(&task_id));
        assert_eq!(peer.event_type, NativeEventType::TaskBtPeerUpdated);
        assert_eq!(
            peer.data,
            NativeEventData::BtPeer {
                peer: NativePeerSnapshot {
                    id: "peer_203.0.113.7_6881".to_string(),
                    ip: "203.0.113.7".to_string(),
                    port: 6881,
                    download_bytes_per_second: 1024,
                    upload_bytes_per_second: 256,
                    seeder: true,
                },
            }
        );

        let tracker = events.try_recv().expect("native tracker event");
        assert_eq!(tracker.task_id.as_ref(), Some(&task_id));
        assert_eq!(tracker.event_type, NativeEventType::TaskBtTrackerUpdated);
        assert_eq!(
            tracker.data,
            NativeEventData::BtTracker {
                tracker: NativeTrackerSnapshot::new("tracker_0", "http://tracker.example/announce",),
            }
        );
    }

    #[test]
    fn sync_bt_job_from_status_requests_direct_completion_without_seed_controls() {
        let engine = Engine::new(GlobalConfig::default());
        let gid = insert_active_bt_job(&engine, JobOptions::default());

        let mut status = sample_bt_status();
        status.is_complete = true;
        status.downloaded = status.total_size;

        let outcome =
            sync_bt_job_from_status(&engine, gid, &status, None, None).expect("sync bt status");
        assert_eq!(outcome.completion_action, BtCompletionAction::Complete);

        let job = engine.registry.get(gid).expect("job in registry");
        assert_eq!(job.status, Status::Active);
        assert!(!job.bt_download_complete_emitted());
    }

    #[test]
    fn sync_bt_status_into_job_preserves_existing_announce_list_when_status_lacks_one() {
        let mut job = Job::new_bt(
            vec!["magnet:?xt=urn:btih:sync-fields".into()],
            PathBuf::from("/tmp/downloads"),
        );
        job.bt = Some(BtSnapshot {
            announce_list: Some(vec!["http://existing.example/announce".into()]),
            ..Default::default()
        });

        let mut status = sample_bt_status();
        status.announce_list = None;

        let outcome =
            sync_bt_status_into_job(&mut job, &status, None, None).expect("sync into raw job");
        assert_eq!(outcome.completion_action, BtCompletionAction::None);

        let bt = job.bt.as_ref().expect("bt snapshot");
        assert_eq!(
            bt.announce_list.as_ref(),
            Some(&vec!["http://existing.example/announce".into()])
        );
    }

    #[test]
    fn seeding_stops_when_ratio_reached() {
        let started = Instant::now();
        assert!(should_stop_seeding(
            100,
            150,
            Some(1.5),
            None,
            started,
            started + Duration::from_secs(1),
        ));
    }

    #[test]
    fn seeding_stops_when_time_reached() {
        let started = Instant::now();
        assert!(should_stop_seeding(
            100,
            10,
            None,
            Some(1),
            started,
            started + Duration::from_secs(60),
        ));
    }

    #[test]
    fn bt_idle_timeout_stops_incomplete_download_after_zero_speed_window() {
        let mut last_activity = Instant::now() - Duration::from_secs(8);
        assert!(should_stop_idle_bt_download(
            0,
            Some(7),
            &mut last_activity,
            Instant::now()
        ));
    }

    #[test]
    fn bt_idle_timeout_resets_when_download_speed_recovers() {
        let mut last_activity = Instant::now() - Duration::from_secs(8);
        let now = Instant::now();
        assert!(!should_stop_idle_bt_download(
            128,
            Some(7),
            &mut last_activity,
            now
        ));
        assert_eq!(last_activity, now);
    }

    #[test]
    fn bt_cancel_handler_preserves_paused_status() {
        let engine = Engine::new(GlobalConfig::default());
        let handle = engine
            .add_uri(&AddUriSpec {
                uris: vec!["magnet:?xt=urn:btih:abc123".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("torrent".into()),
                connections: 1,
                headers: Vec::new(),
                http_user: None,
                http_password: None,
                checksum: None,
            })
            .unwrap();
        engine.pause(handle.gid).unwrap();

        handle_bt_cancellation(&engine, handle.gid);

        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(job.status, Status::Paused);
    }

    #[test]
    fn bt_cancel_handler_does_not_force_active_job_into_error() {
        let engine = Engine::new(GlobalConfig::default());
        let handle = engine
            .add_uri(&AddUriSpec {
                uris: vec!["magnet:?xt=urn:btih:def456".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("torrent".into()),
                connections: 1,
                headers: Vec::new(),
                http_user: None,
                http_password: None,
                checksum: None,
            })
            .unwrap();
        engine.activate_job(handle.gid).unwrap();

        handle_bt_cancellation(&engine, handle.gid);

        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(job.status, Status::Active);
    }
}
