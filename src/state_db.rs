use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::Path,
};

use anyhow::Context;

use crate::ledger::{ExecutedBlock, LedgerSnapshot};

#[derive(Debug)]
pub struct StateDb {
    snapshot_path: std::path::PathBuf,
    blocks_path: std::path::PathBuf,
}

impl StateDb {
    pub fn open(root: impl AsRef<Path>) -> anyhow::Result<Self> {
        let root = root.as_ref();
        fs::create_dir_all(root)
            .with_context(|| format!("failed to create state db root {}", root.display()))?;

        let snapshot_path = root.join("state_snapshot.json");
        let blocks_path = root.join("blocks.jsonl");

        Ok(Self {
            snapshot_path,
            blocks_path,
        })
    }

    pub fn load_snapshot(&self) -> anyhow::Result<Option<LedgerSnapshot>> {
        if !self.snapshot_path.exists() {
            return Ok(None);
        }

        let file = File::open(&self.snapshot_path).with_context(|| {
            format!(
                "failed to open snapshot file {}",
                self.snapshot_path.display()
            )
        })?;

        Ok(Some(serde_json::from_reader(file).with_context(|| {
            format!(
                "failed to deserialize snapshot file {}",
                self.snapshot_path.display()
            )
        })?))
    }

    pub fn save_snapshot(&self, snapshot: &LedgerSnapshot) -> anyhow::Result<()> {
        let tmp_path = self.snapshot_path.with_extension("json.tmp");
        {
            let mut tmp = File::create(&tmp_path).with_context(|| {
                format!("failed to create temp snapshot {}", tmp_path.display())
            })?;
            serde_json::to_writer_pretty(&mut tmp, snapshot)?;
            tmp.flush()?;
        }
        fs::rename(&tmp_path, &self.snapshot_path).with_context(|| {
            format!("failed to commit snapshot {}", self.snapshot_path.display())
        })?;
        Ok(())
    }

    pub fn append_block(&self, block: &ExecutedBlock) -> anyhow::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.blocks_path)
            .with_context(|| {
                format!("failed to open blocks file {}", self.blocks_path.display())
            })?;

        serde_json::to_writer(&mut file, block)?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn load_blocks(&self) -> anyhow::Result<Vec<ExecutedBlock>> {
        if !self.blocks_path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.blocks_path).with_context(|| {
            format!("failed to open blocks file {}", self.blocks_path.display())
        })?;
        let reader = BufReader::new(file);

        let mut blocks = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            blocks.push(serde_json::from_str::<ExecutedBlock>(&line)?);
        }
        Ok(blocks)
    }

    pub fn reset_blocks(&self, blocks: &[ExecutedBlock]) -> anyhow::Result<()> {
        let mut file = File::create(&self.blocks_path).with_context(|| {
            format!("failed to reset blocks file {}", self.blocks_path.display())
        })?;
        for block in blocks {
            serde_json::to_writer(&mut file, block)?;
            file.write_all(b"\n")?;
        }
        file.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::ledger::{AccountState, ReceiptStatus, TxReceipt};

    use super::*;

    #[test]
    fn snapshot_and_blocks_roundtrip() {
        let tmp = tempdir().expect("tempdir");
        let db = StateDb::open(tmp.path()).expect("open db");

        let snapshot = LedgerSnapshot {
            next_height: 3,
            state: [(
                "alice".to_string(),
                AccountState {
                    balance: 42,
                    nonce: 2,
                },
            )]
            .into_iter()
            .collect(),
            mempool: vec![],
            receipts: [(
                "tx1".to_string(),
                TxReceipt {
                    tx_id: "tx1".to_string(),
                    block_height: 2,
                    status: ReceiptStatus::Committed,
                    error: None,
                },
            )]
            .into_iter()
            .collect(),
            seen_tx_ids: vec!["tx1".to_string()],
            blocks: vec![ExecutedBlock {
                height: 1,
                tx_ids: vec!["tx1".to_string()],
                committed: 1,
                rejected: 0,
            }],
        };

        db.save_snapshot(&snapshot).expect("save snapshot");
        let loaded = db
            .load_snapshot()
            .expect("load snapshot")
            .expect("snapshot exists");
        assert_eq!(loaded.next_height, 3);
        assert_eq!(loaded.state.get("alice").expect("state").balance, 42);

        db.append_block(&snapshot.blocks[0]).expect("append block");
        let blocks = db.load_blocks().expect("load blocks");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].height, 1);

        db.reset_blocks(&snapshot.blocks).expect("reset blocks");
        let blocks2 = db.load_blocks().expect("load blocks 2");
        assert_eq!(blocks2.len(), 1);
    }
}
