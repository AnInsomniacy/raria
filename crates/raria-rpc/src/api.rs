//! Native raria HTTP JSON API.

use anyhow::{Context, Result};
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use raria_core::engine::{AddUriSpec, Engine};
use raria_core::native::{
    NativeEvent, NativeEventData, NativeEventType, NativeTaskFile, NativeTaskSummary, TaskId,
    TaskSource,
};
use raria_core::progress::DownloadEvent;
use serde::{Deserialize, Serialize};
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
        .route("/api/v1/events", get(handle_events_ws))
        .route("/api/v1/session/save", post(handle_save_session))
        .route("/api/v1/stats", get(handle_stats))
        .route(
            "/api/v1/tasks",
            get(handle_list_tasks).post(handle_create_task),
        )
        .route(
            "/api/v1/tasks/:task_id",
            get(handle_get_task).delete(handle_remove_task),
        )
        .route("/api/v1/tasks/:task_id/pause", post(handle_pause_task))
        .route("/api/v1/tasks/:task_id/restart", post(handle_restart_task))
        .route("/api/v1/tasks/:task_id/resume", post(handle_resume_task))
        .route("/api/v1/tasks/:task_id/files", get(handle_task_files))
        .route("/api/v1/tasks/:task_id/sources", get(handle_task_sources))
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

async fn handle_config(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
) -> Result<Json<RuntimeConfigResponse>, NativeApiError> {
    require_auth(&state, &headers)?;
    let config = &state.engine.config;

    Ok(Json(RuntimeConfigResponse {
        daemon: RuntimeDaemonConfig {
            download_dir: config.dir.clone(),
            session_path: config.session_file.clone(),
            max_active_tasks: config.max_concurrent_downloads,
        },
        downloads: RuntimeDownloadsConfig {
            default_segments: config.split,
            min_segment_size: config.min_split_size,
            retry_max_attempts: config.max_tries,
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
    let mut events = state.engine.event_bus.subscribe();
    let mut sequence = 1u64;

    while let Ok(event) = events.recv().await {
        let Some(native_event) = download_event_to_native(&state.engine, sequence, event) else {
            continue;
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

fn download_event_to_native(
    engine: &Engine,
    sequence: u64,
    event: DownloadEvent,
) -> Option<NativeEvent> {
    match event {
        DownloadEvent::Started { gid } => Some(NativeEvent::new(
            sequence,
            NativeEventType::TaskStarted,
            Some(task_id_for_event(engine, gid)),
            NativeEventData::Empty,
        )),
        DownloadEvent::Paused { gid } => Some(NativeEvent::new(
            sequence,
            NativeEventType::TaskPaused,
            Some(task_id_for_event(engine, gid)),
            NativeEventData::Empty,
        )),
        DownloadEvent::Complete { gid } => Some(NativeEvent::new(
            sequence,
            NativeEventType::TaskCompleted,
            Some(task_id_for_event(engine, gid)),
            NativeEventData::Empty,
        )),
        DownloadEvent::Error { gid, message } => Some(NativeEvent::new(
            sequence,
            NativeEventType::TaskFailed,
            Some(task_id_for_event(engine, gid)),
            NativeEventData::Error {
                code: "task_failed".to_string(),
                message,
            },
        )),
        DownloadEvent::Progress {
            gid,
            downloaded,
            total,
            speed,
        } => Some(NativeEvent::new(
            sequence,
            NativeEventType::TaskProgress,
            Some(task_id_for_event(engine, gid)),
            NativeEventData::Progress {
                completed_bytes: downloaded,
                total_bytes: total,
                download_bytes_per_second: speed,
            },
        )),
        DownloadEvent::SourceFailed { gid, message, .. } => Some(NativeEvent::new(
            sequence,
            NativeEventType::TaskSourceFailed,
            Some(task_id_for_event(engine, gid)),
            NativeEventData::Error {
                code: "source_failed".to_string(),
                message,
            },
        )),
        DownloadEvent::Stopped { gid } => Some(NativeEvent::new(
            sequence,
            NativeEventType::TaskRemoved,
            Some(task_id_for_event(engine, gid)),
            NativeEventData::Empty,
        )),
        DownloadEvent::BtDownloadComplete { gid } => Some(NativeEvent::new(
            sequence,
            NativeEventType::TaskCompleted,
            Some(task_id_for_event(engine, gid)),
            NativeEventData::Empty,
        )),
        DownloadEvent::StatusChanged { .. } => None,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateTaskRequest {
    sources: Vec<String>,
    download_dir: PathBuf,
    filename: Option<String>,
    segments: Option<u32>,
}

async fn handle_create_task(
    headers: HeaderMap,
    State(state): State<NativeApiState>,
    Json(request): Json<CreateTaskRequest>,
) -> Result<Json<NativeTaskSummary>, NativeApiError> {
    require_auth(&state, &headers)?;
    if request.sources.is_empty() {
        return Err(NativeApiError::InvalidRequest);
    }

    let summary = state
        .engine
        .add_native_task(&AddUriSpec {
            uris: request.sources,
            dir: request.download_dir,
            filename: request.filename,
            connections: request.segments.unwrap_or(1).max(1),
        })
        .map_err(|_| NativeApiError::InvalidRequest)?;

    Ok(Json(summary))
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourcesResponse {
    sources: Vec<TaskSource>,
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

fn task_id_for_event(engine: &Engine, gid: raria_core::job::Gid) -> TaskId {
    engine
        .task_id_for_gid(gid)
        .unwrap_or_else(|| TaskId::from_migration_gid(gid.as_raw()))
}
