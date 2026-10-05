# Research 21: Validator Scale Analysis & Committee Sizing

## 1. Mathematical Scaling Model for Pipelined BFT
Let $N$ be the active validator set size. Under Pipelined HotStuff with BLS12-381 signature aggregation:
- Each validator signs a 96-byte vote and sends it to the Leader.
- Leader bandwidth: $N \times 96\text{ bytes}$ inbound per round.
- Quorum verification: Leader aggregates $N$ signatures via public key addition and verifies single pairing: $O(N)$ addition $+ O(1)$ pairing.

---

## 2. Comparative Sizing Matrix

| Validator Count ($N$) | Leader Inbound Bandwidth per Round | Leader CPU Time (Vote Aggregation) | Quorum Certificate Size | View-Change Latency ($\tau$) | Recommendation |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **50** | $4.8\text{ KB}$ | $0.4\text{ ms}$ | 96 Bytes | $1.5\text{ s}$ | High speed; low decentralization |
| **100** | $9.6\text{ KB}$ | $0.8\text{ ms}$ | 96 Bytes | $2.0\text{ s}$ | **Optimal Genesis Target** |
| **150** | $14.4\text{ KB}$ | $1.2\text{ ms}$ | 96 Bytes | $2.0\text{ s}$ | **Target Mainnet Committee** |
| **300** | $28.8\text{ KB}$ | $2.4\text{ ms}$ | 96 Bytes | $2.8\text{ s}$ | Viable with high-bandwidth leaders |
| **500** | $48.0\text{ KB}$ | $4.2\text{ ms}$ | 96 Bytes | $3.5\text{ s}$ | Approaching global latency limits |
| **1,000** | $96.0\text{ KB}$ | $8.8\text{ ms}$ | 96 Bytes | $5.0\text{ s}$ | Excessive view-change delay |

---

## 3. Final Committee Policy
1. **Mainnet Active Set:** Standardize on an active committee of **$100 - 150$ validators**.
2. **Standby Delegation:** Unlimited delegators delegate stake to active validators.
3. **Epoch Rotation:** Dynamic committee re-selection every 3,600 blocks based on top bonded stake.
