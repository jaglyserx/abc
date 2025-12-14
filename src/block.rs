struct BlockHeader {
    parent_hash: [u8; 32],
    height: u64,
    tx_root: [u8; 32],
}

struct Block {
    headers: BlockHeader,
    transactions: Vec<i64>,
}
