use jsonrpsee::tokio;

use crate::node::run_server;

mod account;
mod block;
mod constants;
mod node;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    run_server().await?;
    Ok(())
}
