# Research: State Sharding & Cross-Shard Mechanics

## 1. Sharding Paradigms
- **Transaction / Execution Sharding**: Transactions are routed to shards based on sender/recipient address spaces. Execution occurs in parallel, but state remains partially synchronized.
- **State Sharding**: Both state and transaction execution are split across shards. Nodes in Shard $k$ store only state belonging to Shard $k$. Requires robust cross-shard communication.

## 2. Key Challenges
1. **Cross-shard atomicity**: Ensuring that a transfer from Shard A to Shard B cannot result in funds vanishing or being duplicated if a network partition occurs.
2. **Single-shard takeover attack**: An attacker with 10% of global stake might corrupt an individual shard if validators are statically assigned. Requires dynamic validator shuffling using Verifiable Random Functions (VRF).
