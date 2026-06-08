use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::post,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{Mutex, broadcast};

use crate::{
    MagnetMeta, TorrentFile, TorrentMeta, parse_magnet_uri, parse_metalink_bytes,
    parse_torrent_bytes,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RpcCall {
    pub method: String,
    pub params: RpcValue,
}

impl RpcCall {
    pub fn new(method: impl Into<String>, params: RpcValue) -> Self {
        Self {
            method: method.into(),
            params,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RpcEvent {
    pub method: String,
    pub params: Vec<RpcValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadTask {
    pub gid: String,
    pub uri: String,
    pub out: Option<String>,
    pub checksum: Option<String>,
    pub header: Option<String>,
    pub load_cookies: Option<String>,
    pub max_download_limit: Option<u32>,
    pub split: Option<u16>,
    pub netrc_path: Option<String>,
    pub http_proxy: Option<String>,
    pub ftp_user: Option<String>,
    pub ftp_passwd: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BittorrentDownloadTask {
    pub gid: String,
    pub torrent_bytes: Option<Vec<u8>>,
    pub magnet_uri: Option<String>,
    pub selected_files: Option<Vec<usize>>,
    pub initial_peers: Vec<String>,
}

impl RpcEvent {
    fn into_json(self) -> Value {
        json!({
            "jsonrpc": "2.0",
            "method": self.method,
            "params": self.params.into_iter().map(RpcValue::into_json).collect::<Vec<_>>(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RpcError {
    pub code: RpcErrorCode,
    pub message: String,
}

impl RpcError {
    fn unsupported_phase_one(method: &str) -> Self {
        Self {
            code: RpcErrorCode::UnsupportedPhaseOne,
            message: format!("{method} is not implemented in raria phase one"),
        }
    }

    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: RpcErrorCode::InvalidParams,
            message: message.into(),
        }
    }

    fn method_not_found(method: &str) -> Self {
        Self {
            code: RpcErrorCode::MethodNotFound,
            message: format!("unknown RPC method: {method}"),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RpcErrorCode {
    MethodNotFound,
    InvalidParams,
    UnsupportedPhaseOne,
}

impl RpcErrorCode {
    fn aria2_code(self) -> i64 {
        match self {
            Self::MethodNotFound | Self::InvalidParams | Self::UnsupportedPhaseOne => 1,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RpcValue {
    Null,
    String(String),
    Array(Vec<RpcValue>),
    Object(BTreeMap<String, RpcValue>),
}

impl RpcValue {
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(value.into())
    }

    pub fn array<const N: usize>(values: [RpcValue; N]) -> Self {
        Self::Array(values.into())
    }

    pub fn object<const N: usize>(values: [(&str, RpcValue); N]) -> Self {
        Self::Object(
            values
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[RpcValue]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&RpcValue> {
        match self {
            Self::Object(values) => values.get(key),
            _ => None,
        }
    }

    fn from_json(value: Value) -> Self {
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => Self::Null,
            Value::String(value) => Self::String(value),
            Value::Array(values) => Self::Array(values.into_iter().map(Self::from_json).collect()),
            Value::Object(values) => Self::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, Self::from_json(value)))
                    .collect(),
            ),
        }
    }

    fn into_json(self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::String(value) => Value::String(value),
            Self::Array(values) => Value::Array(values.into_iter().map(Self::into_json).collect()),
            Self::Object(values) => Value::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, value.into_json()))
                    .collect(),
            ),
        }
    }
}

pub struct RpcEngine {
    next_gid: u64,
    tasks: BTreeMap<String, Task>,
    events: Vec<RpcEvent>,
    event_tx: broadcast::Sender<RpcEvent>,
}

impl Default for RpcEngine {
    fn default() -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            next_gid: 0,
            tasks: BTreeMap::new(),
            events: Vec::new(),
            event_tx,
        }
    }
}

pub type SharedRpcEngine = Arc<Mutex<RpcEngine>>;

pub fn build_rpc_router(engine: SharedRpcEngine) -> Router {
    Router::new()
        .route("/jsonrpc", post(handle_jsonrpc).get(handle_websocket))
        .with_state(engine)
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcSuccess {
    jsonrpc: &'static str,
    id: Option<Value>,
    result: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcFailure {
    jsonrpc: &'static str,
    id: Option<Value>,
    error: JsonRpcErrorBody,
}

#[derive(Debug, Serialize)]
struct JsonRpcErrorBody {
    code: i64,
    message: String,
}

async fn handle_jsonrpc(
    State(engine): State<SharedRpcEngine>,
    Json(request): Json<JsonRpcRequest>,
) -> Json<Value> {
    let call = RpcCall::new(request.method, RpcValue::from_json(request.params));
    let mut engine = engine.lock().await;
    match engine.call(call) {
        Ok(result) => Json(json!(JsonRpcSuccess {
            jsonrpc: "2.0",
            id: request.id,
            result: result.into_json(),
        })),
        Err(error) => Json(json!(JsonRpcFailure {
            jsonrpc: "2.0",
            id: request.id,
            error: JsonRpcErrorBody {
                code: error.code.aria2_code(),
                message: error.message,
            },
        })),
    }
}

async fn handle_websocket(
    State(engine): State<SharedRpcEngine>,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    let rx = {
        let engine = engine.lock().await;
        engine.subscribe_events()
    };
    upgrade.on_upgrade(move |socket| stream_events(socket, rx))
}

async fn stream_events(mut socket: WebSocket, mut rx: broadcast::Receiver<RpcEvent>) {
    while let Ok(event) = rx.recv().await {
        if socket
            .send(Message::Text(event.into_json().to_string().into()))
            .await
            .is_err()
        {
            break;
        }
    }
}

impl RpcEngine {
    pub fn call(&mut self, call: RpcCall) -> Result<RpcValue, RpcError> {
        match call.method.as_str() {
            "aria2.addUri" => self.add_uri(call.params),
            "aria2.addTorrent" => self.add_torrent(call.params),
            "aria2.addMetalink" => self.add_metalink(call.params),
            "aria2.tellStatus" => self.tell_status(call.params),
            "aria2.getFiles" => self.get_files(call.params),
            "aria2.getUris" => self.get_uris(call.params),
            "aria2.tellActive" => Ok(self.tell_by_status("active")),
            "aria2.tellWaiting" => Ok(self.tell_by_status("waiting")),
            "aria2.tellStopped" => Ok(self.tell_stopped()),
            "aria2.pause" | "aria2.forcePause" => self.set_status(call.params, "paused"),
            "aria2.unpause" => self.set_status(call.params, "waiting"),
            "aria2.remove" | "aria2.forceRemove" => self.set_status(call.params, "removed"),
            "aria2.getGlobalStat" => Ok(self.global_stat()),
            "aria2.getVersion" => Ok(self.version()),
            "aria2.getSessionInfo" => Ok(RpcValue::object([(
                "sessionId",
                RpcValue::string("raria-new-session"),
            )])),
            "aria2.saveSession" => Ok(RpcValue::string("OK")),
            "system.multicall" => self.multicall(call.params),
            "system.listMethods" => Ok(RpcValue::Array(
                RPC_METHODS
                    .iter()
                    .map(|method| RpcValue::string(*method))
                    .collect(),
            )),
            "system.listNotifications" => Ok(RpcValue::Array(
                RPC_NOTIFICATIONS
                    .iter()
                    .map(|method| RpcValue::string(*method))
                    .collect(),
            )),
            "aria2.ed2kSearch" | "aria2.getEd2kSearchResults" => {
                Err(RpcError::unsupported_phase_one(&call.method))
            }
            _ => Err(RpcError::method_not_found(&call.method)),
        }
    }

    pub fn drain_events(&mut self) -> Vec<RpcEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn pending_http_tasks(&self) -> Vec<DownloadTask> {
        self.pending_tasks_with_scheme(|uri| {
            uri.starts_with("http://") || uri.starts_with("https://")
        })
    }

    pub fn pending_ftp_tasks(&self) -> Vec<DownloadTask> {
        self.pending_tasks_with_scheme(|uri| uri.starts_with("ftp://"))
    }

    pub fn pending_sftp_tasks(&self) -> Vec<DownloadTask> {
        self.pending_tasks_with_scheme(|uri| uri.starts_with("sftp://"))
    }

    pub fn pending_bittorrent_tasks(&self) -> Vec<BittorrentDownloadTask> {
        self.tasks
            .values()
            .filter(|task| task.status == "waiting")
            .filter_map(|task| {
                let bittorrent = task.bittorrent.as_ref()?;
                Some(BittorrentDownloadTask {
                    gid: task.gid.clone(),
                    torrent_bytes: bittorrent.torrent_bytes.clone(),
                    magnet_uri: bittorrent.magnet_uri.clone(),
                    selected_files: bittorrent.selected_files.clone(),
                    initial_peers: bittorrent.initial_peers.clone(),
                })
            })
            .collect()
    }

    fn pending_tasks_with_scheme(
        &self,
        matches_scheme: impl Fn(&str) -> bool,
    ) -> Vec<DownloadTask> {
        self.tasks
            .values()
            .filter(|task| task.status == "waiting")
            .filter_map(|task| {
                let uri = task.uris.iter().find(|uri| matches_scheme(uri))?;
                Some(DownloadTask {
                    gid: task.gid.clone(),
                    uri: uri.clone(),
                    out: task.out.clone(),
                    checksum: task.checksum.clone(),
                    header: task.header.clone(),
                    load_cookies: task.load_cookies.clone(),
                    max_download_limit: task.max_download_limit,
                    split: task.split,
                    netrc_path: task.netrc_path.clone(),
                    http_proxy: task.http_proxy.clone(),
                    ftp_user: task.ftp_user.clone(),
                    ftp_passwd: task.ftp_passwd.clone(),
                })
            })
            .collect()
    }

    pub fn complete_task(&mut self, gid: &str, completed_length: u64) -> Result<(), RpcError> {
        let task = self
            .tasks
            .get_mut(gid)
            .ok_or_else(|| RpcError::invalid_params(format!("unknown gid: {gid}")))?;
        task.status = "complete".into();
        task.completed_length = completed_length;
        self.emit_event(RpcEvent {
            method: "aria2.onDownloadComplete".into(),
            params: vec![RpcValue::object([("gid", RpcValue::string(gid))])],
        });
        Ok(())
    }

    pub fn fail_task(&mut self, gid: &str, message: String) -> Result<(), RpcError> {
        let task = self
            .tasks
            .get_mut(gid)
            .ok_or_else(|| RpcError::invalid_params(format!("unknown gid: {gid}")))?;
        task.status = "error".into();
        task.error_message = Some(message);
        self.emit_event(RpcEvent {
            method: "aria2.onDownloadError".into(),
            params: vec![RpcValue::object([("gid", RpcValue::string(gid))])],
        });
        Ok(())
    }

    fn subscribe_events(&self) -> broadcast::Receiver<RpcEvent> {
        self.event_tx.subscribe()
    }

    fn add_uri(&mut self, params: RpcValue) -> Result<RpcValue, RpcError> {
        let params = strip_token(params);
        let uris = params
            .as_array()
            .and_then(|params| params.first())
            .and_then(RpcValue::as_array)
            .ok_or_else(|| RpcError::invalid_params("aria2.addUri expects an URI array"))?;
        let uris = uris
            .iter()
            .map(|uri| {
                uri.as_str()
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| RpcError::invalid_params("URI must be a string"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let out = params
            .as_array()
            .and_then(|params| params.get(1))
            .and_then(|options| options.get("out"))
            .and_then(RpcValue::as_str)
            .map(ToOwned::to_owned);
        let checksum = params
            .as_array()
            .and_then(|params| params.get(1))
            .and_then(|options| options.get("checksum"))
            .and_then(RpcValue::as_str)
            .map(ToOwned::to_owned);
        let header = params
            .as_array()
            .and_then(|params| params.get(1))
            .and_then(|options| options.get("header"))
            .and_then(RpcValue::as_str)
            .map(ToOwned::to_owned);
        let load_cookies = params
            .as_array()
            .and_then(|params| params.get(1))
            .and_then(|options| options.get("load-cookies"))
            .and_then(RpcValue::as_str)
            .map(ToOwned::to_owned);
        let max_download_limit = params
            .as_array()
            .and_then(|params| params.get(1))
            .and_then(|options| options.get("max-download-limit"))
            .and_then(RpcValue::as_str)
            .and_then(|value| value.parse::<u32>().ok());
        let split = params
            .as_array()
            .and_then(|params| params.get(1))
            .and_then(|options| options.get("split"))
            .and_then(RpcValue::as_str)
            .and_then(|value| value.parse::<u16>().ok());
        let netrc_path = params
            .as_array()
            .and_then(|params| params.get(1))
            .and_then(|options| options.get("netrc-path"))
            .and_then(RpcValue::as_str)
            .map(ToOwned::to_owned);
        let http_proxy = params
            .as_array()
            .and_then(|params| params.get(1))
            .and_then(|options| options.get("http-proxy"))
            .and_then(RpcValue::as_str)
            .map(ToOwned::to_owned);
        let ftp_user = params
            .as_array()
            .and_then(|params| params.get(1))
            .and_then(|options| options.get("ftp-user"))
            .and_then(RpcValue::as_str)
            .map(ToOwned::to_owned);
        let ftp_passwd = params
            .as_array()
            .and_then(|params| params.get(1))
            .and_then(|options| options.get("ftp-passwd"))
            .and_then(RpcValue::as_str)
            .map(ToOwned::to_owned);

        let initial_peers = initial_peers(params.as_array().and_then(|params| params.get(1)))?;
        let bittorrent = uris
            .iter()
            .find(|uri| uri.starts_with("magnet:"))
            .map(|uri| torrent_from_magnet(uri, initial_peers.clone()))
            .transpose()?;
        let gid = self.allocate_gid();
        self.tasks.insert(
            gid.clone(),
            Task {
                gid: gid.clone(),
                status: "waiting".into(),
                uris,
                out,
                checksum,
                header,
                load_cookies,
                max_download_limit,
                split,
                netrc_path,
                http_proxy,
                ftp_user,
                ftp_passwd,
                bittorrent,
                completed_length: 0,
                error_message: None,
            },
        );
        self.emit_event(RpcEvent {
            method: "aria2.onDownloadStart".into(),
            params: vec![RpcValue::object([("gid", RpcValue::string(&gid))])],
        });
        Ok(RpcValue::string(gid))
    }

    fn add_torrent(&mut self, params: RpcValue) -> Result<RpcValue, RpcError> {
        let params = strip_token(params);
        let encoded = params
            .as_array()
            .and_then(|params| params.first())
            .and_then(RpcValue::as_str)
            .ok_or_else(|| RpcError::invalid_params("aria2.addTorrent expects torrent bytes"))?;
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|error| RpcError::invalid_params(error.to_string()))?;
        let meta = parse_torrent_bytes(&bytes)
            .map_err(|error| RpcError::invalid_params(error.to_string()))?;
        let selected_files = selected_files(params.as_array().and_then(|params| params.get(2)))?;
        let initial_peers = initial_peers(params.as_array().and_then(|params| params.get(2)))?;
        let gid = self.allocate_gid();
        self.tasks.insert(
            gid.clone(),
            Task {
                gid: gid.clone(),
                status: "waiting".into(),
                uris: Vec::new(),
                out: None,
                checksum: None,
                header: None,
                load_cookies: None,
                max_download_limit: None,
                split: None,
                netrc_path: None,
                http_proxy: None,
                ftp_user: None,
                ftp_passwd: None,
                bittorrent: Some(BittorrentTask::from_torrent(
                    meta,
                    bytes,
                    selected_files,
                    initial_peers,
                )),
                completed_length: 0,
                error_message: None,
            },
        );
        self.emit_event(RpcEvent {
            method: "aria2.onDownloadStart".into(),
            params: vec![RpcValue::object([("gid", RpcValue::string(&gid))])],
        });
        Ok(RpcValue::string(gid))
    }

    fn add_metalink(&mut self, params: RpcValue) -> Result<RpcValue, RpcError> {
        let params = strip_token(params);
        let encoded = params
            .as_array()
            .and_then(|params| params.first())
            .and_then(RpcValue::as_str)
            .ok_or_else(|| RpcError::invalid_params("aria2.addMetalink expects Metalink bytes"))?;
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|error| RpcError::invalid_params(error.to_string()))?;
        let metalink = parse_metalink_bytes(&bytes)
            .map_err(|error| RpcError::invalid_params(error.to_string()))?;
        let mut gids = Vec::new();
        for file in metalink.files {
            let Some(uri) = file.resources.first().cloned() else {
                continue;
            };
            let gid = self.allocate_gid();
            self.tasks.insert(
                gid.clone(),
                Task {
                    gid: gid.clone(),
                    status: "waiting".into(),
                    uris: vec![uri],
                    out: Some(file.name),
                    checksum: file.checksum,
                    header: None,
                    load_cookies: None,
                    max_download_limit: None,
                    split: None,
                    netrc_path: None,
                    http_proxy: None,
                    ftp_user: None,
                    ftp_passwd: None,
                    bittorrent: None,
                    completed_length: 0,
                    error_message: None,
                },
            );
            self.emit_event(RpcEvent {
                method: "aria2.onDownloadStart".into(),
                params: vec![RpcValue::object([("gid", RpcValue::string(&gid))])],
            });
            gids.push(RpcValue::string(gid));
        }
        Ok(RpcValue::Array(gids))
    }

    fn tell_status(&self, params: RpcValue) -> Result<RpcValue, RpcError> {
        let gid = gid_param(strip_token(params))?;
        let task = self
            .tasks
            .get(&gid)
            .ok_or_else(|| RpcError::invalid_params(format!("unknown gid: {gid}")))?;
        Ok(task.status_value())
    }

    fn set_status(&mut self, params: RpcValue, status: &str) -> Result<RpcValue, RpcError> {
        let gid = gid_param(strip_token(params))?;
        let task = self
            .tasks
            .get_mut(&gid)
            .ok_or_else(|| RpcError::invalid_params(format!("unknown gid: {gid}")))?;
        task.status = status.into();
        Ok(RpcValue::string(gid))
    }

    fn get_files(&self, params: RpcValue) -> Result<RpcValue, RpcError> {
        let gid = gid_param(strip_token(params))?;
        let task = self
            .tasks
            .get(&gid)
            .ok_or_else(|| RpcError::invalid_params(format!("unknown gid: {gid}")))?;
        Ok(RpcValue::Array(task.files_value()))
    }

    fn get_uris(&self, params: RpcValue) -> Result<RpcValue, RpcError> {
        let gid = gid_param(strip_token(params))?;
        let task = self
            .tasks
            .get(&gid)
            .ok_or_else(|| RpcError::invalid_params(format!("unknown gid: {gid}")))?;
        Ok(RpcValue::Array(
            task.uris
                .iter()
                .map(|uri| {
                    RpcValue::object([
                        ("uri", RpcValue::string(uri)),
                        ("status", RpcValue::string("used")),
                    ])
                })
                .collect(),
        ))
    }

    fn global_stat(&self) -> RpcValue {
        let num_waiting = self
            .tasks
            .values()
            .filter(|task| task.status == "waiting")
            .count();
        RpcValue::object([
            ("downloadSpeed", RpcValue::string("0")),
            ("uploadSpeed", RpcValue::string("0")),
            ("numActive", RpcValue::string("0")),
            ("numWaiting", RpcValue::string(num_waiting.to_string())),
            ("numStopped", RpcValue::string("0")),
        ])
    }

    fn tell_by_status(&self, status: &str) -> RpcValue {
        RpcValue::Array(
            self.tasks
                .values()
                .filter(|task| task.status == status)
                .map(Task::status_value)
                .collect(),
        )
    }

    fn tell_stopped(&self) -> RpcValue {
        RpcValue::Array(
            self.tasks
                .values()
                .filter(|task| matches!(task.status.as_str(), "removed" | "complete" | "error"))
                .map(Task::status_value)
                .collect(),
        )
    }

    fn version(&self) -> RpcValue {
        RpcValue::object([
            ("version", RpcValue::string(env!("CARGO_PKG_VERSION"))),
            (
                "enabledFeatures",
                RpcValue::array([
                    RpcValue::string("Async DNS"),
                    RpcValue::string("BitTorrent"),
                    RpcValue::string("Firefox3 Cookie"),
                    RpcValue::string("HTTPS"),
                    RpcValue::string("Metalink"),
                    RpcValue::string("SFTP"),
                ]),
            ),
        ])
    }

    fn multicall(&mut self, params: RpcValue) -> Result<RpcValue, RpcError> {
        let params = strip_token(params);
        let calls = params
            .as_array()
            .and_then(|params| params.first())
            .and_then(RpcValue::as_array)
            .ok_or_else(|| RpcError::invalid_params("system.multicall expects call array"))?;
        let calls = calls.to_vec();
        let mut results = Vec::with_capacity(calls.len());
        for call in calls {
            let method = call
                .get("methodName")
                .and_then(RpcValue::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing methodName"))?;
            let params = call
                .get("params")
                .cloned()
                .unwrap_or_else(|| RpcValue::array([]));
            let value = self.call(RpcCall::new(method, params))?;
            results.push(RpcValue::array([value]));
        }
        Ok(RpcValue::Array(results))
    }

    fn allocate_gid(&mut self) -> String {
        self.next_gid += 1;
        format!("{:016x}", self.next_gid)
    }

    fn emit_event(&mut self, event: RpcEvent) {
        self.events.push(event.clone());
        let _ = self.event_tx.send(event);
    }
}

const RPC_METHODS: &[&str] = &[
    "aria2.addUri",
    "aria2.ed2kSearch",
    "aria2.getEd2kSearchResults",
    "aria2.addTorrent",
    "aria2.getPeers",
    "aria2.addMetalink",
    "aria2.remove",
    "aria2.pause",
    "aria2.forcePause",
    "aria2.pauseAll",
    "aria2.forcePauseAll",
    "aria2.unpause",
    "aria2.unpauseAll",
    "aria2.forceRemove",
    "aria2.changePosition",
    "aria2.tellStatus",
    "aria2.getUris",
    "aria2.getFiles",
    "aria2.getServers",
    "aria2.tellActive",
    "aria2.tellWaiting",
    "aria2.tellStopped",
    "aria2.getOption",
    "aria2.changeUri",
    "aria2.changeOption",
    "aria2.getGlobalOption",
    "aria2.changeGlobalOption",
    "aria2.purgeDownloadResult",
    "aria2.removeDownloadResult",
    "aria2.getVersion",
    "aria2.getSessionInfo",
    "aria2.shutdown",
    "aria2.forceShutdown",
    "aria2.getGlobalStat",
    "aria2.saveSession",
    "system.multicall",
    "system.listMethods",
    "system.listNotifications",
];

const RPC_NOTIFICATIONS: &[&str] = &[
    "aria2.onDownloadStart",
    "aria2.onDownloadPause",
    "aria2.onDownloadStop",
    "aria2.onDownloadComplete",
    "aria2.onDownloadError",
    "aria2.onBtDownloadComplete",
];

#[derive(Clone, Debug, Eq, PartialEq)]
struct Task {
    gid: String,
    status: String,
    uris: Vec<String>,
    out: Option<String>,
    checksum: Option<String>,
    header: Option<String>,
    load_cookies: Option<String>,
    max_download_limit: Option<u32>,
    split: Option<u16>,
    netrc_path: Option<String>,
    http_proxy: Option<String>,
    ftp_user: Option<String>,
    ftp_passwd: Option<String>,
    bittorrent: Option<BittorrentTask>,
    completed_length: u64,
    error_message: Option<String>,
}

impl Task {
    fn total_length(&self) -> u64 {
        self.bittorrent
            .as_ref()
            .map(|bittorrent| bittorrent.total_length)
            .unwrap_or(0)
    }

    fn files_value(&self) -> Vec<RpcValue> {
        if let Some(bittorrent) = &self.bittorrent {
            return bittorrent
                .files
                .iter()
                .enumerate()
                .map(|(index, file)| {
                    let selected = bittorrent
                        .selected_files
                        .as_ref()
                        .map(|selected| selected.contains(&(index + 1)))
                        .unwrap_or(true);
                    RpcValue::object([
                        ("index", RpcValue::string((index + 1).to_string())),
                        ("path", RpcValue::string(&file.path)),
                        ("length", RpcValue::string(file.length.to_string())),
                        ("completedLength", RpcValue::string("0")),
                        ("selected", RpcValue::string(selected.to_string())),
                        ("uris", RpcValue::array([])),
                    ])
                })
                .collect();
        }
        vec![RpcValue::object([(
            "uris",
            RpcValue::Array(
                self.uris
                    .iter()
                    .map(|uri| RpcValue::object([("uri", RpcValue::string(uri))]))
                    .collect(),
            ),
        )])]
    }

    fn status_value(&self) -> RpcValue {
        let mut status = BTreeMap::from([
            ("gid".to_string(), RpcValue::string(&self.gid)),
            ("status".to_string(), RpcValue::string(&self.status)),
            (
                "totalLength".to_string(),
                RpcValue::string(self.total_length().to_string()),
            ),
            (
                "completedLength".to_string(),
                RpcValue::string(self.completed_length.to_string()),
            ),
            ("downloadSpeed".to_string(), RpcValue::string("0")),
            ("uploadSpeed".to_string(), RpcValue::string("0")),
            ("files".to_string(), RpcValue::Array(self.files_value())),
        ]);
        if let Some(bittorrent) = &self.bittorrent {
            status.insert(
                "infoHash".into(),
                RpcValue::string(&bittorrent.info_hash_hex),
            );
            status.insert("bittorrent".into(), bittorrent.status_value());
        }
        if let Some(message) = &self.error_message {
            status.insert("errorMessage".into(), RpcValue::string(message));
        }
        RpcValue::Object(status)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BittorrentTask {
    info_hash_hex: String,
    name: Option<String>,
    total_length: u64,
    files: Vec<TorrentFile>,
    selected_files: Option<Vec<usize>>,
    torrent_bytes: Option<Vec<u8>>,
    magnet_uri: Option<String>,
    initial_peers: Vec<String>,
}

impl BittorrentTask {
    fn from_torrent(
        meta: TorrentMeta,
        torrent_bytes: Vec<u8>,
        selected_files: Option<Vec<usize>>,
        initial_peers: Vec<String>,
    ) -> Self {
        Self {
            info_hash_hex: meta.info_hash_hex,
            name: Some(meta.name),
            total_length: meta.total_length,
            files: meta.files,
            selected_files,
            torrent_bytes: Some(torrent_bytes),
            magnet_uri: None,
            initial_peers,
        }
    }

    fn from_magnet(uri: &str, meta: MagnetMeta) -> Self {
        Self {
            info_hash_hex: meta.info_hash_hex,
            name: meta.name,
            total_length: 0,
            files: Vec::new(),
            selected_files: None,
            torrent_bytes: None,
            magnet_uri: Some(uri.to_owned()),
            initial_peers: Vec::new(),
        }
    }

    fn status_value(&self) -> RpcValue {
        let name = self.name.as_deref().unwrap_or("");
        RpcValue::object([("info", RpcValue::object([("name", RpcValue::string(name))]))])
    }
}

fn selected_files(options: Option<&RpcValue>) -> Result<Option<Vec<usize>>, RpcError> {
    let Some(value) = options
        .and_then(|options| options.get("select-file"))
        .and_then(RpcValue::as_str)
    else {
        return Ok(None);
    };
    let mut selected = Vec::new();
    for part in value.split(',') {
        if let Some((start, end)) = part.split_once('-') {
            let start = start
                .parse::<usize>()
                .map_err(|error| RpcError::invalid_params(error.to_string()))?;
            let end = end
                .parse::<usize>()
                .map_err(|error| RpcError::invalid_params(error.to_string()))?;
            selected.extend(start..=end);
        } else {
            selected.push(
                part.parse::<usize>()
                    .map_err(|error| RpcError::invalid_params(error.to_string()))?,
            );
        }
    }
    Ok(Some(selected))
}

fn initial_peers(options: Option<&RpcValue>) -> Result<Vec<String>, RpcError> {
    let Some(value) = options
        .and_then(|options| options.get("bt-initial-peer"))
        .and_then(RpcValue::as_str)
    else {
        return Ok(Vec::new());
    };
    Ok(value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn torrent_from_magnet(uri: &str, initial_peers: Vec<String>) -> Result<BittorrentTask, RpcError> {
    parse_magnet_uri(uri)
        .map(|meta| {
            let mut task = BittorrentTask::from_magnet(uri, meta);
            task.initial_peers = initial_peers;
            task
        })
        .map_err(|error| RpcError::invalid_params(error.to_string()))
}

fn gid_param(params: RpcValue) -> Result<String, RpcError> {
    params
        .as_array()
        .and_then(|params| params.first())
        .and_then(RpcValue::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| RpcError::invalid_params("expected gid parameter"))
}

fn strip_token(params: RpcValue) -> RpcValue {
    let RpcValue::Array(values) = params else {
        return params;
    };
    let mut values = values;
    if values
        .first()
        .and_then(RpcValue::as_str)
        .map(|value| value.starts_with("token:"))
        .unwrap_or(false)
    {
        values.remove(0);
    }
    RpcValue::Array(values)
}
