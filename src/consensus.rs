use std::collections::{HashMap, HashSet};

use secp256k1::{Message, PublicKey, Secp256k1, SecretKey, ecdsa::Signature as EcdsaSignature};
use serde::{Deserialize, Serialize};

use crate::block::{
    Block, BlockHash, FastVote, FinalizationCertificate, FinalizationVote, NodeId,
    NotarizationCertificate, NotarizationVote, Signature, UnlockProof,
};

#[allow(clippy::large_enum_variant)]
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
    validator_pubkeys: HashMap<NodeId, PublicKey>,
    signer_secret: SecretKey,
    current_tick: u64,
    round_started_at_tick: u64,
    round_timeout_ticks: u64,
    voted_notarization_rounds: HashSet<u64>,
    emitted_notarizations: HashSet<(u64, BlockHash)>,
    emitted_unlocks: HashSet<(u64, BlockHash)>,
    emitted_finalizations: HashSet<(u64, BlockHash)>,
    emitted_finalization_votes: HashSet<(u64, BlockHash)>,
}

impl ConsensusState {
    pub fn new(id: NodeId, n: usize, f: usize, p: usize, genesis: Block) -> Self {
        let mut tree = BlockTree::new();
        tree.insert_genesis(genesis);
        let (validator_pubkeys, signer_secret) = validator_keys(id, n);
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
            validator_pubkeys,
            signer_secret,
            current_tick: 0,
            round_started_at_tick: 0,
            round_timeout_ticks: 5,
            voted_notarization_rounds: HashSet::new(),
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
        self.round_started_at_tick = self.current_tick;
        self.votes.clear_round(round);
    }

    #[allow(dead_code)]
    pub fn on_tick(&mut self) -> Option<u64> {
        self.current_tick = self.current_tick.saturating_add(1);
        if self.current_tick.saturating_sub(self.round_started_at_tick) >= self.round_timeout_ticks
        {
            self.start_round(self.round.saturating_add(1));
            return Some(self.round);
        }
        None
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
        if self
            .voted_notarization_rounds
            .contains(&p.block.header.round)
        {
            return Vec::new();
        }
        self.tree.insert_block(p.block.clone());

        let block_hash = self.tree.block_hash(&p.block);
        let mut out = Vec::new();
        let vote = NotarizationVote {
            round: p.block.header.round,
            block_hash,
            voter: self.id,
            signature: self.sign_vote(SigKind::Notarization, p.block.header.round, block_hash),
        };
        self.voted_notarization_rounds.insert(p.block.header.round);
        out.push(ConsensusMsg::NotarizationVote(vote));
        if !self.fast_vote_sent {
            let fv = FastVote {
                round: p.block.header.round,
                block_hash,
                voter: self.id,
                signature: self.sign_vote(SigKind::Fast, p.block.header.round, block_hash),
            };
            out.push(ConsensusMsg::FastVote(fv));
            self.fast_vote_sent = true;
        }
        out
    }

    fn on_notarization_vote(&mut self, v: NotarizationVote) -> Vec<ConsensusMsg> {
        if !self.valid_vote_target(v.round, v.block_hash) || !self.valid_voter(v.voter) {
            return Vec::new();
        }
        if !self.verify_vote_signature(
            SigKind::Notarization,
            v.round,
            v.block_hash,
            v.voter,
            &v.signature,
        ) {
            return Vec::new();
        }

        if !self.votes.insert_notarization(v.clone()) {
            return Vec::new();
        }
        let key = (v.round, v.block_hash);

        (self.votes.count_notarization(v.round, v.block_hash)
            >= quorum_notarization(self.n, self.f)
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
        if !self.valid_vote_target(v.round, v.block_hash) || !self.valid_voter(v.voter) {
            return Vec::new();
        }
        if !self.verify_vote_signature(SigKind::Fast, v.round, v.block_hash, v.voter, &v.signature)
        {
            return Vec::new();
        }

        if !self.votes.insert_fast(v.clone()) {
            return Vec::new();
        }
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
        if !self.valid_notarization_certificate(&c) {
            return Vec::new();
        }

        self.tree.mark_notarized(c.block_hash);

        if self.tree.is_unlocked(c.block_hash) {
            self.start_round(c.round + 1);
        }

        let key = (c.round, c.block_hash);
        (if self.votes.only_block_in_round(c.round, c.block_hash) {
            self.emitted_finalization_votes.insert(key)
        } else {
            false
        })
        .then_some(ConsensusMsg::FinalizationVote(FinalizationVote {
            round: c.round,
            block_hash: c.block_hash,
            voter: self.id,
            signature: self.sign_vote(SigKind::Finalization, c.round, c.block_hash),
        }))
        .into_iter()
        .collect()
    }

    fn on_unlock_proof(&mut self, p: UnlockProof) -> Vec<ConsensusMsg> {
        if !self.valid_unlock_proof(&p) {
            return Vec::new();
        }

        self.tree.mark_unlocked(p.block_hash);
        Vec::new()
    }

    fn on_finalization_vote(&mut self, v: FinalizationVote) -> Vec<ConsensusMsg> {
        if !self.valid_vote_target(v.round, v.block_hash) || !self.valid_voter(v.voter) {
            return Vec::new();
        }
        if !self.verify_vote_signature(
            SigKind::Finalization,
            v.round,
            v.block_hash,
            v.voter,
            &v.signature,
        ) {
            return Vec::new();
        }

        if !self.votes.insert_finalization(v.clone()) {
            return Vec::new();
        }
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
        if !self.valid_finalization_certificate(&c) {
            return Vec::new();
        }

        self.tree.mark_finalized(c.block_hash);
        if c.round > self.k_max {
            self.k_max = c.round;
        }
        Vec::new()
    }

    fn valid_proposal(&self, p: &ProposalMsg) -> bool {
        let block = &p.block;
        let leader = self.leader_for_round(block.header.round);

        if block.header.round != self.round || block.header.round == 0 {
            return false;
        }
        if block.header.rank == 0 {
            if block.header.proposer != leader {
                return false;
            }
        } else if block.header.proposer == leader || !self.round_timed_out() {
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

        if !self.valid_notarization_certificate(&p.parent_notarization)
            || !self.valid_unlock_proof(&p.parent_unlock)
            || p.parent_unlock.round != p.parent_notarization.round
            || p.parent_unlock.block_hash != p.parent_notarization.block_hash
        {
            return false;
        }

        true
    }

    fn round_timed_out(&self) -> bool {
        self.current_tick.saturating_sub(self.round_started_at_tick) >= self.round_timeout_ticks
    }

    fn leader_for_round(&self, round: u64) -> NodeId {
        if self.n == 0 {
            0
        } else {
            round % self.n as u64
        }
    }

    fn valid_voter(&self, voter: NodeId) -> bool {
        (voter as usize) < self.n
    }

    fn valid_vote_target(&self, round: u64, block_hash: BlockHash) -> bool {
        self.tree
            .nodes
            .get(&block_hash)
            .is_some_and(|node| node.block.header.round == round)
    }

    fn valid_notarization_certificate(&self, c: &NotarizationCertificate) -> bool {
        self.valid_vote_target(c.round, c.block_hash)
            && self.valid_participants(
                SigKind::Notarization,
                c.round,
                c.block_hash,
                &c.voters,
                &c.signatures,
            )
            && c.voters.len() >= quorum_notarization(self.n, self.f)
    }

    fn valid_unlock_proof(&self, p: &UnlockProof) -> bool {
        self.valid_vote_target(p.round, p.block_hash)
            && self.valid_participants(
                SigKind::Fast,
                p.round,
                p.block_hash,
                &p.voters,
                &p.signatures,
            )
            && p.voters.len() >= quorum_fast(self.n, self.p)
    }

    fn valid_finalization_certificate(&self, c: &FinalizationCertificate) -> bool {
        self.valid_vote_target(c.round, c.block_hash)
            && self.valid_participants(
                SigKind::Finalization,
                c.round,
                c.block_hash,
                &c.voters,
                &c.signatures,
            )
            && c.voters.len() >= quorum_finalization(self.n, self.f)
    }

    fn valid_participants(
        &self,
        kind: SigKind,
        round: u64,
        block_hash: BlockHash,
        voters: &[NodeId],
        signatures: &[Signature],
    ) -> bool {
        if voters.len() != signatures.len() {
            return false;
        }

        let mut unique = HashSet::new();
        voters.iter().zip(signatures).all(|(voter, signature)| {
            self.valid_voter(*voter)
                && unique.insert(*voter)
                && self.verify_vote_signature(kind, round, block_hash, *voter, signature)
        })
    }

    fn sign_vote(&self, kind: SigKind, round: u64, block_hash: BlockHash) -> Signature {
        let secp = Secp256k1::new();
        let digest = vote_digest(kind, round, block_hash);
        let msg = Message::from_digest(digest);
        let sig = secp.sign_ecdsa(msg, &self.signer_secret);
        sig.serialize_compact().to_vec()
    }

    fn verify_vote_signature(
        &self,
        kind: SigKind,
        round: u64,
        block_hash: BlockHash,
        voter: NodeId,
        signature: &[u8],
    ) -> bool {
        let pubkey = match self.validator_pubkeys.get(&voter) {
            Some(key) => key,
            None => return false,
        };
        if signature.len() != 64 {
            return false;
        }

        let sig = match EcdsaSignature::from_compact(signature) {
            Ok(sig) => sig,
            Err(_) => return false,
        };
        let digest = vote_digest(kind, round, block_hash);
        let msg = Message::from_digest(digest);
        Secp256k1::new().verify_ecdsa(msg, &sig, pubkey).is_ok()
    }
}

#[derive(Clone, Copy)]
enum SigKind {
    Notarization,
    Finalization,
    Fast,
}

fn vote_digest(kind: SigKind, round: u64, block_hash: BlockHash) -> [u8; 32] {
    use sha3::{Digest, Sha3_256};
    let mut hasher = Sha3_256::new();
    let kind_byte = match kind {
        SigKind::Notarization => 1u8,
        SigKind::Finalization => 2u8,
        SigKind::Fast => 3u8,
    };
    hasher.update([kind_byte]);
    hasher.update(round.to_le_bytes());
    hasher.update(block_hash);
    hasher.finalize().into()
}

fn validator_keys(id: NodeId, n: usize) -> (HashMap<NodeId, PublicKey>, SecretKey) {
    let mut map = HashMap::new();
    let mut local = deterministic_secret(id);
    let secp = Secp256k1::new();
    for voter in 0..n as u64 {
        let secret = deterministic_secret(voter);
        if voter == id {
            local = secret;
        }
        map.insert(voter, PublicKey::from_secret_key(&secp, &secret));
    }
    (map, local)
}

fn deterministic_secret(node_id: NodeId) -> SecretKey {
    use sha3::{Digest, Sha3_256};

    let mut ctr = 0u64;
    loop {
        let mut hasher = Sha3_256::new();
        hasher.update(node_id.to_le_bytes());
        hasher.update(ctr.to_le_bytes());
        let digest: [u8; 32] = hasher.finalize().into();
        if let Ok(secret) = SecretKey::from_byte_array(digest) {
            return secret;
        }
        ctr = ctr.saturating_add(1);
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
        hasher.update(block.header.round.to_le_bytes());
        hasher.update(block.header.proposer.to_le_bytes());
        hasher.update(block.header.parent_hash);
        hasher.update(block.header.payload_hash);
        hasher.update(block.header.rank.to_le_bytes());
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
    notarization_by_voter_round: HashMap<(u64, NodeId), BlockHash>,
    finalization_by_voter_round: HashMap<(u64, NodeId), BlockHash>,
    fast_by_voter_round: HashMap<(u64, NodeId), BlockHash>,
}

impl VotePools {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear_round(&mut self, round: u64) {
        self.round_blocks.remove(&round);
        self.notarization_by_voter_round
            .retain(|(r, _), _| *r != round);
        self.finalization_by_voter_round
            .retain(|(r, _), _| *r != round);
        self.fast_by_voter_round.retain(|(r, _), _| *r != round);
    }

    pub fn insert_notarization(&mut self, v: NotarizationVote) -> bool {
        if !Self::accepts_vote(
            v.round,
            v.voter,
            v.block_hash,
            &mut self.notarization_by_voter_round,
        ) {
            return false;
        }

        let key = (v.round, v.block_hash);
        self.round_blocks
            .entry(v.round)
            .or_default()
            .insert(v.block_hash);
        self.notarization
            .entry(key)
            .or_default()
            .insert(v.voter, v.signature);
        true
    }

    pub fn insert_finalization(&mut self, v: FinalizationVote) -> bool {
        if !Self::accepts_vote(
            v.round,
            v.voter,
            v.block_hash,
            &mut self.finalization_by_voter_round,
        ) {
            return false;
        }

        let key = (v.round, v.block_hash);
        self.finalization
            .entry(key)
            .or_default()
            .insert(v.voter, v.signature);
        true
    }

    pub fn insert_fast(&mut self, v: FastVote) -> bool {
        if !Self::accepts_vote(
            v.round,
            v.voter,
            v.block_hash,
            &mut self.fast_by_voter_round,
        ) {
            return false;
        }

        let key = (v.round, v.block_hash);
        self.fast
            .entry(key)
            .or_default()
            .insert(v.voter, v.signature);
        true
    }

    fn accepts_vote(
        round: u64,
        voter: NodeId,
        block_hash: BlockHash,
        seen: &mut HashMap<(u64, NodeId), BlockHash>,
    ) -> bool {
        let key = (round, voter);
        match seen.get(&key).copied() {
            Some(existing) => existing == block_hash,
            None => {
                seen.insert(key, block_hash);
                true
            }
        }
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
        let (voters, signatures) = self
            .notarization
            .get(&(round, hash))
            .map(ordered_signers)
            .unwrap_or_default();

        NotarizationCertificate {
            round,
            block_hash: hash,
            voters,
            signatures,
        }
    }

    pub fn build_finalization(&self, round: u64, hash: BlockHash) -> FinalizationCertificate {
        let (voters, signatures) = self
            .finalization
            .get(&(round, hash))
            .map(ordered_signers)
            .unwrap_or_default();

        FinalizationCertificate {
            round,
            block_hash: hash,
            voters,
            signatures,
        }
    }

    pub fn build_unlock_proof(&self, round: u64, hash: BlockHash) -> UnlockProof {
        let (voters, signatures) = self
            .fast
            .get(&(round, hash))
            .map(ordered_signers)
            .unwrap_or_default();

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

fn ordered_signers(votes: &HashMap<NodeId, Signature>) -> (Vec<NodeId>, Vec<Signature>) {
    let mut entries: Vec<(NodeId, Signature)> =
        votes.iter().map(|(k, v)| (*k, v.clone())).collect();
    entries.sort_by_key(|(voter, _)| *voter);
    let voters = entries.iter().map(|(voter, _)| *voter).collect();
    let signatures = entries.into_iter().map(|(_, sig)| sig).collect();
    (voters, signatures)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockHeader, BlockPayload};
    use std::collections::VecDeque;

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
        let voters_vec: Vec<NodeId> = (0..voters as u64).collect();
        NotarizationCertificate {
            round,
            block_hash: hash,
            signatures: voters_vec
                .iter()
                .map(|v| sign_for_node(*v, SigKind::Notarization, round, hash))
                .collect(),
            voters: voters_vec,
        }
    }

    fn unlock(round: u64, hash: BlockHash, voters: usize) -> UnlockProof {
        let voters_vec: Vec<NodeId> = (0..voters as u64).collect();
        UnlockProof {
            round,
            block_hash: hash,
            signatures: voters_vec
                .iter()
                .map(|v| sign_for_node(*v, SigKind::Fast, round, hash))
                .collect(),
            voters: voters_vec,
        }
    }

    fn sign_for_node(voter: NodeId, kind: SigKind, round: u64, hash: BlockHash) -> Signature {
        let secret = deterministic_secret(voter);
        let msg = Message::from_digest(vote_digest(kind, round, hash));
        let sig = Secp256k1::new().sign_ecdsa(msg, &secret);
        sig.serialize_compact().to_vec()
    }

    #[test]
    fn valid_proposal_emits_votes() {
        let genesis = block(0, 0, [0; 32], b"genesis");
        let mut state = ConsensusState::new(1, 4, 1, 1, genesis.clone());
        state.start_round(1);

        let parent_hash = state.tree.block_hash(&genesis);
        let proposal_block = block(1, 1, parent_hash, b"proposal");
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
        let mut state = ConsensusState::new(1, 4, 1, 1, genesis.clone());
        state.start_round(1);
        let b1 = block(1, 2, state.tree.block_hash(&genesis), b"b1");
        let hash = state.tree.block_hash(&b1);
        state.tree.insert_block(b1);

        for voter in 1..=3 {
            let out = state.handle_msg(ConsensusMsg::NotarizationVote(NotarizationVote {
                round: 1,
                block_hash: hash,
                voter,
                signature: sign_for_node(voter, SigKind::Notarization, 1, hash),
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
            voter: 3,
            signature: sign_for_node(3, SigKind::Notarization, 1, hash),
        }));
        assert!(extra.is_empty());
    }

    #[test]
    fn rejects_equivocating_notarization_vote() {
        let genesis = block(0, 0, [0; 32], b"genesis");
        let mut state = ConsensusState::new(1, 4, 1, 1, genesis.clone());
        state.start_round(1);

        let parent_hash = state.tree.block_hash(&genesis);
        let a = block(1, 2, parent_hash, b"a");
        let b = block(1, 3, parent_hash, b"b");
        let ah = state.tree.block_hash(&a);
        let bh = state.tree.block_hash(&b);
        state.tree.insert_block(a);
        state.tree.insert_block(b);

        let first = state.handle_msg(ConsensusMsg::NotarizationVote(NotarizationVote {
            round: 1,
            block_hash: ah,
            voter: 2,
            signature: sign_for_node(2, SigKind::Notarization, 1, ah),
        }));
        assert!(first.is_empty());

        let second = state.handle_msg(ConsensusMsg::NotarizationVote(NotarizationVote {
            round: 1,
            block_hash: bh,
            voter: 2,
            signature: sign_for_node(2, SigKind::Notarization, 1, bh),
        }));
        assert!(second.is_empty());
        assert_eq!(state.votes.count_notarization(1, bh), 0);
    }

    #[test]
    fn rejects_malformed_notarization_certificate() {
        let genesis = block(0, 0, [0; 32], b"genesis");
        let mut state = ConsensusState::new(1, 4, 1, 1, genesis.clone());
        state.start_round(1);
        let b1 = block(1, 2, state.tree.block_hash(&genesis), b"b1");
        let hash = state.tree.block_hash(&b1);
        state.tree.insert_block(b1);

        let malformed = NotarizationCertificate {
            round: 1,
            block_hash: hash,
            voters: vec![1, 1, 2],
            signatures: vec![
                sign_for_node(1, SigKind::Notarization, 1, hash),
                sign_for_node(1, SigKind::Notarization, 1, hash),
                sign_for_node(2, SigKind::Notarization, 1, hash),
            ],
        };

        let out = state.handle_msg(ConsensusMsg::Notarization(malformed));
        assert!(out.is_empty());
        assert!(!state.tree.nodes.get(&hash).is_some_and(|n| n.notarized));
    }

    #[test]
    fn timeout_advances_round() {
        let genesis = block(0, 0, [0; 32], b"genesis");
        let mut state = ConsensusState::new(1, 4, 1, 1, genesis);
        state.start_round(1);
        for _ in 0..4 {
            assert_eq!(state.on_tick(), None);
        }
        assert_eq!(state.on_tick(), Some(2));
        assert_eq!(state.round, 2);
    }

    #[test]
    fn non_leader_rank_zero_proposal_is_rejected() {
        let genesis = block(0, 0, [0; 32], b"genesis");
        let mut state = ConsensusState::new(1, 4, 1, 1, genesis.clone());
        state.start_round(1);
        let parent_hash = state.tree.block_hash(&genesis);

        let proposal = ProposalMsg {
            block: block(1, 2, parent_hash, b"bad-leader"),
            parent_notarization: cert(0, parent_hash, quorum_notarization(4, 1)),
            parent_unlock: unlock(0, parent_hash, quorum_fast(4, 1)),
        };
        let out = state.handle_msg(ConsensusMsg::Proposal(proposal));
        assert!(out.is_empty());
    }

    #[test]
    fn deterministic_simulation_handles_delay_drop_and_byzantine() {
        #[derive(Clone)]
        struct Envelope {
            at: u64,
            to: NodeId,
            msg: ConsensusMsg,
        }

        let genesis = block(0, 0, [0; 32], b"genesis");
        let mut nodes: HashMap<NodeId, ConsensusState> = (0..4u64)
            .map(|id| {
                let mut state = ConsensusState::new(id, 4, 1, 1, genesis.clone());
                state.start_round(1);
                (id, state)
            })
            .collect();

        let parent_hash = nodes.get(&0).expect("node").tree.block_hash(&genesis);
        let proposal_block = block(1, 1, parent_hash, b"sim");
        let proposal_hash = nodes
            .get(&0)
            .expect("node")
            .tree
            .block_hash(&proposal_block);
        let proposal = ConsensusMsg::Proposal(ProposalMsg {
            block: proposal_block,
            parent_notarization: cert(0, parent_hash, quorum_notarization(4, 1)),
            parent_unlock: unlock(0, parent_hash, quorum_fast(4, 1)),
        });

        let mut queue = VecDeque::new();
        for to in 0..4u64 {
            queue.push_back(Envelope {
                at: (to + 1) % 3,
                to,
                msg: proposal.clone(),
            });
        }

        let mut tick = 0u64;
        while tick < 40 {
            let mut ready = Vec::new();
            let mut rest = VecDeque::new();
            while let Some(env) = queue.pop_front() {
                if env.at <= tick {
                    ready.push(env);
                } else {
                    rest.push_back(env);
                }
            }
            queue = rest;

            for env in ready {
                let outputs = nodes
                    .get_mut(&env.to)
                    .expect("node exists")
                    .handle_msg(env.msg.clone());

                for out in outputs {
                    for to in 0..4u64 {
                        // deterministic byzantine/drop behavior: node 3 injects invalid votes, and some gossip is dropped.
                        if env.to == 3 {
                            queue.push_back(Envelope {
                                at: tick + 1,
                                to,
                                msg: ConsensusMsg::NotarizationVote(NotarizationVote {
                                    round: 1,
                                    block_hash: proposal_hash,
                                    voter: 3,
                                    signature: vec![9; 64],
                                }),
                            });
                        }

                        let drop = (tick + to + env.to).is_multiple_of(7);
                        if drop {
                            continue;
                        }
                        let delay = ((tick + to + env.to) % 3) + 1;
                        queue.push_back(Envelope {
                            at: tick + delay,
                            to,
                            msg: out.clone(),
                        });
                    }
                }
            }

            for node in nodes.values_mut() {
                let _ = node.on_tick();
            }
            tick += 1;
        }

        let finalized_counts: Vec<usize> = nodes
            .values()
            .map(|state| {
                state
                    .tree
                    .nodes
                    .values()
                    .filter(|n| n.block.header.round == 1 && n.finalized)
                    .count()
            })
            .collect();
        assert!(finalized_counts.iter().any(|count| *count > 0));
    }
}
