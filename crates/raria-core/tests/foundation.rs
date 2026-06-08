use raria_core::{RariaConfig, RariaRuntime, Result};

#[test]
fn default_config_matches_new_session_contract() {
    let config = RariaConfig::default();

    assert_eq!(config.rpc_listen_port, 6800);
    assert!(!config.rpc_listen_all);
    assert_eq!(config.control_file_extension, ".raria");
}

#[tokio::test]
async fn runtime_starts_and_shuts_down_without_downloads() -> Result<()> {
    let runtime = RariaRuntime::start().await?;

    runtime.shutdown().await?;

    Ok(())
}
