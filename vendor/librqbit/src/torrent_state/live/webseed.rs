use std::{net::SocketAddr, path::Path, time::Duration};

use anyhow::{Context, Result};
use librqbit_core::lengths::{ChunkInfo, ValidPieceIndex};
use peer_binary_protocol::Piece;
use raria_range::backend::ByteSourceBackend;
use tokio::io::AsyncReadExt;
use tracing::{debug, error, trace, warn};

use super::{sort_candidate_pieces_by_strategy, InflightPiece, TorrentStateLive};

fn webseed_peer_handle() -> SocketAddr {
    // A sentinel used only for inflight bookkeeping. This must never be used as a real peer address.
    SocketAddr::from(([0, 0, 0, 0], 0))
}

const WEBSEED_HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const WEBSEED_HTTP_BODY_TIMEOUT: Duration = Duration::from_secs(20);
const WEBSEED_AUX_OPEN_TIMEOUT: Duration = Duration::from_secs(20);
const WEBSEED_AUX_READ_TIMEOUT: Duration = Duration::from_secs(20);

fn redacted_url_for_logs(url: &url::Url) -> String {
    let mut redacted = url.clone();
    let _ = redacted.set_username("");
    let _ = redacted.set_password(None);
    redacted.to_string()
}

pub(crate) async fn run_webseed_downloader(
    state: std::sync::Arc<TorrentStateLive>,
    http: reqwest::Client,
    web_seed_uris: Vec<url::Url>,
) -> Result<()> {
    if web_seed_uris.is_empty() {
        return Ok(());
    }

    // Keep a cheap cloneable view of the seeds. `Url` is cheap enough to clone for our usage.
    let web_seed_uris = std::sync::Arc::new(web_seed_uris);

    loop {
        if state.is_finished() {
            return Ok(());
        }
        if state.cancellation_token.is_cancelled() {
            return Ok(());
        }

        let Some(piece) = reserve_next_needed_piece_for_webseed(&state)? else {
            // Nothing queued right now: avoid tight loop.
            tokio::time::sleep(Duration::from_millis(250)).await;
            continue;
        };

        if let Err(error) = download_piece_via_webseed(&state, &http, &web_seed_uris, piece).await
        {
            debug!(piece = %piece, error = %error, "webseed: piece download failed");
            mark_piece_broken_and_release(&state, piece)?;
            // Backoff a bit; repeated failures (e.g. dead WebSeed) shouldn't burn CPU.
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

fn reserve_next_needed_piece_for_webseed(
    state: &std::sync::Arc<TorrentStateLive>,
) -> Result<Option<ValidPieceIndex>> {
    let mut candidates = {
        let g = state.lock_read("webseed_candidates");
        let chunk_tracker = g.get_chunks()?;

        let priority_streamed_pieces = state
            .streams
            .iter_next_pieces(&state.lengths)
            .filter(|pid| !chunk_tracker.is_piece_have(*pid) && !g.inflight_pieces.contains_key(pid));

        let natural_order_pieces = chunk_tracker
            .iter_queued_pieces(&g.file_priorities, &state.metadata.file_infos)
            .filter(|pid| !g.inflight_pieces.contains_key(pid));

        priority_streamed_pieces
            .chain(natural_order_pieces)
            .collect::<Vec<_>>()
    };

    if candidates.is_empty() {
        return Ok(None);
    }

    sort_candidate_pieces_by_strategy(
        state.shared.options.piece_selection_strategy,
        &mut candidates,
        |piece| state.peers.piece_availability(piece),
    );

    let mut g = state.lock_write("reserve_webseed_piece");
    for candidate in candidates {
        let chunk_tracker = g.get_chunks()?;
        if chunk_tracker.is_piece_have(candidate) || g.inflight_pieces.contains_key(&candidate) {
            continue;
        }

        g.inflight_pieces.insert(
            candidate,
            InflightPiece {
                peer: webseed_peer_handle(),
                started: std::time::Instant::now(),
            },
        );
        g.get_chunks_mut()?.reserve_needed_piece(candidate);
        return Ok(Some(candidate));
    }

    Ok(None)
}

async fn download_piece_via_webseed(
    state: &std::sync::Arc<TorrentStateLive>,
    http: &reqwest::Client,
    web_seed_uris: &std::sync::Arc<Vec<url::Url>>,
    piece: ValidPieceIndex,
) -> Result<()> {
    // Download each chunk of the piece using HTTP ranges.
    for chunk in state.lengths.iter_chunk_infos(piece) {
        // If our piece was stolen by a BT peer, stop spending effort.
        if !piece_is_still_owned_by_webseed(state, piece) {
            trace!(piece = %piece, "webseed: piece no longer owned; aborting download");
            return Ok(());
        }

        let data = download_chunk_via_webseed(state, http, web_seed_uris, &chunk).await?;
        state
            .ingest_webseed_chunk(piece, &chunk, data)
            .await
            .with_context(|| format!("webseed: failed to ingest chunk {chunk:?}"))?;
    }

    Ok(())
}

async fn download_chunk_via_webseed(
    state: &TorrentStateLive,
    http: &reqwest::Client,
    web_seed_uris: &std::sync::Arc<Vec<url::Url>>,
    chunk: &ChunkInfo,
) -> Result<Vec<u8>> {
    let mut out = Vec::<u8>::with_capacity(chunk.size as usize);
    let mut remaining = chunk.size as u64;
    let mut absolute_offset = state.lengths.chunk_absolute_offset(chunk);
    let mut file_idx = 0usize;

    while remaining > 0 {
        let file_info = state
            .metadata
            .file_infos
            .get(file_idx)
            .context("webseed: invalid file index while mapping chunk to files")?;
        let file_len = file_info.len;

        if absolute_offset >= file_len {
            absolute_offset -= file_len;
            file_idx += 1;
            continue;
        }

        let to_take = std::cmp::min(remaining, file_len - absolute_offset) as usize;
        if file_info.attrs.padding {
            out.extend(std::iter::repeat_n(0u8, to_take));
        } else {
            let file_url = fetch_file_range(
                http,
                web_seed_uris,
                state,
                file_idx,
                &file_info.relative_filename,
                absolute_offset,
                to_take,
            )
            .await
            .with_context(|| {
                format!(
                    "webseed: range fetch failed: file_idx={file_idx} offset={absolute_offset} len={to_take}"
                )
            })?;
            out.extend_from_slice(&file_url);
        }

        remaining -= to_take as u64;
        absolute_offset = 0;
        file_idx += 1;
    }

    if out.len() != chunk.size as usize {
        anyhow::bail!(
            "webseed: internal error, fetched size mismatch (expected {}, got {})",
            chunk.size,
            out.len()
        );
    }

    Ok(out)
}

async fn fetch_file_range(
    http: &reqwest::Client,
    web_seed_uris: &std::sync::Arc<Vec<url::Url>>,
    state: &TorrentStateLive,
    file_idx: usize,
    relative_filename: &Path,
    offset: u64,
    len: usize,
) -> Result<Vec<u8>> {
    let candidates =
        candidate_urls_for_file(web_seed_uris, &state.metadata.file_infos, relative_filename);
    if candidates.is_empty() {
        anyhow::bail!("webseed: no candidate URLs for file {file_idx}");
    }

    let range_end = offset
        .checked_add(len as u64)
        .and_then(|v| v.checked_sub(1))
        .context("webseed: invalid range arithmetic")?;
    let range_value = format!("bytes={offset}-{range_end}");

    for url in candidates {
        let safe_url = redacted_url_for_logs(&url);
        trace!(file_idx, %offset, %len, url = %safe_url, "webseed: fetching range");
        let fetched = match url.scheme() {
            "http" | "https" => {
                fetch_http_range(http, &url, &range_value, len)
                    .await
                    .with_context(|| format!("webseed: http range fetch failed for {safe_url}"))
            }
            "ftp" | "ftps" => {
                let backend = raria_ftp::backend::FtpBackend::new();
                fetch_aux_backend_range(&backend, &url, offset, len)
                    .await
                    .with_context(|| format!("webseed: ftp range fetch failed for {safe_url}"))
            }
            "sftp" => {
                let backend = raria_sftp::backend::SftpBackend::new();
                fetch_aux_backend_range(&backend, &url, offset, len)
                    .await
                    .with_context(|| format!("webseed: sftp range fetch failed for {safe_url}"))
            }
            scheme => {
                warn!(url = %safe_url, %scheme, "webseed: unsupported seed URI scheme");
                continue;
            }
        };

        match fetched {
            Ok(bytes) => return Ok(bytes),
            Err(error) => {
                warn!(url = %safe_url, %error, "webseed: range fetch failed for candidate");
                continue;
            }
        }
    }

    anyhow::bail!("webseed: exhausted all candidate URLs for file {file_idx}");
}

async fn fetch_http_range(
    http: &reqwest::Client,
    url: &url::Url,
    range_value: &str,
    len: usize,
) -> Result<Vec<u8>> {
    let safe_url = redacted_url_for_logs(url);
    let response = tokio::time::timeout(
        WEBSEED_HTTP_REQUEST_TIMEOUT,
        http.get(url.clone())
            .header(reqwest::header::RANGE, range_value)
            .send(),
    )
    .await
    .context("webseed: HTTP request timed out")?
    .with_context(|| format!("webseed: HTTP request failed for {safe_url}"))?;

    let status = response.status();
    if status != reqwest::StatusCode::PARTIAL_CONTENT && status != reqwest::StatusCode::OK {
        anyhow::bail!("webseed: unexpected HTTP status {status} for {safe_url}");
    }

    let bytes = tokio::time::timeout(WEBSEED_HTTP_BODY_TIMEOUT, response.bytes())
        .await
        .context("webseed: HTTP body read timed out")?
        .with_context(|| format!("webseed: failed reading response body for {safe_url}"))?;

    if bytes.len() != len {
        anyhow::bail!(
            "webseed: unexpected byte length for range: expected={}, got={}",
            len,
            bytes.len()
        );
    }

    Ok(bytes.to_vec())
}

async fn fetch_aux_backend_range<B: ByteSourceBackend>(
    backend: &B,
    url: &url::Url,
    offset: u64,
    len: usize,
) -> Result<Vec<u8>> {
    let safe_url = redacted_url_for_logs(url);
    let mut stream = tokio::time::timeout(
        WEBSEED_AUX_OPEN_TIMEOUT,
        backend.open_from(url, offset, &raria_range::backend::OpenContext::default()),
    )
    .await
    .with_context(|| format!("webseed: {} open_from timeout for {safe_url}", backend.name()))?
    .with_context(|| format!("webseed: {} open_from failed for {safe_url}", backend.name()))?;

    let mut out = vec![0u8; len];
    tokio::time::timeout(WEBSEED_AUX_READ_TIMEOUT, stream.read_exact(&mut out))
        .await
        .with_context(|| format!("webseed: {} range read timeout for {safe_url}", backend.name()))?
        .with_context(|| format!("webseed: {} range read failed for {safe_url}", backend.name()))?;

    Ok(out)
}

fn candidate_urls_for_file(
    web_seed_uris: &std::sync::Arc<Vec<url::Url>>,
    file_infos: &[crate::file_info::FileInfo],
    relative_filename: &Path,
) -> Vec<url::Url> {
    let is_single_file = file_infos.len() == 1;
    let rel = relative_filename.to_string_lossy().replace('\\', "/");

    web_seed_uris
        .iter()
        .filter_map(|base| {
            if is_single_file && !base.path().ends_with('/') {
                // For single-file torrents, treat a non-directory URI as a direct file URL.
                return Some(base.clone());
            }

            let mut base_dir = base.clone();
            if !base_dir.path().ends_with('/') {
                let new_path = format!("{}/", base_dir.path());
                base_dir.set_path(&new_path);
            }

            // Multi-file torrents typically map by relative path.
            base_dir.join(&rel).ok()
        })
        .collect()
}

fn piece_is_still_owned_by_webseed(state: &TorrentStateLive, piece: ValidPieceIndex) -> bool {
    let g = state.lock_read("webseed_piece_owned_check");
    g.inflight_pieces
        .get(&piece)
        .map(|v| v.peer == webseed_peer_handle())
        .unwrap_or(false)
}

fn mark_piece_broken_and_release(state: &TorrentStateLive, piece: ValidPieceIndex) -> Result<()> {
    let mut g = state.lock_write("webseed_mark_piece_broken");
    if let Some(inflight) = g.inflight_pieces.get(&piece) {
        if inflight.peer != webseed_peer_handle() {
            return Ok(());
        }
    }
    g.inflight_pieces.remove(&piece);
    g.get_chunks_mut()?.mark_piece_broken_if_not_have(piece);
    state.new_pieces_notify.notify_waiters();
    Ok(())
}

impl TorrentStateLive {
    pub(crate) async fn ingest_webseed_chunk(
        self: &std::sync::Arc<Self>,
        piece: ValidPieceIndex,
        chunk: &ChunkInfo,
        data: Vec<u8>,
    ) -> Result<()> {
        let addr = webseed_peer_handle();
        let piece_msg = Piece {
            index: piece.get(),
            begin: chunk.offset,
            block: data,
        };

        // Track fetch volume for stats/speed estimation (even though this isn't a BT peer).
        self.stats
            .fetched_bytes
            .fetch_add(chunk.size as u64, std::sync::atomic::Ordering::Relaxed);
        self.session_stats
            .fetched_bytes
            .fetch_add(chunk.size as u64, std::sync::atomic::Ordering::Relaxed);

        if let Some(dtx) = self.disk_work_tx() {
            let span = tracing::error_span!("webseed_write");
            let state = self.clone();
            let chunk = *chunk;
            let (result_tx, result_rx) = tokio::sync::oneshot::channel::<anyhow::Result<()>>();
            let work = move || {
                let result = span.in_scope(|| {
                    ingest_webseed_chunk_blocking(&state, addr, &piece_msg, &chunk)
                });
                let _ = result_tx.send(result);
            };
            dtx.send(Box::new(work)).await?;
            match result_rx.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err(error),
                Err(_) => anyhow::bail!("webseed: deferred write queue dropped ingest result"),
            }
        } else {
            let chunk = *chunk;
            self.shared.spawner.spawn_block_in_place(|| {
                ingest_webseed_chunk_blocking(self, addr, &piece_msg, &chunk)
            })?;
        }

        Ok(())
    }
}

fn ingest_webseed_chunk_blocking(
    state: &TorrentStateLive,
    addr: SocketAddr,
    piece: &Piece<Vec<u8>>,
    chunk_info: &ChunkInfo,
) -> Result<()> {
    // If someone stole the piece by now, ignore it.
    // However if they didn't, don't let them steal it while we are writing.
    let _ppl_guard = {
        let g = state.lock_read("webseed_check_steal");
        let ppl = state
            .per_piece_locks
            .get(piece.index as usize)
            .map(|l| l.read());

        match g.inflight_pieces.get(&chunk_info.piece_index) {
            Some(InflightPiece { peer, .. }) if *peer == addr => {}
            Some(InflightPiece { peer, .. }) => {
                debug!(
                    "webseed: in-flight piece {} was stolen by {}, ignoring",
                    chunk_info.piece_index, peer
                );
                return Ok(());
            }
            None => {
                debug!(
                    "webseed: in-flight piece {} not found, ignoring",
                    chunk_info.piece_index
                );
                return Ok(());
            }
        };

        ppl
    };

    if let Err(e) = state.file_ops().write_chunk(addr, piece, chunk_info) {
        error!("webseed: FATAL error writing chunk to disk: {e:#}");
        return state.on_fatal_error(e);
    }

    let full_piece_download_time = {
        let mut g = state.lock_write("webseed_mark_chunk_downloaded");
        let chunk_marking_result = g.get_chunks_mut()?.mark_chunk_downloaded(piece);
        trace!(?piece, chunk_marking_result=?chunk_marking_result);

        match chunk_marking_result {
            Some(crate::chunk_tracker::ChunkMarkingResult::Completed) => {
                let piece = chunk_info.piece_index;
                g.inflight_pieces.remove(&piece).map(|t| t.started.elapsed())
            }
            Some(crate::chunk_tracker::ChunkMarkingResult::PreviouslyCompleted) => return Ok(()),
            Some(crate::chunk_tracker::ChunkMarkingResult::NotCompleted) => None,
            None => anyhow::bail!("webseed: bogus data received for chunk download"),
        }
    };

    let Some(full_piece_download_time) = full_piece_download_time else {
        return Ok(());
    };

    let index = chunk_info.piece_index;
    match state
        .file_ops()
        .check_piece(index)
        .with_context(|| format!("webseed: error checking piece={index}"))?
    {
        true => {
            {
                let mut g = state.lock_write("webseed_mark_piece_downloaded");
                g.get_chunks_mut()?.mark_piece_downloaded(chunk_info.piece_index);
            }

            let piece_len = state.lengths.piece_length(chunk_info.piece_index) as u64;
            state
                .stats
                .downloaded_and_checked_bytes
                .fetch_add(piece_len, std::sync::atomic::Ordering::Release);
            state
                .stats
                .downloaded_and_checked_pieces
                .fetch_add(1, std::sync::atomic::Ordering::Release);
            state
                .stats
                .have_bytes
                .fetch_add(piece_len, std::sync::atomic::Ordering::Relaxed);
            #[allow(clippy::cast_possible_truncation)]
            state.stats.total_piece_download_ms.fetch_add(
                full_piece_download_time.as_millis() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );

            debug!("webseed: piece={} downloaded and verified", index);

            state.on_piece_completed(chunk_info.piece_index)?;
            state.transmit_haves(chunk_info.piece_index);
        }
        false => {
            warn!("webseed: checksum did not validate for piece={index}");
            state
                .lock_write("webseed_mark_piece_broken")
                .get_chunks_mut()?
                .mark_piece_broken_if_not_have(chunk_info.piece_index);
            state.new_pieces_notify.notify_waiters();
        }
    };

    Ok(())
}
