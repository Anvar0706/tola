# TOLA Network: Consensus Specification v0.1

## 1. Pipelined BFT State Machine (Jolteon/HotStuff Paradigm)

Consensus proceeds in discrete Views $V = (H, R)$ where $H$ is block height and $R$ is consensus round:

```
View V:
Leader Proposes Block(H) with QC(H-1)
               │ Broadcast via GossipSub
               ▼
Validators Verify Block & Check Locking Rules
               │ Send Vote to Leader (View V)
               ▼
Leader Collects Quorum (>= 2/3 + 1 Voting Power)
               │ Assembles Quorum Certificate: QC(H)
               ▼
View V+1:
Leader Proposes Block(H+1) incorporating QC(H)
               │ (When QC(H) is observed, Block H is FINALIZED)
```

---

## 2. Quorum Certificate (QC) Data Structure

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuorumCertificate {
    pub height: u64,
    pub round: u32,
    pub block_hash: [u8; 32],
    pub vote_type: VoteType, // Prevote | Precommit
    pub validator_bitfield: Vec<u8>,
    pub aggregated_signature: Vec<u8>, // BLS12-381 (96 bytes) or Ed25519 list
}
```

---

## 3. Equivocation Detection & Automated Slashing

If two distinct votes $V_1, V_2$ are observed such that:
$$V_1.\text{validator} == V_2.\text{validator} \land V_1.\text{height} == V_2.\text{height} \land V_1.\text{round} == V_2.\text{round} \land V_1.\text{block\_hash} \ne V_2.\text{block\_hash}$$

The evidence is packaged into a `SlashingEvidence` transaction:
1. Validator is immediately removed from the active consensus set (`Jailed`).
2. $10\%$ of bonded stake is burned permanently.
3. Remaining stake enters an extended 30-day slashing escrow.
