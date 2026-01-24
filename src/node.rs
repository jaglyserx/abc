use std::net::SocketAddr;

use jsonrpsee::{RpcModule, server::Server, tokio};
use serde::Deserialize;

pub async fn run_server() -> anyhow::Result<SocketAddr> {
    let server = Server::builder()
        .build("127.0.0.1".parse::<SocketAddr>()?)
        .await?;
    let mut module = RpcModule::new(());
    module.register_method("say_hello", |_, _, _| "hello")?;
    module.register_method("create_account", |params, _, _| {
        let req = params.parse::<CreateAccountReq>()?;
        account_service::create_account(req)
    })?;

    let addr = server.local_addr()?;
    let handle = server.start(module);
    tokio::spawn(handle.stopped());
    Ok(addr)
}

#[derive(Deserialize)]
struct CreateAccountReq<'a> {
    pass: &'a str,
}

mod account_service {
    use jsonrpsee::types::error::{ErrorCode, ErrorObjectOwned};

    use crate::{account::Account, node::CreateAccountReq};

    const KEYSTORE_DIR: &str = "accounts";

    pub fn create_account(req: CreateAccountReq) -> Result<String, ErrorObjectOwned> {
        create_account_in_dir(req.pass, KEYSTORE_DIR)
    }

    fn create_account_in_dir(pass: &str, dir: &str) -> Result<String, ErrorObjectOwned> {
        let account = Account::new();
        let addr = account.address();

        account
            .persist(dir, pass)
            .map_err(|err| internal_error(format!("failed to store account: {err}")))?;

        Ok(addr.to_string())
    }

    fn internal_error(message: String) -> ErrorObjectOwned {
        ErrorObjectOwned::owned(ErrorCode::InternalError.code(), message, None::<()>)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::fs;
        use tempfile::tempdir;

        #[test]
        fn writes_encrypted_key_to_requested_directory() {
            let tmp_dir = tempdir().expect("tempdir");
            let dir = tmp_dir.path().to_str().expect("utf8 path");

            let addr = create_account_in_dir("hunter2", dir).expect("create account succeeds");

            assert_eq!(addr.len(), 64);
            let keyfile = tmp_dir.path().join(&addr);
            assert!(keyfile.exists(), "key file should exist");

            let contents = fs::read(keyfile).expect("read keyfile");
            assert!(!contents.is_empty(), "key file should not be empty");
        }
    }
}
