use anyhow::{Context, Result};
use base64::Engine as Base64Engine;
use raria_bt::service::{BtService, BtServiceConfig, BtSource};
use raria_core::engine::Engine;
use raria_core::job::{BtFile, BtPeer};
use raria_core::job::Gid;
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

    let bt_service = BtService::with_config(
        download_dir,
        BtServiceConfig {
            socks_proxy_url: engine
                .config
                .all_proxy
                .clone()
                .filter(|proxy| proxy.starts_with("socks5://")),
        },
    )
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
                        engine.registry.update(gid, |job| {
                            job.downloaded = status.downloaded;
                            job.download_speed = status.download_speed;
                            job.upload_speed = status.upload_speed;
                            if job.total_size.is_none() && status.total_size > 0 {
                                job.total_size = Some(status.total_size);
                            }
                            if bt_files.is_some() {
                                job.bt_files = bt_files.clone();
                            }
                            if bt_peers.is_some() {
                                job.bt_peers = bt_peers.clone();
                            }
                        });

                        if status.is_complete {
                            let seed_ratio = job.options.seed_ratio;
                            let seed_time = job.options.seed_time;
                            if seed_ratio.is_none() && seed_time.is_none() {
                                info!(%gid, "BT download complete");
                                let _ = engine.complete_job(gid);
                                return Ok(());
                            }

                            let started = seeding_started_at.get_or_insert_with(Instant::now);
                            if should_stop_seeding(
                                status.downloaded,
                                status.uploaded,
                                seed_ratio,
                                seed_time,
                                *started,
                                Instant::now(),
                            ) {
                                info!(%gid, "BT seeding thresholds reached");
                                let _ = engine.complete_job(gid);
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
        if now.duration_since(seeding_started_at) >= Duration::from_secs(minutes.saturating_mul(60)) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{handle_bt_cancellation, map_bt_files, map_bt_peers, should_stop_seeding};
    use raria_bt::service::{BtFileInfo, BtPeerInfo};
    use raria_core::config::GlobalConfig;
    use raria_core::engine::{AddUriSpec, Engine};
    use raria_core::job::Status;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

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
}
