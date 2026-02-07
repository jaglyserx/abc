use crate::{
    node::run_server,
    node_runtime::{NodeRuntime, NodeRuntimeConfig},
};

mod account;
mod block;
mod consensus;
mod constants;
mod ledger;
mod node;
mod node_runtime;
mod p2p;
mod state_db;
mod store;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (_rpc_addr, rpc_handle) = run_server().await?;
    eprintln!("[info] rpc server started");

    let runtime_config = NodeRuntimeConfig::from_env()?;
    let runtime = NodeRuntime::start(runtime_config).await?;
    eprintln!("[info] node runtime started");

    if let Err(err) = tokio::signal::ctrl_c().await {
        eprintln!("[error] failed waiting for shutdown signal: {err}");
        return Err(err.into());
    }

    rpc_handle.stop()?;
    runtime.shutdown();
    eprintln!("[info] shutdown complete");

    Ok(())
}
