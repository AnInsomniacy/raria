mod cli;
mod config;
mod http_engine;
mod rpc;
mod runtime;

pub use cli::{
    CliCommand, InputTask, OptionDisposition, parse_cli, parse_config_text, parse_input_file_text,
    save_session_text,
};
pub use config::RariaConfig;
pub use http_engine::DownloadEngine;
pub use rpc::{
    HttpTask, RpcCall, RpcEngine, RpcError, RpcErrorCode, RpcEvent, RpcValue, build_rpc_router,
};
pub use runtime::RariaRuntime;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("runtime is already shut down")]
    RuntimeStopped,
    #[error("failed to parse CLI arguments: {0}")]
    CliParse(String),
    #[error("download failed: {0}")]
    Download(String),
}
