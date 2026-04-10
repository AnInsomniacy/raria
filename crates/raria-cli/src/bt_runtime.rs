use anyhow::{Context, Result};
use base64::Engine as Base64Engine;
use raria_bt::service::{BtService, BtSource};
use raria_core::engine::Engine;
use raria_core::job::Gid;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

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
    } else if uri_str.starts_with("torrent:base64:") {
        let b64 = &uri_str["torrent:base64:".len()..];
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .context("failed to decode torrent base64")?;
        BtSource::TorrentBytes(bytes)
    } else {
        BtSource::TorrentFile(std::path::PathBuf::from(uri_str))
    };

    let bt_service = BtService::new(download_dir).context("failed to create BtService")?;
    let handle = bt_service
        .add(source, gid)
        .await
        .context("failed to add torrent to BtService")?;

    info!(%gid, torrent_id = handle.torrent_id, "BT download started");

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!(%gid, "BT download cancelled");
                let _ = bt_service.pause(&handle).await;
                let _ = engine.fail_job(gid, "cancelled by user");
                return Ok(());
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {
                match bt_service.status(&handle).await {
                    Ok(status) => {
                        engine.registry.update(gid, |job| {
                            job.downloaded = status.downloaded;
                            job.download_speed = status.download_speed;
                            job.upload_speed = status.upload_speed;
                            if job.total_size.is_none() && status.total_size > 0 {
                                job.total_size = Some(status.total_size);
                            }
                        });

                        if status.is_complete {
                            info!(%gid, "BT download complete");
                            let _ = engine.complete_job(gid);
                            return Ok(());
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
