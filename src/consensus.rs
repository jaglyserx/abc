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
    emitted_notarizations: HashSet<(u64, BlockHash)>,
    emitted_unlocks: HashSet<(u64, BlockHash)>,
    emitted_finalizations: HashSet<(u64, BlockHash)>,
    emitted_finalization_votes: HashSet<(u64, BlockHash)>,
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
            emitted_notarizations: HashSet::new(),
            emitted_unlocks: HashSet::new(),
            emitted_finalizations: HashSet::new(),
            emitted_finalization_votes: HashSet::new(),
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
        let key = (v.round, v.block_hash);

        (self.votes.count_notarization(v.round, v.block_hash) >= quorum_notarization(self.n, self.f)
            && self.emitted_notarizations.insert(key))
            .then(|| {
            let cert = self.votes.build_notarization(v.round, v.block_hash);
            self.tree.mark_notarized(cert.block_hash);
            ConsensusMsg::Notarization(cert)
        })
        .into_iter()
        .collect()
    }

    fn on_fast_vote(&mut self, v: FastVote) -> Vec<ConsensusMsg> {
        self.votes.insert_fast(v.clone());
        let key = (v.round, v.block_hash);

        (self.votes.count_fast(v.round, v.block_hash) >= quorum_fast(self.n, self.p)
            && self.emitted_unlocks.insert(key))
            .then(|| {
                let proof = self.votes.build_unlock_proof(v.round, v.block_hash);
                self.tree.mark_unlocked(proof.block_hash);
                ConsensusMsg::UnlockProof(proof)
            })
            .into_iter()
            .collect()
    }

    fn on_notarization(&mut self, c: NotarizationCertificate) -> Vec<ConsensusMsg> {
        self.tree.mark_notarized(c.block_hash);

        if self.tree.is_unlocked(c.block_hash) {
            self.start_round(c.round + 1);
        }

        let key = (c.round, c.block_hash);
        self.votes
            .only_block_in_round(c.round, c.block_hash)
            .then_some(self.emitted_finalization_votes.insert(key))
            .unwrap_or(false)
            .then_some(ConsensusMsg::FinalizationVote(FinalizationVote {
                round: c.round,
                block_hash: c.block_hash,
                voter: self.id,
                signature: Signature::new(),
            }))
            .into_iter()
            .collect()
    }

    fn on_unlock_proof(&mut self, p: UnlockProof) -> Vec<ConsensusMsg> {
        self.tree.mark_unlocked(p.block_hash);
        Vec::new()
    }

    fn on_finalization_vote(&mut self, v: FinalizationVote) -> Vec<ConsensusMsg> {
        self.votes.insert_finalization(v.clone());
        let key = (v.round, v.block_hash);
        (self.votes.count_finalization(v.round, v.block_hash)
            >= quorum_finalization(self.n, self.f)
            && self.emitted_finalizations.insert(key))
        .then_some(self.votes.build_finalization(v.round, v.block_hash))
        .map(ConsensusMsg::Finalization)
        .into_iter()
        .collect()
    }

    fn on_finalization(&mut self, c: FinalizationCertificate) -> Vec<ConsensusMsg> {
        self.tree.mark_finalized(c.block_hash);
        if c.round > self.k_max {
            self.k_max = c.round;
        }
        Vec::new()
    }

    fn valid_proposal(&self, p: &ProposalMsg) -> bool {
        let block = &p.block;
        let notarization_quorum = quorum_notarization(self.n, self.f);
        let fast_quorum = quorum_fast(self.n, self.p);

        if block.header.round != self.round || block.header.round == 0 {
            return false;
        }

        if !self.tree.nodes.contains_key(&block.header.parent_hash) {
            return false;
        }

        if p.parent_notarization.block_hash != block.header.parent_hash {
            return false;
        }

        if p.parent_notarization.round + 1 != block.header.round {
            return false;
        }

        if p.parent_notarization.voters.len() != p.parent_notarization.signatures.len()
            || p.parent_unlock.voters.len() != p.parent_unlock.signatures.len()
        {
            return false;
        }

        if p.parent_notarization.voters.len() < notarization_quorum {
            return false;
        }

        if p.parent_unlock.round != p.parent_notarization.round
            || p.parent_unlock.block_hash != p.parent_notarization.block_hash
            || p.parent_unlock.voters.len() < fast_quorum
        {
            return false;
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockHeader, BlockPayload};

    fn block(round: u64, proposer: NodeId, parent_hash: BlockHash, payload: &[u8]) -> Block {
        Block {
            header: BlockHeader {
                round,
                proposer,
                parent_hash,
                payload_hash: [0u8; 32],
                rank: 0,
            },
            payload: BlockPayload {
                bytes: payload.to_vec(),
            },
            signature: vec![],
        }
    }

    fn cert(round: u64, hash: BlockHash, voters: usize) -> NotarizationCertificate {
        NotarizationCertificate {
            round,
            block_hash: hash,
            voters: (0..voters as u64).collect(),
            signatures: vec![vec![]; voters],
        }
    }

    fn unlock(round: u64, hash: BlockHash, voters: usize) -> UnlockProof {
        UnlockProof {
            round,
            block_hash: hash,
            voters: (0..voters as u64).collect(),
            signatures: vec![vec![]; voters],
        }
    }

    #[test]
    fn valid_proposal_emits_votes() {
        let genesis = block(0, 0, [0; 32], b"genesis");
        let mut state = ConsensusState::new(1, 4, 1, 1, genesis.clone());
        state.start_round(1);

        let parent_hash = state.tree.block_hash(&genesis);
        let proposal_block = block(1, 2, parent_hash, b"proposal");
        let proposal = ProposalMsg {
            block: proposal_block,
            parent_notarization: cert(0, parent_hash, quorum_notarization(4, 1)),
            parent_unlock: unlock(0, parent_hash, quorum_fast(4, 1)),
        };

        let out = state.handle_msg(ConsensusMsg::Proposal(proposal));
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn invalid_proposal_parent_hash_is_rejected() {
        let genesis = block(0, 0, [0; 32], b"genesis");
        let mut state = ConsensusState::new(1, 4, 1, 1, genesis.clone());
        state.start_round(1);

        let parent_hash = state.tree.block_hash(&genesis);
        let bad_parent = [9u8; 32];
        let proposal = ProposalMsg {
            block: block(1, 2, bad_parent, b"proposal"),
            parent_notarization: cert(0, parent_hash, quorum_notarization(4, 1)),
            parent_unlock: unlock(0, parent_hash, quorum_fast(4, 1)),
        };

        let out = state.handle_msg(ConsensusMsg::Proposal(proposal));
        assert!(out.is_empty());
    }

    #[test]
    fn notarization_emits_certificate_once() {
        let genesis = block(0, 0, [0; 32], b"genesis");
        let mut state = ConsensusState::new(1, 4, 1, 1, genesis);
        let hash = [7u8; 32];

        for voter in 1..=3 {
            let out = state.handle_msg(ConsensusMsg::NotarizationVote(NotarizationVote {
                round: 1,
                block_hash: hash,
                voter,
                signature: vec![],
            }));
            if voter < 3 {
                assert!(out.is_empty());
            } else {
                assert_eq!(out.len(), 1);
            }
        }

        let extra = state.handle_msg(ConsensusMsg::NotarizationVote(NotarizationVote {
            round: 1,
            block_hash: hash,
            voter: 4,
            signature: vec![],
        }));
        assert!(extra.is_empty());
    }
}
