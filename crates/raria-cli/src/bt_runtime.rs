use anyhow::{Context, Result};
use base64::Engine as Base64Engine;
use raria_bt::service::{BtService, BtServiceConfig, BtSource};
use raria_core::engine::Engine;
use raria_core::job::Gid;
use raria_core::job::{BtCompletionDisposition, BtFile, BtPeer, Job, Status};
use raria_core::progress::DownloadEvent;
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
        ..Default::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BtLifecycleAction {
    None,
    EmitBtDownloadComplete,
    Complete,
}

fn sync_bt_status_into_job(
    job: &mut Job,
    status: &raria_bt::service::BtStatus,
    bt_files: Option<Vec<BtFile>>,
    bt_peers: Option<Vec<BtPeer>>,
) {
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
    if let Some(torrent_name) = &status.torrent_name {
        bt.torrent_name = Some(torrent_name.clone());
    }
    if let Some(announce_list) = &status.announce_list {
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
}

fn advance_bt_lifecycle(
    job: &mut Job,
    status: &raria_bt::service::BtStatus,
    seeding_started_at: &mut Option<Instant>,
    now: Instant,
) -> Result<BtLifecycleAction> {
    if !status.is_complete {
        return Ok(BtLifecycleAction::None);
    }

    let should_seed = job.options.seed_ratio.is_some() || job.options.seed_time.is_some();
    if !should_seed {
        if job.status == Status::Active {
            job.record_bt_download_complete(BtCompletionDisposition::Complete)
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            return Ok(BtLifecycleAction::Complete);
        }
        return Ok(BtLifecycleAction::None);
    }

    if job.status == Status::Active {
        job.record_bt_download_complete(BtCompletionDisposition::Seed)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        seeding_started_at.get_or_insert(now);
        return Ok(BtLifecycleAction::EmitBtDownloadComplete);
    }

    if job.status == Status::Seeding {
        let started = seeding_started_at.get_or_insert(now);
        if should_stop_seeding(
            status.downloaded,
            status.uploaded,
            job.options.seed_ratio,
            job.options.seed_time,
            *started,
            now,
        ) {
            job.transition(Status::Complete)
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            return Ok(BtLifecycleAction::Complete);
        }
    }

    Ok(BtLifecycleAction::None)
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

fn finish_bt_job(engine: &Engine, gid: Gid) {
    engine.cancel_registry.remove(gid);
    persist_bt_job(engine, gid);
    engine.event_bus.publish(DownloadEvent::Complete { gid });
    info!(%gid, "BT job completed");
    engine.work_notify().notify_one();
}

pub(crate) async fn run_bt_download(
    engine: Arc<Engine>,
    gid: Gid,
    cancel: CancellationToken,
    download_dir: PathBuf,
) -> Result<()> {
    let job = engine
        .registry
        .get(gid)
        .context("BT job not found in registry")?;

    let uri_str = job.uris.first().context("BT job has no URIs")?;
    info!(%gid, "daemon: starting BT download");

    let source = if uri_str.starts_with("magnet:") {
        BtSource::Magnet(uri_str.clone())
    } else if let Some(b64) = uri_str.strip_prefix("torrent:base64:") {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .context("failed to decode torrent base64")?;
        BtSource::TorrentBytes(bytes)
    } else {
        BtSource::TorrentFile(std::path::PathBuf::from(uri_str))
    };

    let bt_service = BtService::with_config(download_dir, bt_service_config(engine.as_ref()))
        .context("failed to create BtService")?;
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
    let mut seeding_started_at: Option<Instant> = None;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!(%gid, "BT download cancelled");
                let _ = bt_service.pause(&handle).await;
                handle_bt_cancellation(engine.as_ref(), gid);
                return Ok(());
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {
                match bt_service.status(&handle).await {
                    Ok(status) => {
                        let bt_files = bt_service.file_list(&handle).await.ok().map(map_bt_files);
                        let bt_peers = bt_service.peer_list(&handle).await.ok().map(map_bt_peers);
                        let lifecycle_action = engine
                            .registry
                            .update(gid, |job| -> Result<BtLifecycleAction> {
                                sync_bt_status_into_job(
                                    job,
                                    &status,
                                    bt_files.clone(),
                                    bt_peers.clone(),
                                );
                                advance_bt_lifecycle(
                                    job,
                                    &status,
                                    &mut seeding_started_at,
                                    Instant::now(),
                                )
                            })
                            .context("BT job disappeared during status sync")??;
                        persist_bt_job(engine.as_ref(), gid);

                        match lifecycle_action {
                            BtLifecycleAction::None => {}
                            BtLifecycleAction::EmitBtDownloadComplete => {
                                info!(%gid, "BT payload complete; entering seeding");
                                engine
                                    .event_bus
                                    .publish(DownloadEvent::BtDownloadComplete { gid });
                            }
                            BtLifecycleAction::Complete => {
                                finish_bt_job(engine.as_ref(), gid);
                                return Ok(());
                            }
                        }
                    }
                    Err(e) => {
                        warn!(%gid, error = %e, "BT status check failed");
                        let _ = engine.fail_job(gid, &e.to_string());
                        return Ok(());
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::{
        bt_service_config, handle_bt_cancellation, map_bt_files, map_bt_peers,
        reconcile_bt_completion, should_stop_seeding, sync_bt_status_into_job,
    };
    use raria_bt::service::{BtFileInfo, BtPeerInfo, BtStatus};
    use raria_core::config::GlobalConfig;
    use raria_core::engine::{AddUriSpec, Engine};
    use raria_core::job::Status;
    use raria_core::progress::DownloadEvent;
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

        let config = GlobalConfig {
            bt_dht_config_file: Some(PathBuf::from("/tmp/raria-dht.json")),
            ..Default::default()
        };
        let engine = Engine::new(config);
        let bt_config = bt_service_config(&engine);
        assert_eq!(
            bt_config.dht_config_filename,
            Some(PathBuf::from("/tmp/raria-dht.json"))
        );
    }

    #[test]
    fn sync_bt_job_from_status_populates_bt_snapshot_fields() {
        let engine = Engine::new(GlobalConfig::default());
        let gid = insert_active_bt_job(&engine, JobOptions::default());

        let outcome = sync_bt_job_from_status(
            &engine,
            gid,
            &raria_bt::service::BtStatus {
                total_size: 1_048_576,
                downloaded: 524_288,
                uploaded: 12_345,
                download_speed: 4_096,
                upload_speed: 512,
                num_peers: 3,
                num_seeders: 7,
                is_complete: false,
                info_hash: "abcdef1234567890".into(),
                torrent_name: Some("fixture.bin".into()),
                announce_list: Some(vec!["udp://tracker.example:80/announce".into()]),
                piece_length: 16_384,
                num_pieces: 64,
            },
            None,
            None,
        )
        .expect("sync bt status");

        assert_eq!(outcome.completion_action, BtCompletionAction::None);

        let job = engine.registry.get(gid).expect("job in registry");
        let bt = job.bt.expect("bt snapshot should exist");
        assert_eq!(job.total_size, Some(1_048_576));
        assert_eq!(job.downloaded, 524_288);
        assert_eq!(job.upload_speed, 512);
        assert_eq!(bt.info_hash.as_deref(), Some("abcdef1234567890"));
        assert_eq!(bt.torrent_name.as_deref(), Some("fixture.bin"));
        assert_eq!(
            bt.announce_list.as_deref(),
            Some(&["udp://tracker.example:80/announce".to_string()][..])
        );
        assert_eq!(bt.uploaded, Some(12_345));
        assert_eq!(bt.num_seeders, Some(7));
        assert_eq!(bt.piece_length, Some(16_384));
        assert_eq!(bt.num_pieces, Some(64));
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

        let status = raria_bt::service::BtStatus {
            total_size: 1_024,
            downloaded: 1_024,
            uploaded: 0,
            download_speed: 0,
            upload_speed: 0,
            num_peers: 1,
            num_seeders: 1,
            is_complete: true,
            info_hash: "abcdef1234567890".into(),
            torrent_name: Some("fixture.bin".into()),
            announce_list: None,
            piece_length: 1_024,
            num_pieces: 1,
        };

        let first = sync_bt_job_from_status(&engine, gid, &status, None, None)
            .expect("first bt sync should succeed");
        assert_eq!(first.completion_action, BtCompletionAction::EnterSeeding);

        let first_job = engine.registry.get(gid).expect("job in registry");
        assert_eq!(first_job.status, Status::Seeding);
        assert!(first_job.bt_download_complete_emitted());

        let second = sync_bt_job_from_status(&engine, gid, &status, None, None)
            .expect("second bt sync should succeed");
        assert_eq!(second.completion_action, BtCompletionAction::None);

        let second_job = engine.registry.get(gid).expect("job in registry");
        assert_eq!(second_job.status, Status::Seeding);
        assert!(second_job.bt_download_complete_emitted());
    }

    #[test]
    fn sync_bt_job_from_status_requests_direct_completion_without_seed_controls() {
        let engine = Engine::new(GlobalConfig::default());
        let gid = insert_active_bt_job(&engine, JobOptions::default());

        let outcome = sync_bt_job_from_status(
            &engine,
            gid,
            &raria_bt::service::BtStatus {
                total_size: 2_048,
                downloaded: 2_048,
                uploaded: 0,
                download_speed: 0,
                upload_speed: 0,
                num_peers: 0,
                num_seeders: 0,
                is_complete: true,
                info_hash: "feedfacefeedface".into(),
                torrent_name: Some("fixture.bin".into()),
                announce_list: None,
                piece_length: 2_048,
                num_pieces: 1,
            },
            None,
            None,
        )
        .expect("sync bt status");

        assert_eq!(outcome.completion_action, BtCompletionAction::Complete);

        let job = engine.registry.get(gid).expect("job in registry");
        assert_eq!(job.status, Status::Active);
        assert!(!job.bt_download_complete_emitted());
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
    fn seeding_continues_below_thresholds() {
        let started = Instant::now();
        assert!(!should_stop_seeding(
            100,
            20,
            Some(1.0),
            Some(10),
            started,
            started + Duration::from_secs(30),
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
        };

        let bt_files = Some(map_bt_files(vec![BtFileInfo {
            index: 0,
            path: PathBuf::from("fixture.bin"),
            size: 4096,
            completed_length: 2048,
            selected: true,
        }]));
        let bt_peers = Some(map_bt_peers(vec![BtPeerInfo {
            addr: "127.0.0.1:6881".into(),
            ip: "127.0.0.1".into(),
            port: 6881,
            download_speed: 128,
            upload_speed: 64,
            seeder: true,
        }]));

        sync_bt_status_into_job(
            &engine,
            handle.gid,
            &status,
            bt_files.clone(),
            bt_peers.clone(),
        )
        .expect("sync bt status into job");

        let job = engine.registry.get(handle.gid).expect("job");
        assert_eq!(job.downloaded, 2048);
        assert_eq!(job.download_speed, 128);
        assert_eq!(job.upload_speed, 64);
        assert_eq!(job.total_size, Some(4096));
        let synced_files = job.bt_files.expect("bt files");
        assert_eq!(synced_files.len(), 1);
        assert_eq!(synced_files[0].index, 0);
        assert_eq!(synced_files[0].path, PathBuf::from("fixture.bin"));
        assert_eq!(synced_files[0].length, 4096);
        assert_eq!(synced_files[0].completed_length, 2048);
        assert!(synced_files[0].selected);

        let synced_peers = job.bt_peers.expect("bt peers");
        assert_eq!(synced_peers.len(), 1);
        assert_eq!(synced_peers[0].addr, "127.0.0.1:6881");
        assert_eq!(synced_peers[0].ip, "127.0.0.1");
        assert_eq!(synced_peers[0].port, 6881);
        assert_eq!(synced_peers[0].download_speed, 128);
        assert_eq!(synced_peers[0].upload_speed, 64);
        assert!(synced_peers[0].seeder);

        let bt = job.bt.expect("bt snapshot");
        assert_eq!(bt.info_hash.as_deref(), Some("abcdef1234567890"));
        assert_eq!(bt.torrent_name.as_deref(), Some("fixture.bin"));
        assert_eq!(
            bt.announce_list,
            Some(vec!["http://127.0.0.1:9/announce".into()])
        );
        assert_eq!(bt.uploaded, Some(512));
        assert_eq!(bt.num_seeders, Some(7));
        assert_eq!(bt.piece_length, Some(1024));
        assert_eq!(bt.num_pieces, Some(4));
        assert!(!bt.download_complete_emitted);
    }

    #[test]
    fn sync_bt_status_into_job_caches_real_bt_fields_and_preserves_best_effort_announce_list() {
        let mut job = Job::new_bt(
            vec!["magnet:?xt=urn:btih:sync-fields".into()],
            PathBuf::from("/tmp/downloads"),
        );
        job.bt = Some(raria_core::job::BtSnapshot {
            announce_list: Some(vec!["http://existing.example/announce".into()]),
            ..Default::default()
        });

        let mut status = sample_bt_status();
        status.announce_list = None;

        sync_bt_status_into_job(&mut job, &status, None, None);

        let bt = job.bt.as_ref().expect("bt snapshot");
        assert_eq!(job.total_size, Some(4096));
        assert_eq!(job.downloaded, 2048);
        assert_eq!(job.download_speed, 128);
        assert_eq!(job.upload_speed, 64);
        assert_eq!(job.connections, 3);
        assert_eq!(
            bt.info_hash.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert_eq!(bt.torrent_name.as_deref(), Some("fixture.iso"));
        assert_eq!(
            bt.announce_list.as_ref(),
            Some(&vec!["http://existing.example/announce".into()])
        );
        assert_eq!(bt.uploaded, Some(512));
        assert_eq!(bt.num_seeders, Some(2));
        assert_eq!(bt.piece_length, Some(1024));
        assert_eq!(bt.num_pieces, Some(4));
    }

    #[test]
    fn advance_bt_lifecycle_enters_seeding_once_then_completes_after_threshold() {
        let mut job = Job::new_bt(
            vec!["magnet:?xt=urn:btih:seed-once".into()],
            PathBuf::from("/tmp/downloads"),
        );
        job.transition(Status::Active).unwrap();
        job.options.seed_ratio = Some(1.0);
        let mut seeding_started_at = None;

        let mut first_complete = sample_bt_status();
        first_complete.is_complete = true;
        let action = advance_bt_lifecycle(
            &mut job,
            &first_complete,
            &mut seeding_started_at,
            Instant::now(),
        )
        .expect("first completion reconciliation");
        assert!(
            !completed,
            "job should remain seeding until ratio threshold is met"
        );

        let job = engine.registry.get(handle.gid).expect("job");
        assert_eq!(job.status, Status::Seeding);
        assert!(job.bt_download_complete_emitted());
        assert!(seeding_started_at.is_some());

        let second_action = advance_bt_lifecycle(
            &mut job,
            &first_complete,
            &mut seeding_started_at,
            Instant::now() + Duration::from_secs(1),
        )
        .expect("second completion reconciliation");
        assert!(
            completed,
            "job should complete once seeding ratio threshold is met"
        );

        let mut threshold_reached = first_complete;
        threshold_reached.uploaded = threshold_reached.downloaded;
        let finish_action = advance_bt_lifecycle(
            &mut job,
            &threshold_reached,
            &mut seeding_started_at,
            Instant::now() + Duration::from_secs(2),
        )
        .expect("finish seeding");
        assert_eq!(finish_action, BtLifecycleAction::Complete);
        assert_eq!(job.status, Status::Complete);
        match rx.try_recv().expect("final completion event") {
            DownloadEvent::Complete { gid } => assert_eq!(gid, handle.gid),
            other => panic!("unexpected event after finishing seeding: {other:?}"),
        }
        assert!(
            rx.try_recv().is_err(),
            "BtDownloadComplete must be emitted only once"
        );
    }
}
