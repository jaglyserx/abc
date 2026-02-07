use crate::{
    node::run_server,
    node_runtime::{NodeRuntime, NodeRuntimeConfig},
};

mod account;
mod block;
mod consensus;
mod constants;
mod node;
mod node_runtime;
mod p2p;
mod store;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (_rpc_addr, rpc_handle) = run_server().await?;

    let runtime_config = NodeRuntimeConfig::from_env()?;
    let runtime = NodeRuntime::start(runtime_config).await?;

    tokio::signal::ctrl_c().await?;

    rpc_handle.stop()?;
    runtime.shutdown();

    Ok(())
}
