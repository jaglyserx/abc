# Spec

This project will build a distributed blockchain protocol which need to be fast and robust. 
The Internet Computer have previously released several specifications for protocols which are used by it, 
there are called [ICC](https://internetcomputer.org/whitepapers/Internet%20Computer%20Consensus.pdf).

## Protocol 
This project implements a practical Banyan/ICC-inspired protocol with a JSON-RPC node API, TCP gossip transport, and persistent local state.

Implemented behavior:
- Consensus core:
  - Proposal validation includes round checks, leader/rank constraints, parent proof linkage, quorum checks, and signature verification.
  - Vote equivocation is rejected per voter/round.
  - Timeout-driven round advancement is implemented (`on_tick`).
- Networking:
  - Nodes exchange consensus messages over TCP gossip using newline-delimited JSON wire messages.
  - Message replay is guarded by wire IDs in the node runtime.
- State execution:
  - Transactions are accepted into a mempool and executed in produced blocks.
  - Nonce/replay and balance checks are enforced; receipts are stored for committed/rejected txs.
- Persistence and recovery:
  - Snapshot + block history are persisted to disk (`state_snapshot.json`, `blocks.jsonl`).
  - Node startup rehydrates in-memory ledger state from persisted snapshot.
- Sync scaffolding:
  - Snapshot/block export-import RPCs are available for manual catch-up between nodes.

Primary RPC surface includes account creation, faucet, tx submission, block production, balance/nonce/receipt queries, health/metrics probes, and snapshot sync endpoints.

## Message types
1. Proposal - the leader node proposes another block for notarization, which means proposing for it to be included in the tree.
2. Notarization vote - vote for block to be notarized, which means it's the best block of the round.
3. Notarization - when block collects >= n - t votes. (n = committee size (shard), t < n /3) Node message that it is notarized. 
4. Finalization vote - vote for the block to be finalized and included in tree.
5. Finalization - block is finalized and included in tree forever.

The reason notarization and finalization is separated is because of delay in data delivery different nodes could believe different blocks
were notarized at the same time, if they were instantly committed the tree would fork and diverge. Therefore after notarization the blocks 
are finalised by all parties.

## Control flow
Above the control flow for the finalisation of a block in synchronous order is shown. However, there are both semi, och fully asynchronous settings. 
Sadly, so far there is not theoretical way to make a fully async setting secure for block finalisation, but a semi asynchronous setting can work, and
is most commonly used.

In the case of async, vs async what it really means is that whether the round fully waits for participants before timing them out if there has been no response.
As might be obvious from this statement is that in a decentralised network to do this rigoriously is impossible.

## Banyan fast round vs ICC slow round (message flow)
```
Slow ICC-style round (baseline)
  1) Leader proposes block
  2) Replicas send notarization votes
  3) Quorum -> Notarization certificate
  4) Replicas send finalization votes
  5) Quorum -> Finalization certificate -> commit chain

Fast Banyan round (good case, rank-0 leader)
  1) Leader proposes block + parent notarization + unlock proof
  2) Replicas send fast vote + notarization vote
  3) Quorum -> notarization + unlock proof
  4) Fast finalization votes for rank-0 block
  5) Quorum -> finalization -> commit chain

Notes:
- Fast path runs concurrently with the slow path; if fast path fails, slow path still progresses.
- "Unlock proof" is built from fast votes and permits fast finalization.
```

## Quorum thresholds (quick reference)
- Notarization quorum (Banyan): ⌈(n + f + 1) / 2⌉ votes.
- Finalization quorum: usually `n - f` (i.e., `2f + 1` when `n = 3f + 1`).
- Fast-path quorum (rank-0 fast finalization): `n - p` fast votes, where `p` is the max fast-path faults.

Definitions:
- `n` = committee size (replicas in the round).
- `f` = maximum Byzantine faults (typically `f < n/3`).
- `p` = maximum faults tolerated specifically by the fast path (see Banyan paper).
