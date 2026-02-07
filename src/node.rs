use std::{net::SocketAddr, sync::Arc};

use jsonrpsee::{
    RpcModule,
    server::{Server, ServerHandle},
    types::error::{ErrorCode, ErrorObjectOwned},
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::{
    ledger::{ExecutedBlock, Ledger, LedgerSnapshot, SubmitTxRequest, TxReceipt},
    state_db::StateDb,
};

pub async fn run_server() -> anyhow::Result<(SocketAddr, ServerHandle)> {
    let server = Server::builder()
        .build("127.0.0.1:0".parse::<SocketAddr>()?)
        .await?;

    let state_db = Arc::new(StateDb::open(state_db_root())?);
    let config = Arc::new(AppConfig::from_env());
    let ledger = match state_db.load_snapshot()? {
        Some(snapshot) => Ledger::from_snapshot(snapshot),
        None => Ledger::new(),
    };

    let app_state = Arc::new(Mutex::new(AppState {
        ledger,
        state_db: Arc::clone(&state_db),
        config,
        metrics: RpcMetrics::new(),
    }));

    let mut module = RpcModule::new(Arc::clone(&app_state));

    module.register_method("say_hello", |_, _, _| "hello")?;

    module.register_async_method("create_account", |params, state, _| async move {
        let req = params.parse::<CreateAccountReq>()?;
        if req.pass.len() < state.lock().await.config.min_account_pass_len {
            return Err(invalid_params("passphrase too short"));
        }
        let addr = account_service::create_account(&req.pass)?;

        let mut state = state.lock().await;
        state.metrics.rpc_calls = state.metrics.rpc_calls.saturating_add(1);
        state.ledger.ensure_account(&addr);
        if let Some(amount) = req.initial_balance {
            state.ledger.credit_account(&addr, amount);
        }
        state.persist_snapshot().map_err(internal_error)?;

        Ok::<CreateAccountResp, ErrorObjectOwned>(CreateAccountResp { address: addr })
    })?;

    module.register_async_method("faucet", |params, state, _| async move {
        let req = params.parse::<FaucetReq>()?;
        if req.amount == 0 {
            return Err(invalid_params("amount must be greater than zero"));
        }

        let mut state = state.lock().await;
        state.metrics.rpc_calls = state.metrics.rpc_calls.saturating_add(1);
        if !state.config.enable_faucet {
            return Err(invalid_params("faucet is disabled"));
        }
        if req.amount > state.config.max_faucet_amount {
            return Err(invalid_params("requested amount exceeds faucet limit"));
        }
        state.ledger.credit_account(&req.address, req.amount);
        let balance = state.ledger.get_balance(&req.address);
        state.persist_snapshot().map_err(internal_error)?;

        Ok::<FaucetResp, ErrorObjectOwned>(FaucetResp {
            address: req.address,
            balance,
        })
    })?;

    module.register_async_method("submit_tx", |params, state, _| async move {
        let req = params.parse::<SubmitTxReq>()?;

        let mut state = state.lock().await;
        state.metrics.rpc_calls = state.metrics.rpc_calls.saturating_add(1);
        let submission = state
            .ledger
            .submit_tx(SubmitTxRequest {
                from: req.from,
                to: req.to,
                amount: req.amount,
                nonce: req.nonce,
            })
            .map_err(|err| invalid_params(&err.to_string()))?;
        state.metrics.txs_submitted = state.metrics.txs_submitted.saturating_add(1);
        state.persist_snapshot().map_err(internal_error)?;

        Ok::<SubmitTxResp, ErrorObjectOwned>(SubmitTxResp {
            tx_id: submission.tx_id,
            mempool_size: submission.mempool_size,
        })
    })?;

    module.register_async_method("produce_block", |params, state, _| async move {
        let req = params.parse::<ProduceBlockReq>()?;

        let mut state = state.lock().await;
        state.metrics.rpc_calls = state.metrics.rpc_calls.saturating_add(1);
        let result = state.ledger.execute_block(req.max_txs.unwrap_or(100));
        let executed = state.ledger.blocks().last().cloned().ok_or_else(|| {
            internal_error(anyhow::anyhow!("missing executed block after execution"))
        })?;

        state
            .state_db
            .append_block(&executed)
            .map_err(internal_error)?;
        state.metrics.blocks_produced = state.metrics.blocks_produced.saturating_add(1);
        state.persist_snapshot().map_err(internal_error)?;

        Ok::<ProduceBlockResp, ErrorObjectOwned>(ProduceBlockResp {
            height: result.height,
            included: result.included,
            committed: result.committed,
            rejected: result.rejected,
            tx_ids: result.tx_ids,
        })
    })?;

    module.register_async_method("get_balance", |params, state, _| async move {
        let req = params.parse::<BalanceReq>()?;
        let mut state = state.lock().await;
        state.metrics.rpc_calls = state.metrics.rpc_calls.saturating_add(1);
        Ok::<BalanceResp, ErrorObjectOwned>(BalanceResp {
            address: req.address.clone(),
            balance: state.ledger.get_balance(&req.address),
        })
    })?;

    module.register_async_method("get_nonce", |params, state, _| async move {
        let req = params.parse::<NonceReq>()?;
        let mut state = state.lock().await;
        state.metrics.rpc_calls = state.metrics.rpc_calls.saturating_add(1);
        Ok::<NonceResp, ErrorObjectOwned>(NonceResp {
            address: req.address.clone(),
            nonce: state.ledger.get_nonce(&req.address),
        })
    })?;

    module.register_async_method("get_receipt", |params, state, _| async move {
        let req = params.parse::<ReceiptReq>()?;
        let mut state = state.lock().await;
        state.metrics.rpc_calls = state.metrics.rpc_calls.saturating_add(1);
        Ok::<Option<TxReceipt>, ErrorObjectOwned>(state.ledger.get_receipt(&req.tx_id))
    })?;

    module.register_async_method("mempool_size", |_, state, _| async move {
        let mut state = state.lock().await;
        state.metrics.rpc_calls = state.metrics.rpc_calls.saturating_add(1);
        Ok::<MempoolResp, ErrorObjectOwned>(MempoolResp {
            mempool_size: state.ledger.mempool_size(),
        })
    })?;

    module.register_async_method("export_snapshot", |_, state, _| async move {
        let mut state = state.lock().await;
        state.metrics.rpc_calls = state.metrics.rpc_calls.saturating_add(1);
        state.metrics.snapshots_exported = state.metrics.snapshots_exported.saturating_add(1);
        Ok::<LedgerSnapshot, ErrorObjectOwned>(state.ledger.snapshot())
    })?;

    module.register_async_method("export_blocks", |_, state, _| async move {
        let mut state = state.lock().await;
        state.metrics.rpc_calls = state.metrics.rpc_calls.saturating_add(1);
        Ok::<Vec<ExecutedBlock>, ErrorObjectOwned>(state.ledger.blocks().to_vec())
    })?;

    module.register_async_method("import_snapshot", |params, state, _| async move {
        let req = params.parse::<ImportSnapshotReq>()?;
        let mut state = state.lock().await;
        state.metrics.rpc_calls = state.metrics.rpc_calls.saturating_add(1);

        let current_height = state.ledger.snapshot().next_height;
        if req.snapshot.next_height < current_height {
            return Err(invalid_params(
                "incoming snapshot height is older than local state",
            ));
        }

        state.ledger = Ledger::from_snapshot(req.snapshot.clone());
        state
            .state_db
            .save_snapshot(&req.snapshot)
            .map_err(internal_error)?;

        if let Some(blocks) = req.blocks {
            state
                .state_db
                .reset_blocks(&blocks)
                .map_err(internal_error)?;
        } else {
            let blocks = state.ledger.blocks().to_vec();
            state
                .state_db
                .reset_blocks(&blocks)
                .map_err(internal_error)?;
        }
        state.metrics.snapshots_imported = state.metrics.snapshots_imported.saturating_add(1);

        Ok::<ImportSnapshotResp, ErrorObjectOwned>(ImportSnapshotResp {
            imported_height: state.ledger.snapshot().next_height,
        })
    })?;

    module.register_async_method("health", |_, state, _| async move {
        let mut state = state.lock().await;
        state.metrics.rpc_calls = state.metrics.rpc_calls.saturating_add(1);
        let uptime_seconds = std::time::SystemTime::now()
            .duration_since(state.metrics.started_at)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Ok::<HealthResp, ErrorObjectOwned>(HealthResp {
            status: "ok".to_string(),
            uptime_seconds,
            current_height: state.ledger.current_height(),
            mempool_size: state.ledger.mempool_size(),
        })
    })?;

    module.register_async_method("metrics", |_, state, _| async move {
        let mut state = state.lock().await;
        state.metrics.rpc_calls = state.metrics.rpc_calls.saturating_add(1);
        Ok::<MetricsResp, ErrorObjectOwned>(MetricsResp {
            rpc_calls: state.metrics.rpc_calls,
            txs_submitted: state.metrics.txs_submitted,
            blocks_produced: state.metrics.blocks_produced,
            snapshots_imported: state.metrics.snapshots_imported,
            snapshots_exported: state.metrics.snapshots_exported,
            current_height: state.ledger.current_height(),
            mempool_size: state.ledger.mempool_size(),
        })
    })?;

    let addr = server.local_addr()?;
    let handle = server.start(module);
    Ok((addr, handle))
}

struct AppState {
    ledger: Ledger,
    state_db: Arc<StateDb>,
    config: Arc<AppConfig>,
    metrics: RpcMetrics,
}

impl AppState {
    fn persist_snapshot(&self) -> anyhow::Result<()> {
        self.state_db.save_snapshot(&self.ledger.snapshot())
    }
}

fn state_db_root() -> String {
    let data_dir = std::env::var("ABC_DATA_DIR").unwrap_or_else(|_| "data".to_string());
    let node_id = std::env::var("ABC_NODE_ID").unwrap_or_else(|_| "1".to_string());
    format!("{}/node-{}/state", data_dir, node_id)
}

struct AppConfig {
    min_account_pass_len: usize,
    enable_faucet: bool,
    max_faucet_amount: u64,
}

impl AppConfig {
    fn from_env() -> Self {
        let min_account_pass_len = std::env::var("ABC_MIN_ACCOUNT_PASS_LEN")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(10);
        let enable_faucet = std::env::var("ABC_ENABLE_FAUCET")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(true);
        let max_faucet_amount = std::env::var("ABC_MAX_FAUCET_AMOUNT")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1_000_000);

        Self {
            min_account_pass_len,
            enable_faucet,
            max_faucet_amount,
        }
    }
}

struct RpcMetrics {
    started_at: std::time::SystemTime,
    rpc_calls: u64,
    txs_submitted: u64,
    blocks_produced: u64,
    snapshots_imported: u64,
    snapshots_exported: u64,
}

impl RpcMetrics {
    fn new() -> Self {
        Self {
            started_at: std::time::SystemTime::now(),
            rpc_calls: 0,
            txs_submitted: 0,
            blocks_produced: 0,
            snapshots_imported: 0,
            snapshots_exported: 0,
        }
    }
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
    tx_ids: Vec<String>,
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

#[derive(Deserialize)]
struct ImportSnapshotReq {
    snapshot: LedgerSnapshot,
    blocks: Option<Vec<ExecutedBlock>>,
}

#[derive(Clone, Serialize)]
struct ImportSnapshotResp {
    imported_height: u64,
}

#[derive(Clone, Serialize)]
struct HealthResp {
    status: String,
    uptime_seconds: u64,
    current_height: u64,
    mempool_size: usize,
}

#[derive(Clone, Serialize)]
struct MetricsResp {
    rpc_calls: u64,
    txs_submitted: u64,
    blocks_produced: u64,
    snapshots_imported: u64,
    snapshots_exported: u64,
    current_height: u64,
    mempool_size: usize,
}

fn invalid_params(message: &str) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(
        ErrorCode::InvalidParams.code(),
        message.to_string(),
        None::<()>,
    )
}

fn internal_error(err: anyhow::Error) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(ErrorCode::InternalError.code(), err.to_string(), None::<()>)
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
