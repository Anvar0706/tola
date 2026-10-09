# TOLA Network: On-Chain Governance & Parameter Management

## 1. Overview

TOLA Network incorporates an on-chain, decentralized governance engine (`GovernanceEngine`) that enables protocol evolution, economic parameter tuning, and risk adjustments without requiring disruptive hard forks or off-chain backroom coordination.

---

## 2. Proposal Lifecycle

Every protocol modification progresses through a rigorous 5-stage state machine:

```
[ Deposit Phase ] 
       │ (Deposit >= 10,000 TOLA)
       ▼
[ Active Voting Phase (7 Days) ]
       │
   ┌───┴───────────────────────────────┐
   │                                   │
(Quorum < 40% OR                   (Quorum >= 40% AND
 No/Veto > 50%)                     Yes > 50% AND Veto < 33.4%)
   │                                   │
   ▼                                   ▼
[ Rejected / Vetoed ]            [ Timelocked Phase (48 Hours) ]
                                       │
                                       │ (Timelock expires)
                                       ▼
                                 [ Executed ]
```

### Stage 1: Proposal Submission & Deposit
- Any account can submit a proposal with a structured parameter change payload.
- Minimum Deposit: **10,000 TOLA**. Multiple community members can contribute deposits.
- If the deposit threshold is not achieved within 14 days, the proposal expires and deposits are refunded.

### Stage 2: Active Voting Window
- Duration: **7 days** ($\approx 302,400 \text{ blocks}$ at $2.0 \text{s}$ block time).
- Voting Power: Derived strictly from bonded stake snapshot at the moment voting activates (`start_block`).
- **Flash-Loan Immunity**: Flash-borrowed tokens or unstaked tokens cannot participate.

### Stage 3: Tally & Threshold Calculation
1. **Quorum Requirement**: At least **40.0%** of total network bonded stake must participate (`Yes` + `No` + `NoWithVeto` + `Abstain`).
2. **Veto Gate**: If `NoWithVeto` votes reach or exceed **33.4%** of participating stake, the proposal is rejected as malicious or spam, and **100% of the deposit is permanently burned**.
3. **Approval Threshold**: Excluding abstentions, more than **50.0%** of participating stake must vote `Yes`.

### Stage 4: Mandatory Timelock
- Approved proposals enter a mandatory **48-hour timelock** ($\approx 86,400 \text{ blocks}$).
- Provides a protective buffer for dissenting users and validators to review parameters or safely unbond if desired.

### Stage 5: Execution
- Upon timelock expiration, the transaction executes automatically at block transition, atomically applying the new parameter values to `ActiveProtocolParams`.
- The initial deposit is fully refunded to the proposer.

---

## 3. Modifiable Protocol Parameters

| Parameter | Type | Default Mainnet Value | Description |
| :--- | :---: | :---: | :--- |
| `max_block_gas_limit` | `Gas` | `30,000,000` | Upper limit of computational gas consumed per block |
| `target_base_fee` | `Amount` | `1,000,000,000` ($1 \text{ Gwei}$) | EIP-1559 base fee equilibrium target |
| `min_validator_self_stake` | `Amount` | `100,000 TOLA` | Minimum bonded stake required for validator registration |
| `unbonding_period_blocks` | `u64` | `907,200` ($21 \text{ days}$) | Staking unstaking queue delay protecting weak subjectivity |
| `slashing_penalty_bp` | `u16` | `1,000` ($10.0\%$) | Equivocation / double-sign stake burn penalty |
