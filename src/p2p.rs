use std::{net::SocketAddr, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task::JoinHandle,
};

use crate::{block::NodeId, consensus::ConsensusMsg};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WireMessage {
    pub id: u64,
    pub from: NodeId,
    pub msg: ConsensusMsg,
}

pub struct GossipNode {
    #[allow(dead_code)]
    local_addr: SocketAddr,
    peers: Arc<Vec<SocketAddr>>,
    listener_task: JoinHandle<()>,
}

impl GossipNode {
    pub async fn start(
        bind_addr: SocketAddr,
        peers: Vec<SocketAddr>,
        inbound_tx: mpsc::Sender<WireMessage>,
    ) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(bind_addr).await?;
        let local_addr = listener.local_addr()?;
        let listener_task = tokio::spawn(async move {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(conn) => conn,
                    Err(err) => {
                        eprintln!("p2p accept error: {err}");
                        continue;
                    }
                };

                let tx = inbound_tx.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle_connection(stream, tx).await {
                        eprintln!("p2p connection error: {err}");
                    }
                });
            }
        });

        Ok(Self {
            local_addr,
            peers: Arc::new(peers),
            listener_task,
        })
    }

    #[allow(dead_code)]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub async fn broadcast(&self, message: &WireMessage) {
        let serialized = match serde_json::to_string(message) {
            Ok(value) => value,
            Err(err) => {
                eprintln!("p2p serialization error: {err}");
                return;
            }
        };

        for peer in self.peers.iter().copied() {
            let line = format!("{serialized}\n");
            tokio::spawn(async move {
                match TcpStream::connect(peer).await {
                    Ok(mut stream) => {
                        if let Err(err) = stream.write_all(line.as_bytes()).await {
                            eprintln!("p2p write to {peer} failed: {err}");
                        }
                    }
                    Err(err) => {
                        eprintln!("p2p connect to {peer} failed: {err}");
                    }
                }
            });
        }
    }

    pub fn shutdown(&self) {
        self.listener_task.abort();
    }
}

async fn handle_connection(
    stream: TcpStream,
    inbound_tx: mpsc::Sender<WireMessage>,
) -> anyhow::Result<()> {
    let reader = BufReader::new(stream);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let message: WireMessage = serde_json::from_str(&line)?;
        if inbound_tx.send(message).await.is_err() {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use tokio::{
        io::AsyncWriteExt,
        net::TcpStream,
        sync::mpsc,
        time::{Duration, timeout},
    };

    use super::*;

    #[tokio::test]
    async fn receives_wire_message_over_tcp() {
        let (tx, mut rx) = mpsc::channel(8);
        let bind: SocketAddr = "127.0.0.1:0".parse().expect("addr");

        let node = match GossipNode::start(bind, vec![], tx).await {
            Ok(node) => node,
            Err(err)
                if err
                    .chain()
                    .any(|cause| cause.to_string().contains("Operation not permitted")) =>
            {
                return;
            }
            Err(err) => panic!("listener should start: {err}"),
        };

        let message = WireMessage {
            id: 42,
            from: 7,
            msg: ConsensusMsg::NotarizationVote(crate::block::NotarizationVote {
                round: 1,
                block_hash: [3; 32],
                voter: 7,
                signature: vec![1, 2, 3],
            }),
        };

        let mut stream = TcpStream::connect(node.local_addr())
            .await
            .expect("connect listener");
        let payload = format!(
            "{}\n",
            serde_json::to_string(&message).expect("serialize message")
        );
        stream
            .write_all(payload.as_bytes())
            .await
            .expect("write message");
        drop(stream);

        let received = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("receive in time")
            .expect("message present");
        assert_eq!(received.id, 42);
        assert_eq!(received.from, 7);

        node.shutdown();
    }

    #[tokio::test]
    async fn broadcast_is_non_blocking_with_unreachable_peer() {
        let (tx, _rx) = mpsc::channel(1);
        let bind: SocketAddr = "127.0.0.1:0".parse().expect("addr");

        let node =
            match GossipNode::start(bind, vec!["127.0.0.1:9".parse().expect("peer")], tx).await {
                Ok(node) => node,
                Err(err)
                    if err
                        .chain()
                        .any(|cause| cause.to_string().contains("Operation not permitted")) =>
                {
                    return;
                }
                Err(err) => panic!("listener should start: {err}"),
            };

        let msg = WireMessage {
            id: 1,
            from: 1,
            msg: ConsensusMsg::UnlockProof(crate::block::UnlockProof {
                round: 1,
                block_hash: [1; 32],
                voters: vec![1],
                signatures: vec![vec![]],
            }),
        };

        timeout(Duration::from_millis(100), node.broadcast(&msg))
            .await
            .expect("broadcast should be quick");
        node.shutdown();
    }
}
