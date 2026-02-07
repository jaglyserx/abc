use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{block::Block, consensus::ConsensusMsg};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StoredEvent {
    Genesis(Block),
    Inbound(ConsensusMsg),
    Outbound(ConsensusMsg),
}

#[derive(Debug)]
pub struct ChainStore {
    path: PathBuf,
    file: Mutex<File>,
}

impl ChainStore {
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create store dir {}", parent.display()))?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to open store {}", path.display()))?;

        Ok(Self {
            path,
            file: Mutex::new(file),
        })
    }

    pub fn append(&self, event: &StoredEvent) -> anyhow::Result<()> {
        let mut file = self
            .file
            .lock()
            .map_err(|_| anyhow::anyhow!("store lock poisoned"))?;

        serde_json::to_writer(&mut *file, event)?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    }

    #[cfg(test)]
    fn read_all(path: impl AsRef<Path>) -> anyhow::Result<Vec<StoredEvent>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let mut out = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.is_empty() {
                continue;
            }
            out.push(serde_json::from_str::<StoredEvent>(&line)?);
        }
        Ok(out)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::block::{Block, BlockHeader, BlockPayload};

    use super::*;

    #[test]
    fn appends_events_as_jsonl() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("events.jsonl");
        let store = ChainStore::open(&path).expect("open");

        let block = Block {
            header: BlockHeader {
                round: 0,
                proposer: 1,
                parent_hash: [0; 32],
                payload_hash: [0; 32],
                rank: 0,
            },
            payload: BlockPayload {
                bytes: b"genesis".to_vec(),
            },
            signature: vec![],
        };

        store
            .append(&StoredEvent::Genesis(block.clone()))
            .expect("append genesis");
        store
            .append(&StoredEvent::Inbound(ConsensusMsg::FinalizationVote(
                crate::block::FinalizationVote {
                    round: 1,
                    block_hash: [7; 32],
                    voter: 1,
                    signature: vec![],
                },
            )))
            .expect("append inbound");

        let events = ChainStore::read_all(&path).expect("read");
        assert_eq!(events.len(), 2);
        match &events[0] {
            StoredEvent::Genesis(g) => assert_eq!(g, &block),
            other => panic!("unexpected first event: {other:?}"),
        }
    }
}
