use jsonrpsee::tokio;

use crate::node::run_server;

mod account;
mod block;
mod consensus;
mod constants;
mod node;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (_addr, handle) = run_server().await?;
    handle.stopped().await;
    Ok(())
}
