mod config;
mod runtime;

pub use config::RariaConfig;
pub use runtime::RariaRuntime;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("runtime is already shut down")]
    RuntimeStopped,
}
