use std::{net::SocketAddr, sync::Arc};

use jsonrpsee::{
    RpcModule,
    server::{Server, ServerHandle},
    types::error::{ErrorCode, ErrorObjectOwned},
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::ledger::{Ledger, SubmitTxRequest, TxReceipt};

pub async fn run_server() -> anyhow::Result<(SocketAddr, ServerHandle)> {
    let server = Server::builder()
        .build("127.0.0.1:0".parse::<SocketAddr>()?)
        .await?;

    let ledger = Arc::new(Mutex::new(Ledger::new()));
    let mut module = RpcModule::new(Arc::clone(&ledger));

    module.register_method("say_hello", |_, _, _| "hello")?;

    module.register_async_method("create_account", |params, ledger, _| async move {
        let req = params.parse::<CreateAccountReq>()?;
        let addr = account_service::create_account(&req.pass)?;

        let mut ledger = ledger.lock().await;
        ledger.ensure_account(&addr);
        if let Some(amount) = req.initial_balance {
            ledger.credit_account(&addr, amount);
        }

        Ok::<CreateAccountResp, ErrorObjectOwned>(CreateAccountResp { address: addr })
    })?;

    module.register_async_method("faucet", |params, ledger, _| async move {
        let req = params.parse::<FaucetReq>()?;
        if req.amount == 0 {
            return Err(invalid_params("amount must be greater than zero"));
        }

        let mut ledger = ledger.lock().await;
        ledger.credit_account(&req.address, req.amount);
        let balance = ledger.get_balance(&req.address);

        Ok::<FaucetResp, ErrorObjectOwned>(FaucetResp {
            address: req.address,
            balance,
        })
    })?;

    module.register_async_method("submit_tx", |params, ledger, _| async move {
        let req = params.parse::<SubmitTxReq>()?;

        let mut ledger = ledger.lock().await;
        let submission = ledger
            .submit_tx(SubmitTxRequest {
                from: req.from,
                to: req.to,
                amount: req.amount,
                nonce: req.nonce,
            })
            .map_err(|err| invalid_params(&err.to_string()))?;

        Ok::<SubmitTxResp, ErrorObjectOwned>(SubmitTxResp {
            tx_id: submission.tx_id,
            mempool_size: submission.mempool_size,
        })
    })?;

    module.register_async_method("produce_block", |params, ledger, _| async move {
        let req = params.parse::<ProduceBlockReq>()?;

        let mut ledger = ledger.lock().await;
        let result = ledger.execute_block(req.max_txs.unwrap_or(100));

        Ok::<ProduceBlockResp, ErrorObjectOwned>(ProduceBlockResp {
            height: result.height,
            included: result.included,
            committed: result.committed,
            rejected: result.rejected,
        })
    })?;

    module.register_async_method("get_balance", |params, ledger, _| async move {
        let req = params.parse::<BalanceReq>()?;
        let ledger = ledger.lock().await;
        Ok::<BalanceResp, ErrorObjectOwned>(BalanceResp {
            address: req.address.clone(),
            balance: ledger.get_balance(&req.address),
        })
    })?;

    module.register_async_method("get_nonce", |params, ledger, _| async move {
        let req = params.parse::<NonceReq>()?;
        let ledger = ledger.lock().await;
        Ok::<NonceResp, ErrorObjectOwned>(NonceResp {
            address: req.address.clone(),
            nonce: ledger.get_nonce(&req.address),
        })
    })?;

    module.register_async_method("get_receipt", |params, ledger, _| async move {
        let req = params.parse::<ReceiptReq>()?;
        let ledger = ledger.lock().await;
        Ok::<Option<TxReceipt>, ErrorObjectOwned>(ledger.get_receipt(&req.tx_id))
    })?;

    module.register_async_method("mempool_size", |_, ledger, _| async move {
        let ledger = ledger.lock().await;
        Ok::<MempoolResp, ErrorObjectOwned>(MempoolResp {
            mempool_size: ledger.mempool_size(),
        })
    })?;

    let addr = server.local_addr()?;
    let handle = server.start(module);
    Ok((addr, handle))
}

#[derive(Deserialize)]
struct CreateAccountReq {
    pass: String,
    initial_balance: Option<u64>,
}

#[derive(Clone, Serialize)]
struct CreateAccountResp {
    address: String,
}

#[derive(Deserialize)]
struct FaucetReq {
    address: String,
    amount: u64,
}

#[derive(Clone, Serialize)]
struct FaucetResp {
    address: String,
    balance: u128,
}

#[derive(Deserialize)]
struct SubmitTxReq {
    from: String,
    to: String,
    amount: u64,
    nonce: u64,
}

#[derive(Clone, Serialize)]
struct SubmitTxResp {
    tx_id: String,
    mempool_size: usize,
}

#[derive(Deserialize)]
struct ProduceBlockReq {
    max_txs: Option<usize>,
}

#[derive(Clone, Serialize)]
struct ProduceBlockResp {
    height: u64,
    included: usize,
    committed: usize,
    rejected: usize,
}

#[derive(Deserialize)]
struct BalanceReq {
    address: String,
}

#[derive(Clone, Serialize)]
struct BalanceResp {
    address: String,
    balance: u128,
}

#[derive(Deserialize)]
struct NonceReq {
    address: String,
}

#[derive(Clone, Serialize)]
struct NonceResp {
    address: String,
    nonce: u64,
}

#[derive(Deserialize)]
struct ReceiptReq {
    tx_id: String,
}

#[derive(Clone, Serialize)]
struct MempoolResp {
    mempool_size: usize,
}

fn invalid_params(message: &str) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(
        ErrorCode::InvalidParams.code(),
        message.to_string(),
        None::<()>,
    )
}

mod account_service {
    use super::{ErrorCode, ErrorObjectOwned};
    use crate::account::Account;

    const KEYSTORE_DIR: &str = "accounts";

    pub fn create_account(pass: &str) -> Result<String, ErrorObjectOwned> {
        create_account_in_dir(pass, KEYSTORE_DIR)
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
        use super::create_account_in_dir;
        use crate::node::run_server;
        use jsonrpsee::tokio;
        use std::fs;
        use std::time::Duration;
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

        #[tokio::test]
        async fn server_starts_with_ephemeral_port() {
            let (addr, handle) = match run_server().await {
                Ok(server) => server,
                Err(err)
                    if err
                        .chain()
                        .any(|cause| cause.to_string().contains("Operation not permitted")) =>
                {
                    return;
                }
                Err(err) => panic!("server starts: {err}"),
            };

            assert!(addr.port() > 0, "server should bind an ephemeral port");

            handle.stop().expect("stop server");
            tokio::time::timeout(Duration::from_secs(2), handle.stopped())
                .await
                .expect("server should stop");
        }
    }
}
