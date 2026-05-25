//! Native raria HTTP JSON API.

use crate::metalink_tasks::{
    apply_metalink_seed_metadata, normalize_metalink_for_engine, parse_metalink_xml,
    torrent_metadata_source,
};
use anyhow::{Context, Result};
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use raria_core::engine::{AddUriSpec, Engine};
use raria_core::native::{
    NativePeerSnapshot, NativeTaskFile, NativeTaskSummary, NativeTrackerSnapshot, TaskId,
    TaskSource,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::info;

/// Native API server configuration.
#[derive(Debug, Clone)]
pub struct NativeApiConfig {
    /// Address to listen on.
    pub listen_addr: SocketAddr,
    /// Optional bearer token required for native API requests.
    pub auth_token: Option<String>,
}

impl Default for NativeApiConfig {
    fn default() -> Self {
        Self {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 6800)),
            auth_token: None,
        }
    }
}

/// Addresses returned by the native API server.
#[derive(Debug, Clone)]
pub struct NativeApiAddrs {
    /// HTTP API address.
    pub http: SocketAddr,
}

#[derive(Clone)]
pub(crate) struct NativeApiState {
    engine: Arc<Engine>,
    auth_token: Option<String>,
}

/// Start the native HTTP JSON API server.
pub async fn start_native_api_server(
    engine: Arc<Engine>,
    config: &NativeApiConfig,
    cancel: CancellationToken,
) -> Result<NativeApiAddrs> {
    let listener = tokio::net::TcpListener::bind(config.listen_addr)
        .await
        .context("failed to bind native API server")?;
    let addr = listener
        .local_addr()
        .context("failed to read native API local address")?;

    let app = native_api_router(engine, config.auth_token.clone());

    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                cancel.cancelled().await;
                info!("stopping native API server");
            })
            .await
            .expect("native API server task failed");
    });

    Ok(NativeApiAddrs { http: addr })
}

/// Build the native API router for standalone or shared listeners.
pub fn native_api_router(engine: Arc<Engine>, auth_token: Option<String>) -> Router {
    let state = NativeApiState { engine, auth_token };
    Router::new()
        .route("/api/v1/health", get(handle_health))
        .route("/api/v1/config", get(handle_config))
        .route("/api/v1/daemon/shutdown", post(handle_daemon_shutdown))
        .route("/api/v1/events", get(handle_events_ws))
        .route("/api/v1/session/save", post(handle_save_session))
        .route("/api/v1/stats", get(handle_stats))
        .route(
            "/api/v1/transfer",
            get(handle_global_transfer).patch(handle_patch_global_transfer),
        )
        .route(
            "/api/v1/tasks",
            get(handle_list_tasks).post(handle_create_task),
        )
        .route(
            "/api/v1/tasks/:task_id",
            get(handle_get_task).delete(handle_remove_task),
        )
        .route("/api/v1/tasks/:task_id/pause", post(handle_pause_task))
        .route(
            "/api/v1/tasks/:task_id/queue",
            get(handle_task_queue).patch(handle_patch_task_queue),
        )
        .route("/api/v1/tasks/:task_id/restart", post(handle_restart_task))
        .route("/api/v1/tasks/:task_id/resume", post(handle_resume_task))
        .route(
            "/api/v1/tasks/:task_id/files",
            get(handle_task_files).patch(handle_patch_task_files),
        )
        .route(
            "/api/v1/tasks/:task_id/bt/seeding",
            get(handle_task_bt_seeding).patch(handle_patch_task_bt_seeding),
        )
        .route(
            "/api/v1/tasks/:task_id/transfer",
            get(handle_task_transfer).patch(handle_patch_task_transfer),
        )
        .route("/api/v1/tasks/:task_id/peers", get(handle_task_peers))
        .route(
            "/api/v1/tasks/:task_id/sources",
            get(handle_task_sources).patch(handle_patch_task_sources),
        )
        .route(
            "/api/v1/tasks/:task_id/trackers",
            get(handle_task_trackers).patch(handle_patch_task_trackers),
        )
        .with_state(state)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    status: &'static str,
    api_version: u32,
    version: &'static str,
    uptime_seconds: u64,
}

async fn handle_health(State(state): State<NativeApiState>) -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok",
        api_version: 1,
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: state.engine.uptime_seconds(),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeConfigResponse {
    daemon: RuntimeDaemonConfig,
    downloads: RuntimeDownloadsConfig,
    metalink: RuntimeMetalinkConfig,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeDaemonConfig {
    download_dir: PathBuf,
    session_path: PathBuf,
    max_active_tasks: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeDownloadsConfig {
    default_segments: u32,
    min_segment_size: u64,
    retry_max_attempts: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeMetalinkConfig {
    preferred_locations: Vec<String>,
    preferred_protocol: Option<String>,
    unique_protocols: bool,
}

async fn handle_config(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
) -> Result<Json<RuntimeConfigResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let config = &state.engine.config;

    Ok(Json(RuntimeConfigResponse {
        daemon: RuntimeDaemonConfig {
            download_dir: config.download_dir.clone(),
            session_path: config.session_file.clone(),
            max_active_tasks: config.max_concurrent_downloads,
        },
        downloads: RuntimeDownloadsConfig {
            default_segments: config.default_segments,
            min_segment_size: config.min_segment_size,
            retry_max_attempts: config.retry_attempts,
        },
        metalink: RuntimeMetalinkConfig {
            preferred_locations: config.metalink_preferred_locations.clone(),
            preferred_protocol: config.metalink_preferred_protocol.clone(),
            unique_protocols: config.metalink_unique_protocols,
        },
    }))
}

async fn handle_events_ws(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<NativeApiState>,
) -> Response {
    if let Err(error) = require_auth(&state, &headers) {
        return error.into_response();
    }
    ws.on_upgrade(move |socket| handle_events_client(socket, state))
}

async fn handle_events_client(mut socket: WebSocket, state: NativeApiState) {
    let mut native_events = state.engine.native_event_bus.subscribe();
    let mut sequence = 1u64;

    loop {
        let native_event = match native_events.try_recv() {
            Ok(event) => {
                let mut event = event;
                event.sequence = sequence;
                event
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                match native_events.recv().await {
                    Ok(event) => {
                        let mut event = event;
                        event.sequence = sequence;
                        event
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
        };

        sequence += 1;

        let Ok(text) = serde_json::to_string(&native_event) else {
            continue;
        };
        if socket.send(WsMessage::Text(text)).await.is_err() {
            break;
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskListResponse {
    tasks: Vec<NativeTaskSummary>,
}

async fn handle_list_tasks(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
) -> Result<Json<TaskListResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let tasks = state.engine.native_task_summaries();

    Ok(Json(TaskListResponse { tasks }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatsResponse {
    task_counts: TaskCounts,
    download_bytes_per_second: u64,
    upload_bytes_per_second: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveSessionResponse {
    status: &'static str,
    task_count: usize,
    session_path: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DaemonShutdownResponse {
    status: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GlobalTransferPolicyResponse {
    download_bytes_per_second_limit: u64,
    upload_bytes_per_second_limit: u64,
    max_active_tasks: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchGlobalTransferPolicyRequest {
    download_bytes_per_second_limit: Option<u64>,
    upload_bytes_per_second_limit: Option<u64>,
    max_active_tasks: Option<u32>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskCounts {
    queued: usize,
    running: usize,
    paused: usize,
    seeding: usize,
    completed: usize,
    failed: usize,
    removed: usize,
}

async fn handle_stats(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
) -> Result<Json<StatsResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let mut counts = TaskCounts::default();
    let mut download_bytes_per_second = 0u64;
    let mut upload_bytes_per_second = 0u64;

    for job in state.engine.registry.snapshot() {
        match job.status {
            raria_core::job::Status::Waiting => counts.queued += 1,
            raria_core::job::Status::Active => counts.running += 1,
            raria_core::job::Status::Paused => counts.paused += 1,
            raria_core::job::Status::Seeding => counts.seeding += 1,
            raria_core::job::Status::Complete => counts.completed += 1,
            raria_core::job::Status::Error => counts.failed += 1,
            raria_core::job::Status::Removed => counts.removed += 1,
        }
        download_bytes_per_second += job.download_speed;
        upload_bytes_per_second += job.upload_speed;
    }

    Ok(Json(StatsResponse {
        task_counts: counts,
        download_bytes_per_second,
        upload_bytes_per_second,
    }))
}

async fn handle_global_transfer(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
) -> Result<Json<GlobalTransferPolicyResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    Ok(Json(GlobalTransferPolicyResponse {
        download_bytes_per_second_limit: state.engine.global_rate_limiter.limit_bps(),
        upload_bytes_per_second_limit: state.engine.global_upload_limit_bps(),
        max_active_tasks: state.engine.scheduler.max_concurrent(),
    }))
}

async fn handle_patch_global_transfer(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Json(request): Json<PatchGlobalTransferPolicyRequest>,
) -> Result<Json<GlobalTransferPolicyResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    if let Some(limit) = request.download_bytes_per_second_limit {
        state.engine.global_rate_limiter.update_limit(limit);
    }
    if let Some(limit) = request.upload_bytes_per_second_limit {
        state.engine.update_global_upload_limit(limit);
    }
    if let Some(max_active_tasks) = request.max_active_tasks {
        state.engine.scheduler.set_max_concurrent(max_active_tasks);
        state.engine.work_notify().notify_one();
    }
    Ok(Json(GlobalTransferPolicyResponse {
        download_bytes_per_second_limit: state.engine.global_rate_limiter.limit_bps(),
        upload_bytes_per_second_limit: state.engine.global_upload_limit_bps(),
        max_active_tasks: state.engine.scheduler.max_concurrent(),
    }))
}

async fn handle_save_session(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
) -> Result<Json<SaveSessionResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    state
        .engine
        .save_session()
        .map_err(|_| NativeApiError::SessionStoreUnavailable)?;

    Ok(Json(SaveSessionResponse {
        status: "saved",
        task_count: state.engine.registry.len(),
        session_path: state.engine.config.session_file.clone(),
    }))
}

async fn handle_daemon_shutdown(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
) -> Result<Json<DaemonShutdownResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    state.engine.shutdown();

    Ok(Json(DaemonShutdownResponse {
        status: "shuttingDown",
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateTaskRequest {
    sources: Option<Vec<String>>,
    download_dir: PathBuf,
    filename: Option<String>,
    segments: Option<u32>,
    headers: Option<BTreeMap<String, String>>,
    auth: Option<CreateTaskAuth>,
    checksum: Option<String>,
    metalink: Option<CreateMetalinkTaskOptions>,
    bt: Option<CreateBtTaskOptions>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateTaskAuth {
    username: String,
    password: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateMetalinkTaskOptions {
    bytes_base64: Option<String>,
    path: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateMetalinkTasksResponse {
    tasks: Vec<NativeTaskSummary>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateBtTaskOptions {
    selected_file_ids: Option<Vec<String>>,
    tracker_uris: Option<Vec<String>>,
    metadata_only: Option<bool>,
    web_seed_uris: Option<Vec<String>>,
    delete_unselected_files_on_completion: Option<bool>,
    seeding: Option<PatchBtSeedingPolicyRequest>,
}

async fn handle_create_task(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Json(request): Json<CreateTaskRequest>,
) -> Result<Response, NativeApiError> {
    require_auth(&state, &headers)?;
    if request.metalink.is_some() {
        let response = create_metalink_tasks(&state.engine, request)
            .await
            .map_err(|_| NativeApiError::InvalidRequest)?;
        return Ok(Json(response).into_response());
    }

    let Some(sources) = request.sources else {
        return Err(NativeApiError::InvalidRequest);
    };
    if sources.is_empty() {
        return Err(NativeApiError::InvalidRequest);
    }
    let request_headers = request
        .headers
        .unwrap_or_default()
        .into_iter()
        .map(|(name, value)| {
            let name = name.trim().to_string();
            anyhow::ensure!(!name.is_empty(), "header name must not be empty");
            axum::http::HeaderName::from_bytes(name.as_bytes())?;
            axum::http::HeaderValue::from_str(&value)?;
            Ok((name, value))
        })
        .collect::<Result<Vec<_>>>()
        .map_err(|_| NativeApiError::InvalidRequest)?;
    let (http_user, http_password) = request
        .auth
        .map(|auth| (Some(auth.username), auth.password))
        .unwrap_or((None, None));
    let bt_options = request.bt.as_ref();
    let bt_selected_files = bt_options
        .and_then(|bt| {
            bt.selected_file_ids.as_ref().map(|ids| {
                ids.iter()
                    .map(|id| parse_file_id(id))
                    .collect::<std::result::Result<Vec<_>, _>>()
            })
        })
        .transpose()?;

    let summary = state
        .engine
        .add_native_task(&AddUriSpec {
            uris: sources,
            dir: request.download_dir,
            filename: request.filename,
            connections: request.segments.unwrap_or(1).max(1),
            headers: request_headers,
            http_user,
            http_password,
            checksum: request.checksum,
        })
        .map_err(|_| NativeApiError::InvalidRequest)?;
    if let Some(selected_files) = bt_selected_files {
        state
            .engine
            .update_native_task_file_selection(
                &summary.task_id,
                &selected_files
                    .into_iter()
                    .map(|index| format!("file_{index}"))
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| NativeApiError::InvalidRequest)?;
    }
    if let Some(trackers) = bt_options.and_then(|bt| bt.tracker_uris.as_ref()) {
        state
            .engine
            .update_native_task_trackers(&summary.task_id, trackers)
            .map_err(|_| NativeApiError::InvalidRequest)?;
    }
    if let Some(metadata_only) = bt_options.and_then(|bt| bt.metadata_only) {
        state
            .engine
            .update_native_bt_metadata_only_policy(&summary.task_id, metadata_only)
            .map_err(|_| NativeApiError::InvalidRequest)?;
    }
    if let Some(web_seed_uris) = bt_options.and_then(|bt| bt.web_seed_uris.as_ref()) {
        state
            .engine
            .update_native_bt_web_seed_uris(&summary.task_id, web_seed_uris)
            .map_err(|_| NativeApiError::InvalidRequest)?;
    }
    if let Some(delete_unselected) =
        bt_options.and_then(|bt| bt.delete_unselected_files_on_completion)
    {
        state
            .engine
            .update_native_bt_delete_unselected_files_policy(&summary.task_id, delete_unselected)
            .map_err(|_| NativeApiError::InvalidRequest)?;
    }
    if let Some(seeding) = bt_options.and_then(|bt| bt.seeding.as_ref()) {
        state
            .engine
            .update_native_bt_seeding_policy(
                &summary.task_id,
                seeding.target_ratio,
                seeding.stop_after_minutes,
                seeding.idle_download_timeout_seconds,
            )
            .map_err(|_| NativeApiError::InvalidRequest)?;
    }
    let summary = state
        .engine
        .native_task_summary(&summary.task_id)
        .map_err(|_| NativeApiError::InvalidRequest)?;

    Ok(Json(summary).into_response())
}

async fn create_metalink_tasks(
    engine: &Arc<Engine>,
    request: CreateTaskRequest,
) -> Result<CreateMetalinkTasksResponse> {
    use base64::Engine as Base64Engine;

    let metalink = request.metalink.context("metalink payload missing")?;
    let xml = if let Some(bytes_base64) = metalink.bytes_base64 {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(bytes_base64)
            .context("invalid metalink base64")?;
        String::from_utf8(bytes).context("metalink is not valid UTF-8")?
    } else if let Some(path) = metalink.path {
        tokio::fs::read_to_string(path)
            .await
            .context("failed to read metalink file")?
    } else {
        anyhow::bail!("metalink bytesBase64 or path is required");
    };
    let metalink = parse_metalink_xml(&xml)?;
    let seeds = normalize_metalink_for_engine(engine, &metalink);

    let mut tasks = Vec::new();
    for seed in seeds {
        if seed.uris.is_empty() && torrent_metadata_source(&seed).is_none() {
            continue;
        }
        let summary = create_task_from_metalink_seed(
            engine,
            &seed,
            request.download_dir.clone(),
            request.segments.unwrap_or(1).max(1),
        )?;
        tasks.push(summary);
    }
    anyhow::ensure!(!tasks.is_empty(), "metalink contains no downloadable files");
    Ok(CreateMetalinkTasksResponse { tasks })
}

fn create_task_from_metalink_seed(
    engine: &Engine,
    seed: &raria_metalink::normalizer::RangeJobSeed,
    download_dir: PathBuf,
    connections: u32,
) -> Result<NativeTaskSummary> {
    let summary = if let Some(metadata_source) = torrent_metadata_source(seed) {
        engine.add_native_bt_task_from_metadata_source(
            &metadata_source.uri,
            &seed.uris,
            download_dir,
            Some(seed.filename.clone()),
            connections,
        )?
    } else {
        engine.add_native_task(&AddUriSpec {
            uris: seed.uris.clone(),
            dir: download_dir,
            filename: Some(seed.filename.clone()),
            connections,
            headers: Vec::new(),
            http_user: None,
            http_password: None,
            checksum: seed
                .checksum
                .as_ref()
                .map(|checksum| format!("{}={}", checksum.algo, checksum.value)),
        })?
    };
    let gid = engine
        .gid_for_task_id(&summary.task_id)
        .context("native task was not registered")?;
    apply_metalink_seed_metadata(engine, gid, seed)?;
    engine.native_task_summary(&summary.task_id)
}

async fn handle_get_task(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
) -> Result<Json<NativeTaskSummary>, NativeApiError> {
    require_auth(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let summary = state
        .engine
        .native_task_summary(&task_id)
        .map_err(|_| NativeApiError::TaskNotFound)?;

    Ok(Json(summary))
}

async fn handle_pause_task(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
) -> Result<Json<NativeTaskSummary>, NativeApiError> {
    require_auth(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let summary = state
        .engine
        .pause_native_task(&task_id)
        .map_err(|_| NativeApiError::TaskNotFound)?;

    Ok(Json(summary))
}

async fn handle_resume_task(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
) -> Result<Json<NativeTaskSummary>, NativeApiError> {
    require_auth(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let summary = state
        .engine
        .resume_native_task(&task_id)
        .map_err(|_| NativeApiError::TaskNotFound)?;

    Ok(Json(summary))
}

async fn handle_remove_task(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
) -> Result<Json<NativeTaskSummary>, NativeApiError> {
    require_auth(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let summary = state
        .engine
        .remove_native_task(&task_id)
        .map_err(|_| NativeApiError::TaskNotFound)?;

    Ok(Json(summary))
}

async fn handle_restart_task(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
) -> Result<Json<NativeTaskSummary>, NativeApiError> {
    require_auth(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let summary = state
        .engine
        .restart_native_task(&task_id)
        .map_err(|_| NativeApiError::TaskNotFound)?;
    Ok(Json(summary))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FilesResponse {
    files: Vec<NativeTaskFile>,
}

async fn handle_task_files(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
) -> Result<Json<FilesResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let summary = task_summary_by_id(&state.engine, &task_id)?;
    Ok(Json(FilesResponse {
        files: summary.files,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchFilesRequest {
    selected_file_ids: Vec<String>,
}

async fn handle_patch_task_files(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
    Json(request): Json<PatchFilesRequest>,
) -> Result<Json<FilesResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    ensure_native_task_exists(&state.engine, &task_id)?;
    let summary = state
        .engine
        .update_native_task_file_selection(&task_id, &request.selected_file_ids)
        .map_err(|_| NativeApiError::InvalidRequest)?;
    Ok(Json(FilesResponse {
        files: summary.files,
    }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourcesResponse {
    sources: Vec<TaskSource>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchSourcesRequest {
    sources: Vec<String>,
}

async fn handle_task_sources(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
) -> Result<Json<SourcesResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let summary = task_summary_by_id(&state.engine, &task_id)?;
    Ok(Json(SourcesResponse {
        sources: summary.sources,
    }))
}

async fn handle_patch_task_sources(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
    Json(request): Json<PatchSourcesRequest>,
) -> Result<Json<SourcesResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    ensure_native_task_exists(&state.engine, &task_id)?;
    let summary = state
        .engine
        .replace_native_task_sources(&task_id, &request.sources)
        .map_err(|_| NativeApiError::InvalidRequest)?;
    Ok(Json(SourcesResponse {
        sources: summary.sources,
    }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PeersResponse {
    peers: Vec<NativePeerSnapshot>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BtSeedingPolicyResponse {
    target_ratio: Option<f64>,
    stop_after_minutes: Option<u64>,
    idle_download_timeout_seconds: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TransferPolicyResponse {
    download_bytes_per_second_limit: u64,
    upload_bytes_per_second_limit: u64,
    segments: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QueuePositionResponse {
    task_id: TaskId,
    position: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchQueuePositionRequest {
    position: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchTransferPolicyRequest {
    download_bytes_per_second_limit: Option<u64>,
    upload_bytes_per_second_limit: Option<u64>,
    segments: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchBtSeedingPolicyRequest {
    target_ratio: Option<f64>,
    stop_after_minutes: Option<u64>,
    idle_download_timeout_seconds: Option<u64>,
}

async fn handle_task_bt_seeding(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
) -> Result<Json<BtSeedingPolicyResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let job = state
        .engine
        .registry
        .get_by_task_id(&task_id)
        .ok_or(NativeApiError::TaskNotFound)?;
    Ok(Json(BtSeedingPolicyResponse {
        target_ratio: job.options.seed_ratio,
        stop_after_minutes: job.options.seed_time,
        idle_download_timeout_seconds: job.options.bt_idle_download_timeout,
    }))
}

async fn handle_patch_task_bt_seeding(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
    Json(request): Json<PatchBtSeedingPolicyRequest>,
) -> Result<Json<BtSeedingPolicyResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    ensure_native_task_exists(&state.engine, &task_id)?;
    let (target_ratio, stop_after_minutes, idle_download_timeout_seconds) = state
        .engine
        .update_native_bt_seeding_policy(
            &task_id,
            request.target_ratio,
            request.stop_after_minutes,
            request.idle_download_timeout_seconds,
        )
        .map_err(|_| NativeApiError::InvalidRequest)?;
    Ok(Json(BtSeedingPolicyResponse {
        target_ratio,
        stop_after_minutes,
        idle_download_timeout_seconds,
    }))
}

async fn handle_task_queue(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
) -> Result<Json<QueuePositionResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let position = state
        .engine
        .scheduler
        .waiting_task_queue()
        .iter()
        .position(|queued| queued == &task_id)
        .ok_or(NativeApiError::TaskNotFound)?;
    Ok(Json(QueuePositionResponse { task_id, position }))
}

async fn handle_patch_task_queue(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
    Json(request): Json<PatchQueuePositionRequest>,
) -> Result<Json<QueuePositionResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    ensure_native_task_exists(&state.engine, &task_id)?;
    let position = state
        .engine
        .change_native_queue_position(&task_id, request.position)
        .map_err(|_| NativeApiError::InvalidRequest)?;
    Ok(Json(QueuePositionResponse { task_id, position }))
}

async fn handle_task_transfer(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
) -> Result<Json<TransferPolicyResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let job = state
        .engine
        .registry
        .get_by_task_id(&task_id)
        .ok_or(NativeApiError::TaskNotFound)?;
    Ok(Json(TransferPolicyResponse {
        download_bytes_per_second_limit: job.options.max_download_limit,
        upload_bytes_per_second_limit: job.options.max_upload_limit,
        segments: job.options.max_connections,
    }))
}

async fn handle_patch_task_transfer(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
    Json(request): Json<PatchTransferPolicyRequest>,
) -> Result<Json<TransferPolicyResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    ensure_native_task_exists(&state.engine, &task_id)?;
    let (download_limit, upload_limit, segments) = state
        .engine
        .update_native_transfer_policy(
            &task_id,
            request.download_bytes_per_second_limit,
            request.upload_bytes_per_second_limit,
            request.segments,
        )
        .map_err(|_| NativeApiError::InvalidRequest)?;
    Ok(Json(TransferPolicyResponse {
        download_bytes_per_second_limit: download_limit,
        upload_bytes_per_second_limit: upload_limit,
        segments,
    }))
}

async fn handle_task_peers(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
) -> Result<Json<PeersResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let peers = state
        .engine
        .native_task_peers(&task_id)
        .map_err(|_| NativeApiError::TaskNotFound)?;
    Ok(Json(PeersResponse { peers }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackersResponse {
    trackers: Vec<NativeTrackerSnapshot>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchTrackersRequest {
    tracker_uris: Vec<String>,
    #[serde(default)]
    excluded_tracker_uris: Option<Vec<String>>,
    #[serde(default)]
    connect_timeout_seconds: Option<Option<u64>>,
    #[serde(default)]
    timeout_seconds: Option<Option<u64>>,
    #[serde(default)]
    interval_seconds: Option<Option<u64>>,
}

async fn handle_task_trackers(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
) -> Result<Json<TrackersResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let trackers = state
        .engine
        .native_task_trackers(&task_id)
        .map_err(|_| NativeApiError::TaskNotFound)?;
    Ok(Json(TrackersResponse { trackers }))
}

async fn handle_patch_task_trackers(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Path(task_id): Path<String>,
    Json(request): Json<PatchTrackersRequest>,
) -> Result<Json<TrackersResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    ensure_native_task_exists(&state.engine, &task_id)?;
    state
        .engine
        .update_native_task_trackers(&task_id, &request.tracker_uris)
        .map_err(|_| NativeApiError::InvalidRequest)?;
    let trackers = state
        .engine
        .update_native_task_tracker_policy(
            &task_id,
            request.excluded_tracker_uris.as_deref(),
            request.connect_timeout_seconds,
            request.timeout_seconds,
            request.interval_seconds,
        )
        .map_err(|_| NativeApiError::InvalidRequest)?;
    Ok(Json(TrackersResponse { trackers }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorResponse {
    code: &'static str,
    message: &'static str,
}

#[derive(Debug)]
enum NativeApiError {
    TaskNotFound,
    InvalidTaskId,
    InvalidRequest,
    AuthRequired,
    SessionStoreUnavailable,
}

impl IntoResponse for NativeApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, message) = match self {
            Self::TaskNotFound => (StatusCode::NOT_FOUND, "task_not_found", "task not found"),
            Self::InvalidTaskId => (
                StatusCode::BAD_REQUEST,
                "invalid_task_id",
                "invalid task id",
            ),
            Self::InvalidRequest => (
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "invalid request",
            ),
            Self::AuthRequired => (
                StatusCode::UNAUTHORIZED,
                "auth_required",
                "authentication required",
            ),
            Self::SessionStoreUnavailable => (
                StatusCode::CONFLICT,
                "session_store_unavailable",
                "session store unavailable",
            ),
        };
        (status, Json(ErrorResponse { code, message })).into_response()
    }
}

fn require_auth(state: &NativeApiState, headers: &HeaderMap) -> Result<(), NativeApiError> {
    let Some(expected) = state.auth_token.as_deref() else {
        return Ok(());
    };

    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    if token == Some(expected) {
        Ok(())
    } else {
        Err(NativeApiError::AuthRequired)
    }
}

fn task_summary_by_id(engine: &Engine, task_id: &str) -> Result<NativeTaskSummary, NativeApiError> {
    let task_id = parse_task_id(task_id)?;
    engine
        .native_task_summary(&task_id)
        .map_err(|_| NativeApiError::TaskNotFound)
}

fn parse_task_id(task_id: &str) -> Result<TaskId, NativeApiError> {
    let task_id = TaskId::parse(task_id.to_string()).map_err(|_| NativeApiError::InvalidTaskId)?;
    Ok(task_id)
}

fn parse_file_id(id: &str) -> Result<usize, NativeApiError> {
    id.strip_prefix("file_")
        .and_then(|raw| raw.parse::<usize>().ok())
        .ok_or(NativeApiError::InvalidRequest)
}

fn ensure_native_task_exists(engine: &Engine, task_id: &TaskId) -> Result<(), NativeApiError> {
    engine
        .gid_for_task_id(task_id)
        .map(|_| ())
        .ok_or(NativeApiError::TaskNotFound)
}
