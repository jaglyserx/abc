use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::block::{
    Block, BlockHash, FastVote, FinalizationCertificate, FinalizationVote, NodeId,
    NotarizationCertificate, NotarizationVote, Signature, UnlockProof,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ConsensusMsg {
    Proposal(ProposalMsg),
    NotarizationVote(NotarizationVote),
    FinalizationVote(FinalizationVote),
    FastVote(FastVote),
    Notarization(NotarizationCertificate),
    Finalization(FinalizationCertificate),
    UnlockProof(UnlockProof),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposalMsg {
    pub block: Block,
    pub parent_notarization: NotarizationCertificate,
    pub parent_unlock: UnlockProof,
}

#[derive(Clone, Debug)]
pub struct ConsensusState {
    pub id: NodeId,
    pub n: usize,
    pub f: usize,
    pub p: usize,
    pub round: u64,
    pub k_max: u64,
    pub fast_vote_sent: bool,
    pub proposed: bool,
    pub tree: BlockTree,
    pub votes: VotePools,
}

impl ConsensusState {
    pub fn new(id: NodeId, n: usize, f: usize, p: usize, genesis: Block) -> Self {
        let mut tree = BlockTree::new();
        tree.insert_genesis(genesis);
        Self {
            id,
            n,
            f,
            p,
            round: 0,
            k_max: 0,
            fast_vote_sent: false,
            proposed: false,
            tree,
            votes: VotePools::new(),
        }
    }

    pub fn start_round(&mut self, round: u64) {
        self.round = round;
        self.fast_vote_sent = false;
        self.proposed = false;
        self.votes.clear_round(round);
    }

    pub fn handle_msg(&mut self, msg: ConsensusMsg) -> Vec<ConsensusMsg> {
        match msg {
            ConsensusMsg::Proposal(p) => self.on_proposal(p),
            ConsensusMsg::NotarizationVote(v) => self.on_notarization_vote(v),
            ConsensusMsg::FinalizationVote(v) => self.on_finalization_vote(v),
            ConsensusMsg::FastVote(v) => self.on_fast_vote(v),
            ConsensusMsg::Notarization(c) => self.on_notarization(c),
            ConsensusMsg::Finalization(c) => self.on_finalization(c),
            ConsensusMsg::UnlockProof(p) => self.on_unlock_proof(p),
        }
    }

    fn on_proposal(&mut self, p: ProposalMsg) -> Vec<ConsensusMsg> {
        if !self.valid_proposal(&p) {
            return Vec::new();
        }
        self.tree.insert_block(p.block.clone());
        let mut out = Vec::new();
        let vote = NotarizationVote {
            round: p.block.header.round,
            block_hash: self.tree.block_hash(&p.block),
            voter: self.id,
            signature: Signature::new(),
        };
        out.push(ConsensusMsg::NotarizationVote(vote));
        if !self.fast_vote_sent {
            let fv = FastVote {
                round: p.block.header.round,
                block_hash: self.tree.block_hash(&p.block),
                voter: self.id,
                signature: Signature::new(),
            };
            out.push(ConsensusMsg::FastVote(fv));
            self.fast_vote_sent = true;
        }
        out
    }

    fn on_notarization_vote(&mut self, v: NotarizationVote) -> Vec<ConsensusMsg> {
        self.votes.insert_notarization(v.clone());
        if self.votes.count_notarization(v.round, v.block_hash)
            >= quorum_notarization(self.n, self.f)
        {
            let cert = self.votes.build_notarization(v.round, v.block_hash);
            self.tree.mark_notarized(cert.block_hash);
            return vec![ConsensusMsg::Notarization(cert)];
        }
        Vec::new()
    }

    fn on_fast_vote(&mut self, v: FastVote) -> Vec<ConsensusMsg> {
        self.votes.insert_fast(v.clone());
        if self.votes.count_fast(v.round, v.block_hash) >= quorum_fast(self.n, self.p) {
            let proof = self.votes.build_unlock_proof(v.round, v.block_hash);
            self.tree.mark_unlocked(proof.block_hash);
            return vec![ConsensusMsg::UnlockProof(proof)];
        }
        Vec::new()
    }

    fn on_notarization(&mut self, c: NotarizationCertificate) -> Vec<ConsensusMsg> {
        self.tree.mark_notarized(c.block_hash);
        let mut out = Vec::new();
        if self.votes.only_block_in_round(c.round, c.block_hash) {
            let v = FinalizationVote {
                round: c.round,
                block_hash: c.block_hash,
                voter: self.id,
                signature: Signature::new(),
            };
            out.push(ConsensusMsg::FinalizationVote(v));
        }
        if self.tree.is_unlocked(c.block_hash) {
            self.start_round(c.round + 1);
        }
        out
    }

    fn on_unlock_proof(&mut self, p: UnlockProof) -> Vec<ConsensusMsg> {
        self.tree.mark_unlocked(p.block_hash);
        Vec::new()
    }

    fn on_finalization_vote(&mut self, v: FinalizationVote) -> Vec<ConsensusMsg> {
        self.votes.insert_finalization(v.clone());
        if self.votes.count_finalization(v.round, v.block_hash)
            >= quorum_finalization(self.n, self.f)
        {
            let cert = self.votes.build_finalization(v.round, v.block_hash);
            return vec![ConsensusMsg::Finalization(cert)];
        }
        Vec::new()
    }

    fn on_finalization(&mut self, c: FinalizationCertificate) -> Vec<ConsensusMsg> {
        self.tree.mark_finalized(c.block_hash);
        if c.round > self.k_max {
            self.k_max = c.round;
        }
        Vec::new()
    }

    fn valid_proposal(&self, _p: &ProposalMsg) -> bool {
        // TODO: verify round, leader rank, signatures, and parent notarization/unlock proofs.
        true
    }
}

#[derive(Clone, Debug, Default)]
pub struct BlockTree {
    pub nodes: HashMap<BlockHash, BlockNode>,
    pub genesis: Option<BlockHash>,
}

#[derive(Clone, Debug)]
pub struct BlockNode {
    pub block: Block,
    pub parent: Option<BlockHash>,
    pub children: Vec<BlockHash>,
    pub notarized: bool,
    pub unlocked: bool,
    pub finalized: bool,
}

impl BlockTree {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            genesis: None,
        }
    }

    pub fn insert_genesis(&mut self, block: Block) {
        let hash = self.block_hash(&block);
        let node = BlockNode {
            block,
            parent: None,
            children: Vec::new(),
            notarized: true,
            unlocked: true,
            finalized: true,
        };
        self.nodes.insert(hash, node);
        self.genesis = Some(hash);
    }

    pub fn insert_block(&mut self, block: Block) {
        let parent = block.header.parent_hash;
        let hash = self.block_hash(&block);
        let node = BlockNode {
            block,
            parent: Some(parent),
            children: Vec::new(),
            notarized: false,
            unlocked: false,
            finalized: false,
        };
        self.nodes.insert(hash, node);
        if let Some(p) = self.nodes.get_mut(&parent) {
            p.children.push(hash);
        }
    }

    pub fn mark_notarized(&mut self, hash: BlockHash) {
        if let Some(node) = self.nodes.get_mut(&hash) {
            node.notarized = true;
        }
    }

    pub fn mark_unlocked(&mut self, hash: BlockHash) {
        if let Some(node) = self.nodes.get_mut(&hash) {
            node.unlocked = true;
        }
    }

    pub fn mark_finalized(&mut self, hash: BlockHash) {
        if let Some(node) = self.nodes.get_mut(&hash) {
            node.finalized = true;
        }
    }

    pub fn is_unlocked(&self, hash: BlockHash) -> bool {
        self.nodes.get(&hash).is_some_and(|n| n.unlocked)
    }

    pub fn block_hash(&self, block: &Block) -> BlockHash {
        use sha3::{Digest, Sha3_256};
        let mut hasher = Sha3_256::new();
        hasher.update(&block.header.round.to_le_bytes());
        hasher.update(&block.header.proposer.to_le_bytes());
        hasher.update(&block.header.parent_hash);
        hasher.update(&block.header.payload_hash);
        hasher.update(&block.header.rank.to_le_bytes());
        hasher.update(&block.payload.bytes);
        let out = hasher.finalize();
        out.into()
    }
}

#[derive(Clone, Debug, Default)]
pub struct VotePools {
    notarization: HashMap<(u64, BlockHash), HashMap<NodeId, Signature>>,
    finalization: HashMap<(u64, BlockHash), HashMap<NodeId, Signature>>,
    fast: HashMap<(u64, BlockHash), HashMap<NodeId, Signature>>,
    round_blocks: HashMap<u64, HashSet<BlockHash>>,
}

impl VotePools {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear_round(&mut self, round: u64) {
        self.round_blocks.remove(&round);
    }

    pub fn insert_notarization(&mut self, v: NotarizationVote) {
        let key = (v.round, v.block_hash);
        self.round_blocks
            .entry(v.round)
            .or_default()
            .insert(v.block_hash);
        self.notarization
            .entry(key)
            .or_default()
            .insert(v.voter, v.signature);
    }

    pub fn insert_finalization(&mut self, v: FinalizationVote) {
        let key = (v.round, v.block_hash);
        self.finalization
            .entry(key)
            .or_default()
            .insert(v.voter, v.signature);
    }

    pub fn insert_fast(&mut self, v: FastVote) {
        let key = (v.round, v.block_hash);
        self.fast
            .entry(key)
            .or_default()
            .insert(v.voter, v.signature);
    }

    pub fn count_notarization(&self, round: u64, hash: BlockHash) -> usize {
        self.notarization
            .get(&(round, hash))
            .map(|m| m.len())
            .unwrap_or(0)
    }

    pub fn count_finalization(&self, round: u64, hash: BlockHash) -> usize {
        self.finalization
            .get(&(round, hash))
            .map(|m| m.len())
            .unwrap_or(0)
    }

    pub fn count_fast(&self, round: u64, hash: BlockHash) -> usize {
        self.fast.get(&(round, hash)).map(|m| m.len()).unwrap_or(0)
    }

    pub fn build_notarization(&self, round: u64, hash: BlockHash) -> NotarizationCertificate {
        let voters = self
            .notarization
            .get(&(round, hash))
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_else(Vec::new);
        let signatures = self
            .notarization
            .get(&(round, hash))
            .map(|m| m.values().cloned().collect())
            .unwrap_or_else(Vec::new);
        NotarizationCertificate {
            round,
            block_hash: hash,
            voters,
            signatures,
        }
    }

    pub fn build_finalization(&self, round: u64, hash: BlockHash) -> FinalizationCertificate {
        let voters = self
            .finalization
            .get(&(round, hash))
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_else(Vec::new);
        let signatures = self
            .finalization
            .get(&(round, hash))
            .map(|m| m.values().cloned().collect())
            .unwrap_or_else(Vec::new);
        FinalizationCertificate {
            round,
            block_hash: hash,
            voters,
            signatures,
        }
    }

    pub fn build_unlock_proof(&self, round: u64, hash: BlockHash) -> UnlockProof {
        let voters = self
            .fast
            .get(&(round, hash))
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_else(Vec::new);
        let signatures = self
            .fast
            .get(&(round, hash))
            .map(|m| m.values().cloned().collect())
            .unwrap_or_else(Vec::new);
        UnlockProof {
            round,
            block_hash: hash,
            voters,
            signatures,
        }
    }

    pub fn only_block_in_round(&self, round: u64, hash: BlockHash) -> bool {
        self.round_blocks
            .get(&round)
            .map(|set| set.len() == 1 && set.contains(&hash))
            .unwrap_or(false)
    }
}

pub fn quorum_notarization(n: usize, f: usize) -> usize {
    (n + f + 2) / 2
}

pub fn quorum_finalization(n: usize, f: usize) -> usize {
    n.saturating_sub(f)
}

pub fn quorum_fast(n: usize, p: usize) -> usize {
    n.saturating_sub(p)
}
