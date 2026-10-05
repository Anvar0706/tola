# TOLA Network: Consensus State Machine Specification v0.1

## 1. Pipelined BFT State Machine (Jolteon/HotStuff Paradigm)

Consensus operates across discrete Views $V = (H, R)$ governed by an asynchronous Pacemaker:

```
                  ┌──────────────────────────────┐
                  │          NEW_VIEW            │
                  │ (Enter View V = (Height, R)) │
                  └──────────────┬───────────────┘
                                 │
             ┌───────────────────┴───────────────────┐
             ▼ (If Node is Leader)                   ▼ (If Node is Validator)
   ┌───────────────────┐                   ┌───────────────────┐
   │   PROPOSE_BLOCK   │                   │    WAIT_PROPOSAL  │
   │ (Package Txs, QC) │                   │  (Arm Pacemaker)  │
   └─────────┬─────────┘                   └─────────┬─────────┘
             │ Broadcast Proposal                    │ Proposal Received
             └───────────────────┬───────────────────┘
                                 ▼
                       ┌───────────────────┐
                       │   VALIDATE_BLOCK  │
                       │ & CHECK LOCK RULE │
                       └─────────┬─────────┘
                                 │ Valid
                                 ▼
                       ┌───────────────────┐
                       │     SEND_VOTE     │
                       │ (Send to Leader)  │
                       └─────────┬─────────┘
                                 │
             ┌───────────────────┴───────────────────┐
             ▼ (Leader Collects Quorum)              ▼ (Timeout Fires)
   ┌───────────────────┐                   ┌───────────────────┐
   │    FORM_QC(V)     │                   │   VIEW_TIMEOUT    │
   │ (>= 2/3 + 1 Stake)│                   │(Broadcast Timeout)│
   └─────────┬─────────┘                   └─────────┬─────────┘
             │                                       │
             ▼                                       ▼
   Advance to View V+1                     Advance to Round R+1
   (Pipeline Block H+1)                    (Elect Next Leader)
```

---

## 2. Commit & Finality Rules

### The Two-Chain Commit Rule:
In Pipelined BFT, finality is achieved when two consecutive Quorum Certificates form a direct chain:
- Let $B_1$ be a proposed block at height $H$ with round $R_1$.
- Let $B_2$ be a proposed block at height $H+1$ with round $R_2$ whose parent is $B_1$, and $B_2$ incorporates $QC(B_1)$.
- If $B_2$ achieves a Quorum Certificate $QC(B_2)$ such that $R_2 == R_1 + 1$:
  $$\text{Commit}(B_1) \implies \text{Block } B_1 \text{ is Irrevocably Finalized}$$

---

## 3. Formal Safety & Liveness Theorems

### Theorem 1 (Consensus Safety):
Under the partial synchrony model with total validator voting power $N$, if Byzantine voting power $f < N/3$, no two honest nodes can commit conflicting blocks at the same height.
- *Proof Sketch:* A block commit requires $\ge 2f + 1$ votes. Any two quorums $Q_1, Q_2$ intersect by at least $2(2f+1) - (3f+1) = f + 1$ validators. Since at most $f$ validators are Byzantine, at least one honest validator is in $Q_1 \cap Q_2$. An honest validator never votes for conflicting proposals in the same round due to the monotonic locking rule. Thus, conflicting commits cannot coexist.

### Theorem 2 (Consensus Liveness):
After Global Stabilization Time (GST), if message delivery latency is bounded by $\Delta$ and the Pacemaker timeout is set to $\tau > 2\Delta$, honest validators will elect an honest leader and finalize blocks continuously.

---

## 4. Leader Election & Pacemaker Schedule

1. **Deterministic Weighted Round-Robin:**
   $$\text{Leader}(H, R) = \text{ValidatorSet}.\text{SelectProposer}(\text{Seed} = \text{BLAKE3}(H \parallel R))$$
2. **Pacemaker Timeout Exponential Backoff:**
   $$\tau(R) = \tau_{\text{base}} \times 1.5^{\min(R, 8)}$$
   Base timeout $\tau_{\text{base}} = 2.0\text{ seconds}$.
