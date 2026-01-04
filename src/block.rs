// Byzantine Fault Tolerance with rotating leader: https://arxiv.org/pdf/2312.05869
// Currently unaware of faster, or generally better algorithm's for block proposition so will
// implement it.
//
// This will be an essential part of an SMR (State machine replication) protocol.
// These ensure all nodes in the network share the same state, in an efficient manner.
//
struct BlockProposer {}

impl BlockProposer {
    pub fn new() -> BlockProposer {
        BlockProposer {}
    }
}
