use serde::{Deserialize, Serialize};

pub type BlockHash = [u8; 32];
pub type PayloadHash = [u8; 32];
pub type NodeId = u64;
pub type Signature = Vec<u8>;
pub type AggregateSignature = Vec<Signature>;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockHeader {
    pub round: u64,
    pub proposer: NodeId,
    pub parent_hash: BlockHash,
    pub payload_hash: PayloadHash,
    pub rank: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub payload: BlockPayload,
    pub signature: Signature,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockPayload {
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotarizationVote {
    pub round: u64,
    pub block_hash: BlockHash,
    pub voter: NodeId,
    pub signature: Signature,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalizationVote {
    pub round: u64,
    pub block_hash: BlockHash,
    pub voter: NodeId,
    pub signature: Signature,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastVote {
    pub round: u64,
    pub block_hash: BlockHash,
    pub voter: NodeId,
    pub signature: Signature,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotarizationCertificate {
    pub round: u64,
    pub block_hash: BlockHash,
    pub voters: Vec<NodeId>,
    pub signatures: AggregateSignature,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalizationCertificate {
    pub round: u64,
    pub block_hash: BlockHash,
    pub voters: Vec<NodeId>,
    pub signatures: AggregateSignature,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnlockProof {
    pub round: u64,
    pub block_hash: BlockHash,
    pub voters: Vec<NodeId>,
    pub signatures: AggregateSignature,
}
