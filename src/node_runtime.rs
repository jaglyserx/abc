use std::{collections::HashSet, net::SocketAddr, path::PathBuf, sync::Arc};

use tokio::{
    sync::{Mutex, mpsc},
    task::JoinHandle,
};

use crate::{
    block::{Block, BlockHeader, BlockPayload, NodeId},
    consensus::ConsensusState,
    p2p::{GossipNode, WireMessage},
    store::{ChainStore, StoredEvent},
};

const DEFAULT_N: usize = 4;
const DEFAULT_F: usize = 1;
const DEFAULT_P: usize = 1;
const DEFAULT_P2P_BIND: &str = "127.0.0.1:7000";
const DEFAULT_DATA_DIR: &str = "data";

#[derive(Clone, Debug)]
pub struct NodeRuntimeConfig {
    pub node_id: NodeId,
    pub n: usize,
    pub f: usize,
    pub p: usize,
    pub p2p_bind: SocketAddr,
    pub peers: Vec<SocketAddr>,
    pub data_dir: PathBuf,
}

impl NodeRuntimeConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let node_id = std::env::var("ABC_NODE_ID")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1);

        let n = std::env::var("ABC_N")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(DEFAULT_N);
        let f = std::env::var("ABC_F")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(DEFAULT_F);
        let p = std::env::var("ABC_P")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(DEFAULT_P);

        let p2p_bind = std::env::var("ABC_P2P_BIND")
            .unwrap_or_else(|_| DEFAULT_P2P_BIND.to_string())
            .parse::<SocketAddr>()?;

        let peers = parse_peers(&std::env::var("ABC_P2P_PEERS").unwrap_or_default())?;

        let data_dir = PathBuf::from(
            std::env::var("ABC_DATA_DIR").unwrap_or_else(|_| DEFAULT_DATA_DIR.to_string()),
        );

        Ok(Self {
            node_id,
            n,
            f,
            p,
            p2p_bind,
            peers,
            data_dir,
        })
    }
}

fn parse_peers(value: &str) -> anyhow::Result<Vec<SocketAddr>> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<SocketAddr>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[derive(Debug)]
struct Engine {
    node_id: NodeId,
    next_id: u64,
    seen: HashSet<u64>,
    state: ConsensusState,
    store: Arc<ChainStore>,
}

impl Engine {
    fn new(
        node_id: NodeId,
        n: usize,
        f: usize,
        p: usize,
        store: Arc<ChainStore>,
    ) -> anyhow::Result<Self> {
        let genesis = genesis_block(node_id);
        store.append(&StoredEvent::Genesis(genesis.clone()))?;

        Ok(Self {
            node_id,
            next_id: 1,
            seen: HashSet::new(),
            state: ConsensusState::new(node_id, n, f, p, genesis),
            store,
        })
    }

    fn process_inbound(&mut self, wire: WireMessage) -> anyhow::Result<Vec<WireMessage>> {
        if !self.seen.insert(wire.id) {
            return Ok(Vec::new());
        }

        self.store.append(&StoredEvent::Inbound(wire.msg.clone()))?;
        let outbound = self.state.handle_msg(wire.msg);

        let mut out = Vec::with_capacity(outbound.len());
        for msg in outbound {
            self.store.append(&StoredEvent::Outbound(msg.clone()))?;
            out.push(WireMessage {
                id: self.next_wire_id(),
                from: self.node_id,
                msg,
            });
        }

        Ok(out)
    }

    fn next_wire_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

pub struct NodeRuntime {
    gossip: Arc<GossipNode>,
    inbound_task: JoinHandle<()>,
    #[allow(dead_code)]
    store_path: PathBuf,
}

impl NodeRuntime {
    pub async fn start(config: NodeRuntimeConfig) -> anyhow::Result<Self> {
        let store_path = config
            .data_dir
            .join(format!("node-{}/events.jsonl", config.node_id));
        let store = Arc::new(ChainStore::open(&store_path)?);

        let engine = Engine::new(
            config.node_id,
            config.n,
            config.f,
            config.p,
            Arc::clone(&store),
        )?;
        let engine = Arc::new(Mutex::new(engine));

        let (inbound_tx, mut inbound_rx) = mpsc::channel::<WireMessage>(256);
        let gossip_node =
            Arc::new(GossipNode::start(config.p2p_bind, config.peers, inbound_tx).await?);
        let gossip_for_loop = Arc::clone(&gossip_node);

        let inbound_task = tokio::spawn(async move {
            while let Some(wire) = inbound_rx.recv().await {
                let outbound = {
                    let mut engine = engine.lock().await;
                    match engine.process_inbound(wire) {
                        Ok(out) => out,
                        Err(err) => {
                            eprintln!("node runtime engine error: {err}");
                            continue;
                        }
                    }
                };

                for msg in outbound {
                    gossip_for_loop.broadcast(&msg).await;
                }
            }
        });

        Ok(Self {
            gossip: gossip_node,
            inbound_task,
            store_path,
        })
    }

    #[allow(dead_code)]
    pub fn store_path(&self) -> &PathBuf {
        &self.store_path
    }

    pub fn shutdown(&self) {
        self.gossip.shutdown();
        self.inbound_task.abort();
    }
}

fn genesis_block(proposer: NodeId) -> Block {
    Block {
        header: BlockHeader {
            round: 0,
            proposer,
            parent_hash: [0; 32],
            payload_hash: [0; 32],
            rank: 0,
        },
        payload: BlockPayload {
            bytes: b"genesis".to_vec(),
        },
        signature: vec![],
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn parse_peers_handles_csv() {
        let peers = parse_peers("127.0.0.1:8124,127.0.0.1:8125").expect("parse peers");
        assert_eq!(peers.len(), 2);
    }

    #[tokio::test]
    async fn runtime_persists_genesis_event() {
        let tmp = tempdir().expect("tempdir");
        let cfg = NodeRuntimeConfig {
            node_id: 3,
            n: 4,
            f: 1,
            p: 1,
            p2p_bind: "127.0.0.1:0".parse().expect("bind"),
            peers: vec![],
            data_dir: tmp.path().to_path_buf(),
        };

        let runtime = match NodeRuntime::start(cfg).await {
            Ok(rt) => rt,
            Err(err)
                if err
                    .chain()
                    .any(|cause| cause.to_string().contains("Operation not permitted")) =>
            {
                return;
            }
            Err(err) => panic!("runtime should start: {err}"),
        };

        let content = fs::read_to_string(runtime.store_path()).expect("read store file");
        assert!(content.contains("Genesis"));

        runtime.shutdown();
    }
}
