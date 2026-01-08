// Byzantine Fault Tolerance with rotating leader: https://arxiv.org/pdf/2312.05869
// Currently unaware of faster, or generally better algorithm's for block proposition so will
// implement it.
//
// This will be an essential part of an SMR (State machine replication) protocol.
// These ensure all nodes in the network share the same state, in an efficient manner.
//
// Another necessary component will be an Internet Computer Consensus (ICC) protocol,
// which is essential to have several nodes agree on shared data.
// Examples of these include: Proof-of-Work (PoW), Proof-of-Authority (PoA), Proof-of-Stake (PoS).
//
// The BANYAN SMR protocol must satisfy:
// 1. Deadlock freeness (every sound must end with a block inclusion)
// 2. Safety (all honest blocks should finalise the same blocks in a replica)
// 3. Liveness (if the the current moment is perfectly sequenced, and the leader is honest, then
//    the suggested block is added to the block tree.)
// 4. Fast termination (if the network is momentarile synchronous, the leader is honest, and n-p
//    replicas are honest then the block is added to the tree and finalized in one roundtrip.
//
// Structure needs a block proposer which handles the BANYAN algorithm.
// To represent different types of payloads for different events we'll use a set of messages:
// 1. Proposal block
// 2. Vote
// 3. Quorum certificate
// 4. Timeout
// 5. Timeout certificate
struct BlockProposer {}

impl BlockProposer {
    pub fn new() -> BlockProposer {
        BlockProposer {}
    }
}

struct Block {}
