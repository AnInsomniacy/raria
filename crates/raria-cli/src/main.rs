use raria_core::RariaRuntime;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let runtime = RariaRuntime::start().await?;
    runtime.shutdown().await?;
    Ok(())
}
