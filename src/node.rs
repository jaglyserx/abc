use std::net::SocketAddr;

use jsonrpsee::{RpcModule, server::Server, tokio};

async fn run_server() -> anyhow::Result<SocketAddr> {
    let server = Server::builder()
        .build("127.0.0.1".parse::<SocketAddr>()?)
        .await?;
    let mut module = RpcModule::new(());
    module.register_method("say_hello", |_, _, _| "hello")?;
    module.register_method("create_account",|_,_,_| account_service::create_account() );

    let addr = server.local_addr()?;
    let handle = server.start(module);
    tokio::spawn(handle.stopped());
    Ok(addr)
}

mod account_service{
    pub fn create_account() {}
}
