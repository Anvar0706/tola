# TOLA Network: Consensus Engine & Pipelined BFT Specification

## 1. Overview & Architectural Motivation
The **TOLA Consensus Engine** (`tola-consensus`) implements a high-assurance, Byzantine Fault Tolerant (BFT) Proof-of-Stake consensus state machine conforming to **ADR-0009** and **architecture/consensus-specification-v0.1.md**.

In classical BFT protocols (e.g. Tendermint/PBFT), all-to-all voting gossip incurs quadratic $O(N^2)$ message complexity, creating severe network degradation when validator counts exceed 100. Furthermore, naive 1-second block targets violate speed-of-light cross-continental latency bounds under global validator topologies (proven in `research/06-consensus-validation.md`).

TOLA solves these limitations by implementing **Pipelined BFT (Jolteon/HotStuff paradigm)** with:
1. **Linear $O(N)$ Message Complexity:** Validators cast votes directly to the view leader; the leader aggregates votes into a compact threshold Quorum Certificate (QC).
2. **Deterministic 2-Chain Finality:** Consecutive block pipelining finalizes blocks in 2 rounds ($4.0 - 5.0\text{ seconds}$) without probabilistic reorganizations.
3. **Decoupled Execution Pipeline:** State root execution of block $H-1$ proceeds concurrently while consensus pipeline proposes and validates block $H$.

---

## 2. Quorum Invariant & Byzantine Fault Tolerance

For an active validator committee with total voting power $W$, the system safely tolerates up to $f < W / 3$ Byzantine or crashed voting power:

$$Q = \left\lfloor \frac{2 \cdot W}{3} \right\rfloor + 1$$

- **Safety Invariant:** Two conflicting Quorum Certificates can never be formed for the same height and round, because any two quorums must intersect by at least $f + 1$ honest validators:
  $$2 \cdot Q - W = 2 \left(\left\lfloor \frac{2W}{3} \right\rfloor + 1\right) - W \ge \frac{W}{3} + 1 > f$$

---

## 3. Consensus Lifecycle: Views & 2-Chain Finality

Consensus proceeds in discrete Views $V = (H, R)$, where $H$ is block height and $R$ is the consensus round:

```
View (H, R):
   [ Leader Election ] -> Deterministic BLAKE3 stake-weighted sampling
          |
   [ Leader Proposes Block B_H ] -> Includes QC(H-1) justifying parent
          |
   [ Validators Verify & Lock ] -> Checks Locking Rule (round >= locked_qc.round)
          |
   [ Validators Cast Precommit Votes ] -> Ed25519 signature to leader
          |
   [ Leader Aggregates Votes ] -> Bitfield + concatenated signatures
          |
   [ Quorum Certificate QC(H) Formed ]
          |
View (H+1, R'):
   [ Leader Proposes Block B_{H+1} ] -> Includes QC(H)
          |
   [ QC(H+1) Formed ]
          |
   ========================================================================
   2-CHAIN COMMIT RULE: B_H is permanently FINALIZED and IRREVERSIBLE!
   ========================================================================
```

### 3.1 The 2-Chain Commit Rule
Under Jolteon's 2-chain rule:
- Block $B_1$ achieves $QC_1$.
- Block $B_2$ references $B_1$ and achieves $QC_2$.
- When $QC_2$ is observed by any honest validator, block $B_1$ is **committed**.
- Once finalized, block $B_1$ can NEVER be reorganized. Node state advances `last_finalized = H(B_1)`.

---

## 4. HotStuff Locking Safety Rule

To guarantee safety across asynchronous network partitions and view changes:
1. **Locking Rule:** Each validator maintains `locked_qc`. When a validator observes a valid QC at round $R$, it updates `locked_qc = QC` if $R > \text{locked\_qc}.\text{round}$.
2. **Voting Rule:** A validator votes for a proposal $B$ with parent certificate $QC_{\text{parent}}$ if and only if:
   $$QC_{\text{parent}}.\text{round} \ge \text{locked\_qc}.\text{round} \quad \lor \quad B \text{ extends } \text{locked\_qc}.\text{block}$$
This invariant prevents a partitioned minority from creating a conflicting fork after an asynchronous delay.

---

## 5. Quorum Certificate (QC) Structure

```rust
pub struct QuorumCertificate {
    pub height: BlockHeight,             // 8 bytes LE
    pub round: Round,                     // 8 bytes LE
    pub block_hash: Hash,                 // 32 bytes BLAKE3
    pub vote_type: VoteType,             // 1 byte (Precommit)
    pub validator_bitfield: Vec<u8>,     // ceil(N / 8) bytes
    pub aggregated_signature: Vec<u8>,   // N * 64 bytes Ed25519 signatures
}
```

- **Bitfield Canonical Ordering:** Bit $i$ indicates that the $i$-th validator in `committee.validators` signed the vote.
- **Signature Ordering:** Individual 64-byte Ed25519 signatures are concatenated strictly in ascending validator index order, enabling $O(1)$ stream verification against committee public keys.

---

## 6. Equivocation Detection & Slashing

A validator equivocates if it casts two conflicting votes for different block hashes at the exact same height and round:

$$V_1.\text{validator} == V_2.\text{validator} \land V_1.\text{height} == V_2.\text{height} \land V_1.\text{round} == V_2.\text{round} \land V_1.\text{block\_hash} \ne V_2.\text{block\_hash}$$

- **Detection:** `VoteAggregator` tracks all cast votes in-memory. Conflicting votes immediately halt with `ConsensusError::EquivocationDetected(ValidatorId)`.
- **Enforcement (Phase 9):** Cryptographic proof is submitted to the validator staking registry, triggering:
  1. Instant removal from the active committee (`Jailed`).
  2. $10\%$ permanent stake burning.
  3. Extended 30-day unbonding freeze.

---

## 7. Pacemaker View Synchronization

When a leader goes offline or fails to collect a QC within the view duration, the **Pacemaker** advances the round:
- **Base Round Timeout:** $\tau_{\text{base}} = 2000\text{ ms}$ (2.0s).
- **Exponential Backoff:** $\tau(R) = \min(\tau_{\text{base}} \cdot 2^{\min(R, 4)}, 30000)\text{ ms}$.
- Round step-up elects a new deterministic leader:
  $$\text{Leader}(H, R) = \text{Sample}(\text{BLAKE3}(b\text{"TOLA-LEADER-ELECTION"} \parallel H \parallel R))$$
Ensures network progress resumes automatically after leader failure without operator intervention.
