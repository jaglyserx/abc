use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};

pub type AccountId = String;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub from: AccountId,
    pub to: AccountId,
    pub amount: u64,
    pub nonce: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SubmitTxRequest {
    pub from: AccountId,
    pub to: AccountId,
    pub amount: u64,
    pub nonce: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ReceiptStatus {
    Committed,
    Rejected,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxReceipt {
    pub tx_id: String,
    pub block_height: u64,
    pub status: ReceiptStatus,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountState {
    pub balance: u128,
    pub nonce: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockExecutionResult {
    pub height: u64,
    pub included: usize,
    pub committed: usize,
    pub rejected: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxSubmission {
    pub tx_id: String,
    pub mempool_size: usize,
}

#[derive(Debug, Default)]
pub struct Ledger {
    next_height: u64,
    state: HashMap<AccountId, AccountState>,
    mempool: VecDeque<Transaction>,
    receipts: HashMap<String, TxReceipt>,
    seen_tx_ids: HashMap<String, bool>,
}

impl Ledger {
    pub fn new() -> Self {
        Self {
            next_height: 1,
            ..Self::default()
        }
    }

    pub fn ensure_account(&mut self, account: &str) {
        self.state
            .entry(account.to_string())
            .or_insert(AccountState {
                balance: 0,
                nonce: 0,
            });
    }

    pub fn credit_account(&mut self, account: &str, amount: u64) {
        self.ensure_account(account);
        if let Some(state) = self.state.get_mut(account) {
            state.balance = state.balance.saturating_add(amount as u128);
        }
    }

    pub fn submit_tx(&mut self, req: SubmitTxRequest) -> anyhow::Result<TxSubmission> {
        if req.from == req.to {
            anyhow::bail!("sender and recipient must be different");
        }
        if req.amount == 0 {
            anyhow::bail!("amount must be greater than zero");
        }

        self.ensure_account(&req.from);
        self.ensure_account(&req.to);

        let tx_id = tx_id(&req);
        if self.seen_tx_ids.contains_key(&tx_id) {
            anyhow::bail!("duplicate tx id");
        }

        let tx = Transaction {
            id: tx_id.clone(),
            from: req.from,
            to: req.to,
            amount: req.amount,
            nonce: req.nonce,
        };

        self.mempool.push_back(tx);
        self.seen_tx_ids.insert(tx_id.clone(), true);

        Ok(TxSubmission {
            tx_id,
            mempool_size: self.mempool.len(),
        })
    }

    pub fn execute_block(&mut self, max_txs: usize) -> BlockExecutionResult {
        let max_txs = max_txs.max(1);
        let height = self.next_height;
        self.next_height = self.next_height.saturating_add(1);

        let mut included = 0usize;
        let mut committed = 0usize;
        let mut rejected = 0usize;

        while included < max_txs {
            let Some(tx) = self.mempool.pop_front() else {
                break;
            };
            included += 1;

            let result = self.apply_tx(&tx);
            if result.error.is_none() {
                committed += 1;
            } else {
                rejected += 1;
            }

            self.receipts.insert(
                tx.id.clone(),
                TxReceipt {
                    tx_id: tx.id,
                    block_height: height,
                    status: if result.error.is_none() {
                        ReceiptStatus::Committed
                    } else {
                        ReceiptStatus::Rejected
                    },
                    error: result.error,
                },
            );
        }

        BlockExecutionResult {
            height,
            included,
            committed,
            rejected,
        }
    }

    pub fn get_balance(&self, account: &str) -> u128 {
        self.state.get(account).map(|s| s.balance).unwrap_or(0)
    }

    pub fn get_nonce(&self, account: &str) -> u64 {
        self.state.get(account).map(|s| s.nonce).unwrap_or(0)
    }

    pub fn get_receipt(&self, tx_id: &str) -> Option<TxReceipt> {
        self.receipts.get(tx_id).cloned()
    }

    pub fn mempool_size(&self) -> usize {
        self.mempool.len()
    }

    fn apply_tx(&mut self, tx: &Transaction) -> ApplyResult {
        let Some(sender_view) = self.state.get(&tx.from).cloned() else {
            return ApplyResult {
                error: Some("sender account does not exist".to_string()),
            };
        };

        if tx.nonce != sender_view.nonce.saturating_add(1) {
            return ApplyResult {
                error: Some(format!(
                    "invalid nonce: expected {}, got {}",
                    sender_view.nonce.saturating_add(1),
                    tx.nonce
                )),
            };
        }

        if sender_view.balance < tx.amount as u128 {
            return ApplyResult {
                error: Some("insufficient balance".to_string()),
            };
        }

        let recipient_balance = self
            .state
            .get(&tx.to)
            .map(|s| s.balance)
            .unwrap_or_default();

        if let Some(sender) = self.state.get_mut(&tx.from) {
            sender.balance = sender.balance.saturating_sub(tx.amount as u128);
            sender.nonce = tx.nonce;
        }

        self.state.insert(
            tx.to.clone(),
            AccountState {
                balance: recipient_balance.saturating_add(tx.amount as u128),
                nonce: self.get_nonce(&tx.to),
            },
        );

        ApplyResult { error: None }
    }
}

struct ApplyResult {
    error: Option<String>,
}

fn tx_id(req: &SubmitTxRequest) -> String {
    let mut hasher = Sha3_256::new();
    hasher.update(req.from.as_bytes());
    hasher.update(req.to.as_bytes());
    hasher.update(req.amount.to_le_bytes());
    hasher.update(req.nonce.to_le_bytes());
    let out = hasher.finalize();
    let mut id = String::with_capacity(out.len() * 2);
    for byte in out {
        use std::fmt::Write as _;
        let _ = write!(&mut id, "{:02x}", byte);
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executes_valid_transfer_and_updates_balances() {
        let mut ledger = Ledger::new();
        ledger.credit_account("alice", 100);
        ledger.ensure_account("bob");

        let submit = ledger
            .submit_tx(SubmitTxRequest {
                from: "alice".to_string(),
                to: "bob".to_string(),
                amount: 25,
                nonce: 1,
            })
            .expect("submit");

        let block = ledger.execute_block(100);
        assert_eq!(block.committed, 1);
        assert_eq!(ledger.get_balance("alice"), 75);
        assert_eq!(ledger.get_balance("bob"), 25);
        assert_eq!(ledger.get_nonce("alice"), 1);
        assert_eq!(ledger.mempool_size(), 0);

        let receipt = ledger.get_receipt(&submit.tx_id).expect("receipt");
        matches!(receipt.status, ReceiptStatus::Committed);
        assert_eq!(receipt.error, None);
    }

    #[test]
    fn rejects_nonce_replay() {
        let mut ledger = Ledger::new();
        ledger.credit_account("alice", 100);
        ledger.ensure_account("bob");

        ledger
            .submit_tx(SubmitTxRequest {
                from: "alice".to_string(),
                to: "bob".to_string(),
                amount: 10,
                nonce: 1,
            })
            .expect("submit 1");
        ledger.execute_block(10);

        let submit2 = ledger
            .submit_tx(SubmitTxRequest {
                from: "alice".to_string(),
                to: "bob".to_string(),
                amount: 11,
                nonce: 1,
            })
            .expect("submit 2");

        let block = ledger.execute_block(10);
        assert_eq!(block.rejected, 1);

        let receipt = ledger.get_receipt(&submit2.tx_id).expect("receipt");
        matches!(receipt.status, ReceiptStatus::Rejected);
        assert!(receipt.error.expect("error").contains("invalid nonce"));
    }

    #[test]
    fn rejects_insufficient_balance() {
        let mut ledger = Ledger::new();
        ledger.credit_account("alice", 5);
        ledger.ensure_account("bob");

        let submit = ledger
            .submit_tx(SubmitTxRequest {
                from: "alice".to_string(),
                to: "bob".to_string(),
                amount: 9,
                nonce: 1,
            })
            .expect("submit");

        let block = ledger.execute_block(10);
        assert_eq!(block.rejected, 1);

        let receipt = ledger.get_receipt(&submit.tx_id).expect("receipt");
        matches!(receipt.status, ReceiptStatus::Rejected);
        assert_eq!(ledger.get_balance("alice"), 5);
        assert_eq!(ledger.get_balance("bob"), 0);
    }
}
