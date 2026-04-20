use anyhow::{Context, Result};
use base64::Engine as Base64Engine;
use raria_bt::service::{
    BtService, BtServiceConfig, BtSource, BtStatus, PeerEncryptionMinLevel, PeerEncryptionMode,
    PeerEncryptionPolicy, PieceSelectionStrategy,
};
use raria_core::config::{BtMinCryptoLevel, BtPieceStrategy};
use raria_core::engine::Engine;
use raria_core::job::{BtCompletionDisposition, BtFile, BtPeer, Gid, Job, Status};
use raria_core::logging::emit_structured_log;
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
            .all_proxy
            .clone()
            .filter(|proxy| proxy.starts_with("socks5://")),
        dht_config_filename: engine.config.bt_dht_config_file.clone(),
        piece_selection_strategy: match engine.config.bt_piece_strategy {
            BtPieceStrategy::Current => PieceSelectionStrategy::Current,
            BtPieceStrategy::RarestFirst => PieceSelectionStrategy::RarestFirst,
        },
        peer_encryption_policy: PeerEncryptionPolicy {
            mode: if engine.config.bt_require_crypto {
                PeerEncryptionMode::Require
            } else {
                PeerEncryptionMode::Prefer
            },
            min_crypto_level: match engine.config.bt_min_crypto_level {
                BtMinCryptoLevel::Plain => PeerEncryptionMinLevel::Plain,
                BtMinCryptoLevel::Arc4 => PeerEncryptionMinLevel::Arc4,
            },
        },
        ..Default::default()
    }
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
    })
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
}

fn persist_bt_job(engine: &Engine, gid: Gid) {
    if let Some(store) = engine.store() {
        if let Some(job) = engine.registry.get(gid) {
            if let Err(error) = store.put_job(&job) {
                warn!(%gid, error = %error, "failed to persist BT job");
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

pub(crate) async fn run_bt_download(
    engine: Arc<Engine>,
    gid: Gid,
    cancel: CancellationToken,
    bt_service: Arc<BtService>,
) -> Result<()> {
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
        [("gid", gid.to_string())],
    );

    let source = if uri_str.starts_with("magnet:") {
        BtSource::Magnet(uri_str.clone())
    } else if let Some(b64) = uri_str.strip_prefix("torrent:base64:") {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .context("failed to decode torrent base64")?;
        BtSource::TorrentBytes(bytes)
    } else {
        BtSource::TorrentFile(PathBuf::from(uri_str))
    };

    let web_seed_uris = derive_bt_web_seed_uris(&job, uri_str);

    // WebSeed pre-download: if URIs are available and we have torrent bytes,
    // download files via HTTP/FTP/SFTP before librqbit starts so that its
    // initial_check discovers them as already-complete pieces on disk.
    if let Some(ws_uris) = &web_seed_uris {
        let torrent_bytes_opt = match &source {
            BtSource::TorrentBytes(bytes) => Some(bytes.clone()),
            BtSource::TorrentFile(path) => match std::fs::read(path) {
                Ok(b) => Some(b),
                Err(e) => {
                    warn!(%gid, error = %e, "could not read torrent file for WebSeed, skipping pre-download");
                    None
                }
            },
            BtSource::Magnet(_) => {
                // Magnet URIs don't carry torrent metadata yet — skip WebSeed.
                None
            }
        };

        if let Some(torrent_bytes) = torrent_bytes_opt {
            match raria_bt::torrent_meta::TorrentMeta::from_bytes(&torrent_bytes) {
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

    let handle = bt_service
        .add(
            source,
            gid,
            job.options.bt_selected_files.clone(),
            job.options.bt_trackers.clone(),
        )
        .await
        .context("failed to add torrent to BtService")?;

    info!(%gid, torrent_id = handle.torrent_id, "BT download started");
    emit_structured_log(
        "INFO",
        "raria::bt",
        "BT download started",
        [
            ("gid", gid.to_string()),
            ("torrent_id", handle.torrent_id.to_string()),
        ],
    );
    let mut seeding_started_at: Option<Instant> = None;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!(%gid, "BT download cancelled");
                emit_structured_log(
                    "INFO",
                    "raria::bt",
                    "BT download cancelled",
                    [("gid", gid.to_string())],
                );
                let _ = bt_service.pause(&handle).await;
                handle_bt_cancellation(engine.as_ref(), gid);
                return Ok(());
            }
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
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
                                    [("gid", gid.to_string())],
                                );
                                engine
                                    .event_bus
                                    .publish(DownloadEvent::BtDownloadComplete { gid });
                            }
                            BtCompletionAction::Complete => {
                                engine.complete_job(gid)?;
                                return Ok(());
                            }
                            BtCompletionAction::None => {}
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
                            [("gid", gid.to_string()), ("error", error.to_string())],
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
        BtCompletionAction, bt_service_config, derive_bt_web_seed_uris, handle_bt_cancellation,
        map_bt_files, map_bt_peers, should_stop_seeding, sync_bt_job_from_status,
        sync_bt_status_into_job,
    };
    use crate::bt_runtime::PieceSelectionStrategy;
    use raria_bt::service::{
        BtFileInfo, BtPeerInfo, BtStatus, PeerEncryptionMinLevel, PeerEncryptionMode,
        PeerEncryptionPolicy,
    };
    use raria_core::config::{BtMinCryptoLevel, BtPieceStrategy, GlobalConfig, JobOptions};
    use raria_core::engine::{AddUriSpec, Engine};
    use raria_core::job::{BtSnapshot, Job, Status};
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
    fn bt_service_config_forwards_only_socks5_all_proxy() {
        let config = GlobalConfig {
            all_proxy: Some("socks5://127.0.0.1:1080".into()),
            ..Default::default()
        };
        let engine = Engine::new(config);
        let bt_config = bt_service_config(&engine);
        assert_eq!(
            bt_config.socks_proxy_url.as_deref(),
            Some("socks5://127.0.0.1:1080")
        );

        let config = GlobalConfig {
            all_proxy: Some("http://127.0.0.1:8080".into()),
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
    fn bt_service_config_forwards_bt_crypto_policy() {
        let engine = Engine::new(GlobalConfig {
            bt_require_crypto: true,
            bt_min_crypto_level: BtMinCryptoLevel::Arc4,
            ..Default::default()
        });
        let bt_config = bt_service_config(&engine);
        assert_eq!(
            bt_config.peer_encryption_policy,
            PeerEncryptionPolicy {
                mode: PeerEncryptionMode::Require,
                min_crypto_level: PeerEncryptionMinLevel::Arc4,
            }
        );

        let engine = Engine::new(GlobalConfig {
            bt_require_crypto: false,
            bt_min_crypto_level: BtMinCryptoLevel::Plain,
            ..Default::default()
        });
        let bt_config = bt_service_config(&engine);
        assert_eq!(
            bt_config.peer_encryption_policy,
            PeerEncryptionPolicy {
                mode: PeerEncryptionMode::Prefer,
                min_crypto_level: PeerEncryptionMinLevel::Plain,
            }
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
    fn sync_bt_job_from_status_populates_bt_snapshot_fields() {
        let engine = Engine::new(GlobalConfig::default());
        let gid = insert_active_bt_job(&engine, JobOptions::default());

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

        let mut status = sample_bt_status();
        status.is_complete = true;
        status.downloaded = status.total_size;

        let first = sync_bt_job_from_status(&engine, gid, &status, None, None)
            .expect("first bt sync should succeed");
        assert_eq!(first.completion_action, BtCompletionAction::EnterSeeding);

        let first_job = engine.registry.get(gid).expect("job in registry");
        assert_eq!(first_job.status, Status::Seeding);
        assert!(first_job.bt_download_complete_emitted());

        let second = sync_bt_job_from_status(&engine, gid, &status, None, None)
            .expect("second bt sync should succeed");
        assert_eq!(second.completion_action, BtCompletionAction::None);
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
    fn bt_cancel_handler_preserves_paused_status() {
        let engine = Engine::new(GlobalConfig::default());
        let handle = engine
            .add_uri(&AddUriSpec {
                uris: vec!["magnet:?xt=urn:btih:abc123".into()],
                dir: PathBuf::from("/tmp"),
                filename: Some("torrent".into()),
                connections: 1,
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
            })
            .unwrap();
        engine.activate_job(handle.gid).unwrap();

        handle_bt_cancellation(&engine, handle.gid);

        let job = engine.registry.get(handle.gid).unwrap();
        assert_eq!(job.status, Status::Active);
    }
}
