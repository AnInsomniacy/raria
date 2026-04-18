use librqbit_dht::{PersistentDht, PersistentDhtConfig};
use raria_bt::service::{BtServiceConfig, parity_contract_session_options};
use serde_json::Value;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::tempdir;
use tokio::time::{sleep, timeout};

fn file_has_data(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
}

async fn wait_for_persisted_json(path: &Path) -> Value {
    timeout(Duration::from_secs(5), async {
        loop {
            if file_has_data(path) {
                let bytes = fs::read(path).expect("read DHT persistence file");
                let value: Value =
                    serde_json::from_slice(&bytes).expect("parse DHT persistence JSON");
                return value;
            }
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("DHT persistence file should be written")
}

async fn wait_for_udp_port_release(addr: SocketAddr) {
    timeout(Duration::from_secs(5), async {
        loop {
            match std::net::UdpSocket::bind(addr) {
                Ok(socket) => {
                    drop(socket);
                    return;
                }
                Err(_) => sleep(Duration::from_millis(25)).await,
            }
        }
    })
    .await
    .expect("DHT UDP listen port should be released after cancellation");
}

fn strip_transient_routing_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("last_refreshed");
            for nested in map.values_mut() {
                strip_transient_routing_fields(nested);
            }
        }
        Value::Array(items) => {
            for nested in items {
                strip_transient_routing_fields(nested);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[test]
fn dht_persistence_contract_wires_custom_config_path_into_session_options() {
    let download_dir = tempdir().expect("download tempdir");
    let dht_config = download_dir.path().join("dht-state.json");

    let options = parity_contract_session_options(
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
                "BT session persistence directory must stay stable when DHT persistence is enabled"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn dht_persistence_dump_survives_restart_and_restores_persisted_state() {
    let download_dir = tempdir().expect("download tempdir");
    let dht_config = download_dir.path().join("dht-state.json");

    let dht = PersistentDht::create(
        Some(PersistentDhtConfig {
            dump_interval: Some(Duration::from_millis(100)),
            config_filename: Some(dht_config.clone()),
        }),
        None,
    )
    .await
    .expect("create persistent DHT");

    let first_addr = dht.listen_addr();
    let persisted = wait_for_persisted_json(&dht_config).await;
    assert!(
        persisted.get("table").is_some(),
        "persisted DHT state must include the routing table"
    );
    assert!(
        persisted.get("peer_store").is_some(),
        "persisted DHT state must include the peer store field"
    );

    dht.cancellation_token().cancel();
    wait_for_udp_port_release(first_addr).await;

    let restored = PersistentDht::create(
        Some(PersistentDhtConfig {
            dump_interval: Some(Duration::from_millis(100)),
            config_filename: Some(dht_config.clone()),
        }),
        None,
    )
    .await
    .expect("restore persistent DHT");

    assert_eq!(
        restored.listen_addr(),
        first_addr,
        "persistent DHT should restore the previous listen address from disk"
    );

    let mut restored_table = restored
        .with_routing_table(|table| serde_json::to_value(table))
        .expect("serialize restored routing table");
    let mut persisted_table = persisted["table"].clone();
    strip_transient_routing_fields(&mut restored_table);
    strip_transient_routing_fields(&mut persisted_table);
    assert_eq!(
        restored_table, persisted_table,
        "restored DHT routing table should match the persisted snapshot"
    );

    restored.cancellation_token().cancel();
    wait_for_udp_port_release(first_addr).await;
}
