# TOLA Network: Native Asset Economics & Supply Schedule

## 1. Executive Summary

The native cryptographic asset of TOLA Network is designated by the ticker **TOLA**.
TOLA serves three fundamental functions within the protocol:
1. **Network Consensus & Economic Security**: Bonded by validators and delegators in the Proof-of-Stake consensus engine to safeguard against Byzantine actions.
2. **Computational Fuel (Gas)**: Measures and prices state transitions, memory execution, and persistent storage access, preventing denial-of-service vectors.
3. **Medium of Exchange & Micropayments**: High-speed, low-friction settlement currency underpinning the decoupled messenger ecosystem and decentralized applications.

---

## 2. Denomination & Base Units

TOLA utilizes 18 decimal places of precision ($10^{18}$), aligning with standard Web3 numerical models:

$$\text{1 TOLA} = 10^{18} \text{ nano-tola / tol}$$

$$\text{1 Gwei-tola} = 10^9 \text{ nano-tola / tol}$$

$$\text{1 nano-tola (base unit)} = 1 \text{ tol}$$

All protocol calculations, state storage, and serialization formats operate strictly on integer base units (`u128`), eliminating floating-point rounding errors.

---

## 3. Genesis Supply & Initial Allocation

The canonical Mainnet Genesis specification (`genesis-mainnet.json`, Chain ID `9999`) establishes a fixed initial supply of **1,000,000,000 TOLA** ($10^{27}$ base units):

| Allocation Category | Percentage | Amount (TOLA) | Purpose / Vesting |
| :--- | :---: | :---: | :--- |
| **Foundation Reserve** | 30.0% | 300,000,000 | Long-term protocol R&D, security bounties, multi-year lockups |
| **Ecosystem Treasury & Grants** | 25.0% | 250,000,000 | Developer developer tooling, dApp incentives, liquidity bootstrapping |
| **Initial Validator Staking Pool** | 20.0% | 200,000,000 | Genesis validator set (4 nodes $\times$ 50M TOLA bonded) |
| **Community Public Distribution** | 15.0% | 150,000,000 | Airdrops, liquidity pools, community onboarding |
| **Early Core Contributors** | 10.0% | 100,000,000 | Core engineering team with 48-month linear vesting |
| **Total Genesis Supply** | **100.0%** | **1,000,000,000** | Initial circulating & bonded cap |

---

## 4. Staking Emission Schedule (Disinflationary Curve)

Block production rewards are newly minted according to a mathematically predictable disinflationary schedule:

- **Initial Annual Inflation Rate**: $5.0\%$ ($500 \text{ bps}$) of Genesis Supply.
- **Annual Decay Rate**: $-0.5\%$ ($-50 \text{ bps}$) per year.
- **Terminal Inflation Floor**: $1.5\%$ ($150 \text{ bps}$), reached at Year 7 and preserved perpetually.
- **Block Time**: $2.0 \text{ seconds}$ nominal ($\approx 15,778,800 \text{ blocks/year}$).

### Mathematical Formulation

For any block height $H$:
$$\text{Year}(H) = \left\lfloor \frac{H}{\text{BlocksPerYear}} \right\rfloor$$

$$\text{InflationRate}(H) = \max\left( \text{InitialRate} - \text{Year}(H) \times \text{DecayRate}, \text{TerminalFloor} \right)$$

$$\text{BlockReward}(H) = \frac{\text{GenesisSupply} \times \text{InflationRate}(H)}{10,000 \times \text{BlocksPerYear}}$$

### Reward Distribution Split

Each newly minted block reward is split deterministically at the protocol level:
1. **Validator & Delegator Staking Pool (90%)**: Credited to the active consensus committee proportional to voting weight and validator commission.
2. **On-Chain Community Treasury (10%)**: Directed into a timelocked treasury reserve managed strictly by community governance.

---

## 5. Deflationary Fee Burn Mechanism (EIP-1559 Style)

To counteract staking emissions and align token velocity with network utility, TOLA Network implements a mandatory base fee burn:

$$\text{Total Fee} = \text{GasUsed} \times (\text{BaseFee} + \text{PriorityFee})$$

- **100% of the Base Fee** is permanently destroyed (burned) from existence.
- **Priority Fee (Tip)** is awarded to the block proposer.

During periods of high transaction throughput, the net block burn rate $\text{BurnRate}(H)$ can exceed the gross block emission $\text{BlockReward}(H)$, rendering TOLA net-deflationary.

---

## 6. Formal Supply Conservation Invariant

The protocol enforces the following mathematical invariant across every valid state transition:

$$\sum_{i} \text{Balance}_i + \text{TotalStaked} + \text{TreasuryBalance} + \text{TotalBurned} = \text{GenesisSupply} + \sum_{h=1}^H \text{MintedReward}_h$$

Any state transition violating this identity halts the node immediately, preventing unauthorized token generation or silent overflow leaks.
