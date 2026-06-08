mod cli;
mod config;
mod runtime;

pub use cli::{
    CliCommand, InputTask, OptionDisposition, parse_cli, parse_config_text, parse_input_file_text,
    save_session_text,
};
pub use config::RariaConfig;
pub use runtime::RariaRuntime;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("runtime is already shut down")]
    RuntimeStopped,
    #[error("failed to parse CLI arguments: {0}")]
    CliParse(String),
}
