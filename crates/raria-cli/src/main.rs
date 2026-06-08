use std::{path::PathBuf, sync::Arc};

use raria_core::{
    DownloadEngine, RariaConfig, RpcCall, RpcEngine, RpcServer, RpcValue, build_rpc_router,
    parse_cli,
};
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let command = parse_cli(std::env::args())?;
    let mut config = RariaConfig::default();
    if let Some(dir) = command.dir {
        config.download_dir = PathBuf::from(dir);
    }
    config.rpc_listen_port = command.rpc_listen_port;

    if command.enable_rpc {
        let engine = Arc::new(Mutex::new(RpcEngine::default()));
        let app = build_rpc_router(engine.clone());
        let (_server, addr) = RpcServer::start(config, engine, app).await?;
        println!("raria RPC listening on {addr}");
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    if command.uris.is_empty() {
        return Ok(());
    }

    let mut engine = RpcEngine::default();
    for uri in command.uris {
        let mut options = Vec::new();
        if let Some(split) = command.split {
            options.push(("split", RpcValue::string(split.to_string())));
        }
        engine
            .call(RpcCall::new(
                "aria2.addUri",
                RpcValue::array([
                    RpcValue::array([RpcValue::string(uri)]),
                    RpcValue::Object(
                        options
                            .into_iter()
                            .map(|(key, value)| (key.into(), value))
                            .collect(),
                    ),
                ]),
            ))
            .map_err(|error| anyhow::anyhow!(error.message))?;
    }
    DownloadEngine::new(config).run_once(&mut engine).await?;
    Ok(())
}
