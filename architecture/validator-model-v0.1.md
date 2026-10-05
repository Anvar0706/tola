# TOLA Network: Validator Sizing & Committee Model v0.1

## 1. Rigorous Validator Set Sizing Analysis

| Validator Set Size ($N$) | Message Complexity per Block | Network Bandwidth per Leader | Signature Verification Latency (BLS12-381) | Consensus Cadence | Architectural Feasibility |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **$N = 50$** | 100 messages | $\sim 0.2\text{ MB/s}$ | $\sim 1.2\text{ ms}$ | $1.5 - 2.0\text{ s}$ | High, but lower decentralization |
| **$N = 100$** | 200 messages | $\sim 0.5\text{ MB/s}$ | $\sim 2.4\text{ ms}$ | $2.0\text{ s}$ | **Optimal Genesis Target** |
| **$N = 150$** | 300 messages | $\sim 0.8\text{ MB/s}$ | $\sim 3.6\text{ ms}$ | $2.0 - 2.5\text{ s}$ | **Recommended Mainnet Capacity** |
| **$N = 300$** | 600 messages | $\sim 1.8\text{ MB/s}$ | $\sim 7.2\text{ ms}$ | $2.5 - 3.0\text{ s}$ | Feasible with aggregation trees |
| **$N = 500$** | 1,000 messages | $\sim 3.2\text{ MB/s}$ | $\sim 12.0\text{ ms}$ | $3.5 - 4.5\text{ s}$ | Requires tree aggregation |
| **$N = 1,000$** | 2,000 messages | $\sim 7.5\text{ MB/s}$ | $\sim 25.0\text{ ms}$ | $5.0 - 7.0\text{ s}$ | High latency; requires VRF committees |

---

## 2. The Two-Tier Validator Architecture

To maximize decentralization while preserving sub-3-second BFT consensus cadence, TOLA Network implements a **Two-Tier Validator Structure**:

```
[Tier 1: Active Consensus Committee (Top 100 - 150 by Stake)]
- Participates in real-time block proposal, BFT voting, and QC signing.
- Rotated dynamically every Epoch (3,600 blocks / ~2 hours).
- High hardware requirements (8+ vCPU, 32 GB RAM, NVMe).
                 │
                 ▼ Delegation & Slashing
[Tier 2: Standby & Delegator Network (Unlimited Participants)]
- Stake delegated to Tier 1 validators.
- Earns pro-rata staking rewards minus validator commission.
- Zero P2P consensus voting overhead on the network core.
```

---

## 3. Remote Signer & Anti-Equivocation Architecture

```
TOLA Validator Node (Public Network)          TOLA Remote Signer (Isolated Airgap)
           │                                                   │
           ├── 1. Request Signature (Height, Round, Hash) ────►│
           │   (mTLS over local Unix Socket / Private IP)      ├── 2. Check Local Guard:
           │                                                   │      Height > HighestSignedHeight?
           │                                                   │      - Yes: Record to Guard DB
           │                                                   │      - No: HARD REJECT (Panic)
           │◄── 3. Return Ed25519/BLS Signature ───────────────┤
```
