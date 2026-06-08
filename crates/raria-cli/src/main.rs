use std::{path::PathBuf, sync::Arc};

use raria_core::{
    DownloadEngine, RariaConfig, RpcCall, RpcEngine, RpcServer, RpcValue, build_rpc_router,
    parse_cli, parse_input_file_text, save_session_text,
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

    let mut input_tasks = Vec::new();
    if let Some(path) = &command.input_file {
        let text = tokio::fs::read_to_string(path).await?;
        input_tasks.extend(parse_input_file_text(&text)?);
    }
    if !command.uris.is_empty() {
        input_tasks.push(raria_core::InputTask {
            uris: command.uris,
            options: Vec::new(),
        });
    }
    if let Some(path) = &command.save_session {
        tokio::fs::write(path, save_session_text(&input_tasks)).await?;
    }

    if input_tasks.is_empty() {
        return Ok(());
    }

    let mut engine = RpcEngine::default();
    for task in input_tasks {
        let mut options = Vec::new();
        if let Some(split) = command.split {
            options.push(("split".to_string(), RpcValue::string(split.to_string())));
        }
        for (key, value) in task.options {
            options.push((key, RpcValue::string(value)));
        }
        engine
            .call(RpcCall::new(
                "aria2.addUri",
                RpcValue::array([
                    RpcValue::Array(task.uris.into_iter().map(RpcValue::string).collect()),
                    RpcValue::Object(options.into_iter().collect()),
                ]),
            ))
            .map_err(|error| anyhow::anyhow!(error.message))?;
    }
    DownloadEngine::new(config).run_once(&mut engine).await?;
    Ok(())
}
