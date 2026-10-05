# Research 14: Economic Model & Incentive Engineering Analysis

## 1. Executive Summary & Separation of Powers
This research paper models the economic foundation of TOLA Network, establishes mathematical supply conservation invariants, and rigorously separates **Technical Protocol Decisions** from **Economic / Product Decisions** requiring governance or founder sign-off.

**Core Principles:**
1. **Security Depends on Capital-at-Risk:** A Proof-of-Stake network's Byzantine resistance is proportional to the economic cost of acquiring $\ge 1/3 + 1$ of active bonded voting power.
2. **Anti-Spam Requires Real Economic Cost:** Free or zero-fee transactions guarantee catastrophic spam and disk bloat. Gas fees must impose real economic penalties on attackers while maintaining sub-cent costs for genuine social micro-transactions.
3. **Disinflationary Emission + Burning:** A predictable, decaying emission curve paired with 100% burning of base transaction fees balances validator rewards against long-term purchasing power stability.

---

## 2. Technical vs Economic Decision Boundaries

```
┌─────────────────────────────────────────────────────────────┐
│ TECHNICAL PROTOCOL DECISIONS (Engineering Autonomy)         │
│ - Exact arithmetic precision: 18 decimals (nano-tola)       │
│ - EIP-1559 dynamic base fee algorithm and elasticity bounds │
│ - Mathematical supply conservation invariant checks         │
│ - Slashing verification and double-signing evidence proofs  │
│ - Unbonding lock queue state machines                       │
└─────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────▼───────────────────────────────┐
│ ECONOMIC / PRODUCT DECISIONS (Requires Founder / Governance)│
│ - Genesis Total Supply (e.g. 1,000,000,000 TOLA)            │
│ - Genesis Token Allocation & Vesting Schedules              │
│ - Initial Annual Inflation Rate (e.g. 5.0% vs 7.0%)         │
│ - Slashing Penalty Percentage (e.g. 5% vs 20% burnt)        │
│ - Community Treasury Allocation Share (e.g. 10% vs 20%)     │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. Mathematical Supply Conservation & Inflation Dynamics

### Supply Conservation Invariant:
At every block height $H$:
$$\sum \text{AccountBalances} + \text{TotalBondedStake} + \text{TotalUnbonding} + \text{TotalBurned} = \text{GenesisSupply} + \sum_{i=1}^H \text{BlockReward}_i$$

### Disinflationary Emission Schedule:
Let $I_0$ be the initial annual inflation rate (provisional target: $5.0\%$).
Let $\lambda$ be the annual decay constant ($\lambda = 0.90$, a $10\%$ annual reduction in emission).
Let $I_{\text{floor}}$ be the terminal inflation floor (provisional target: $1.5\%$).

The annual inflation rate at year $t$ is modeled as:
$$I(t) = \max\left(I_0 \cdot \lambda^t, \, I_{\text{floor}}\right)$$

### Net Issuance with EIP-1559 Fee Burning:
$$\Delta \text{Supply}_{\text{net}} = \text{StakingEmission} - \text{BaseFeesBurned}$$
When daily network transaction volume reaches high utilization ($> 2,000\text{ TPS}$), $\text{BaseFeesBurned} > \text{StakingEmission}$, rendering TOLA a mathematically deflationary asset.

---

## 4. Slashing Economics & Byzantine Deterrence

1. **Equivocation (Double Signing):**
   - **Technical Rule:** Instant revocation of consensus voting rights (permanent jailing) + burn $10\%$ of validator self-stake.
   - **Economic Justification:** Punishes malicious collusion without destroying innocent delegator pools completely in the event of accidental operational misconfigurations.
2. **Liveness Faults (Downtime):**
   - **Technical Rule:** If a validator misses $> 500$ blocks out of a sliding window of $10,000$ blocks, the validator is temporarily jailed for $24$ hours. Zero stake is burned; validator earns zero rewards until unjailed.

---

## 5. Architectural Decision Recommendation
Formalize the mathematical conservation invariants and EIP-1559 base fee elasticity rules in the protocol core, while leaving Genesis allocation ratios and initial inflation percentages parameterizable via `genesis.json`.
