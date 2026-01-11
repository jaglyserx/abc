# Spec

This project will build a distributed blockchain protocol which need to be fast and robust. 
The Internet Computer have previously released several specifications for protocols which are used by it, 
there are called [ICC](https://internetcomputer.org/whitepapers/Internet%20Computer%20Consensus.pdf).

## Protocol 
Implement improved version of ICC, called [BANYAN](https://arxiv.org/pdf/2312.05869)

This is in worst case scenario as fast as ICC, but with a benevolent leader, it's faster since it only requires one roundtrip.
Every blockchain protocol is based on a Node API (an api running on a machine, or several), which both broadcast and receive messages.
In this project the messages are received through JSON-RPC. The node API needs to be able to handle user functions. For a normal wallet 
this includes checking the balance etc.
The node API must also handle incoming messages from other nodes. These include voting for proposal of notarization, vote of 
notarization and finalization.

Node's speak to each other by a Gossip Protocol. This means that the nodes at random send their state to other random nodes, so that the
state replication spreads like wildfire. This way it's very unlikely that nodes would miss data updates.

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
