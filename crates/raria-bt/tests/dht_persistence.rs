use raria_bt::service::{BtServiceConfig, native_session_options};
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn dht_persistence_contract_wires_custom_config_path_into_session_options() {
    let download_dir = tempdir().expect("download tempdir");
    let dht_config = download_dir.path().join("dht-state.json");

    let options = native_session_options(
        download_dir.path(),
        &BtServiceConfig {
            disable_dht: false,
            disable_dht_persistence: false,
            dht_config_filename: Some(dht_config.clone()),
            ..Default::default()
        },
    );

    assert!(
        !options.disable_dht,
        "DHT must remain enabled for persistence coverage"
    );
    assert!(
        !options.disable_dht_persistence,
        "DHT persistence must remain enabled for the contract test"
    );

    let persistent = options
        .dht_config
        .expect("DHT persistence config must be present");
    assert_eq!(
        persistent.config_filename,
        Some(PathBuf::from(&dht_config)),
        "custom DHT persistence path must be forwarded into the BT session options"
    );

    match options
        .persistence
        .expect("session persistence must remain enabled")
    {
        librqbit::SessionPersistenceConfig::Json { folder } => {
            assert_eq!(
                folder,
                Some(download_dir.path().join(".raria-bt-session")),
                "default BT session persistence directory should remain download-dir scoped"
            );
        }
    }
}

#[test]
fn bt_session_persistence_contract_accepts_native_raria_state_dir() {
    let download_dir = tempdir().expect("download tempdir");
    let native_state_dir = download_dir.path().join("native-state/bt-session");

    let options = native_session_options(
        download_dir.path(),
        &BtServiceConfig {
            disable_dht: true,
            disable_dht_persistence: true,
            session_persistence_dir: Some(native_state_dir.clone()),
            ..Default::default()
        },
    );

    assert!(
        options.fastresume,
        "BT session persistence must keep librqbit fastresume enabled"
    );
    match options
        .persistence
        .expect("session persistence must remain enabled")
    {
        librqbit::SessionPersistenceConfig::Json { folder } => {
            assert_eq!(
                folder,
                Some(native_state_dir),
                "BT fastresume state must be bound to the raria-native state directory when configured"
            );
        }
    }
}
