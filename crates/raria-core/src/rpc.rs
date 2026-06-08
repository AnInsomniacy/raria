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
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{Mutex, broadcast};

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
pub struct HttpTask {
    pub gid: String,
    pub uri: String,
    pub out: Option<String>,
    pub checksum: Option<String>,
    pub header: Option<String>,
    pub load_cookies: Option<String>,
    pub max_download_limit: Option<u32>,
    pub split: Option<u16>,
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
            "aria2.tellStatus" => self.tell_status(call.params),
            "aria2.pause" | "aria2.forcePause" => self.set_status(call.params, "paused"),
            "aria2.unpause" => self.set_status(call.params, "waiting"),
            "aria2.remove" | "aria2.forceRemove" => self.set_status(call.params, "removed"),
            "aria2.getGlobalStat" => Ok(self.global_stat()),
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

    pub fn pending_http_tasks(&self) -> Vec<HttpTask> {
        self.tasks
            .values()
            .filter(|task| task.status == "waiting")
            .filter_map(|task| {
                let uri = task
                    .uris
                    .iter()
                    .find(|uri| uri.starts_with("http://") || uri.starts_with("https://"))?;
                Some(HttpTask {
                    gid: task.gid.clone(),
                    uri: uri.clone(),
                    out: task.out.clone(),
                    checksum: task.checksum.clone(),
                    header: task.header.clone(),
                    load_cookies: task.load_cookies.clone(),
                    max_download_limit: task.max_download_limit,
                    split: task.split,
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

    fn tell_status(&self, params: RpcValue) -> Result<RpcValue, RpcError> {
        let gid = gid_param(params)?;
        let task = self
            .tasks
            .get(&gid)
            .ok_or_else(|| RpcError::invalid_params(format!("unknown gid: {gid}")))?;

        let mut status = BTreeMap::from([
            ("gid".to_string(), RpcValue::string(&task.gid)),
            ("status".to_string(), RpcValue::string(&task.status)),
            ("totalLength".to_string(), RpcValue::string("0")),
            (
                "completedLength".to_string(),
                RpcValue::string(task.completed_length.to_string()),
            ),
            ("downloadSpeed".to_string(), RpcValue::string("0")),
            ("uploadSpeed".to_string(), RpcValue::string("0")),
        ]);
        status.insert(
            "files".into(),
            RpcValue::array([RpcValue::object([(
                "uris",
                RpcValue::Array(
                    task.uris
                        .iter()
                        .map(|uri| RpcValue::object([("uri", RpcValue::string(uri))]))
                        .collect(),
                ),
            )])]),
        );
        if let Some(message) = &task.error_message {
            status.insert("errorMessage".into(), RpcValue::string(message));
        }
        Ok(RpcValue::Object(status))
    }

    fn set_status(&mut self, params: RpcValue, status: &str) -> Result<RpcValue, RpcError> {
        let gid = gid_param(params)?;
        let task = self
            .tasks
            .get_mut(&gid)
            .ok_or_else(|| RpcError::invalid_params(format!("unknown gid: {gid}")))?;
        task.status = status.into();
        Ok(RpcValue::string(gid))
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

    fn multicall(&mut self, params: RpcValue) -> Result<RpcValue, RpcError> {
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
    completed_length: u64,
    error_message: Option<String>,
}

fn gid_param(params: RpcValue) -> Result<String, RpcError> {
    params
        .as_array()
        .and_then(|params| params.first())
        .and_then(RpcValue::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| RpcError::invalid_params("expected gid parameter"))
}
