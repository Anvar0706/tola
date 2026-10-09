# TOLA Network: Validator Registry, Staking & Automated Slashing Specification

## 1. Overview & Architectural Motivation
The **TOLA Staking & Slashing Subsystem** (`tola-consensus::registry` & `tola-consensus::slashing`) establishes the economic security and Byzantine accountability layer for TOLA Network, conforming to **ADR-0009**, **research/06-consensus-validation.md**, and **prompts/13-consensus.md**.

Proof-of-Stake protocols require strong economic guarantees to prevent Sybil attacks, long-range equivocation, and non-deterministic committee selection. In TOLA:
1. **Sybil Resistance via Bonded Capital:** Only validators meeting a minimum self-stake threshold can participate in consensus proposals and vote aggregation.
2. **$O(1)$ Share-Based Pool Accounting:** Staking delegations and slashing penalties are calculated using a share-based pool model, avoiding expensive $O(N)$ loops over delegators when rewards accrue or slashes are enacted.
3. **Automated Byzantine Slashing:** Double-voting (equivocation) is cryptographically proven via dual signed vote envelopes, triggering an immediate 10% stake burn and 30-day jailing.
4. **Weak Subjectivity Mitigation:** A 21-day unbonding delay locks withdrawn funds, ensuring dishonest validators cannot unbond before equivocation evidence is admitted to the ledger.

---

## 2. Validator State Machine & Lifecycle

Every validator node transitions through a deterministic state machine:

```
                  +---------------------------+
                  |        Candidate          | <-------+
                  +---------------------------+         |
                     |                     ^            |
      Stake Ranked   |                     | Fall out   | Unjail
      in Top K       v                     | of Top K   | (Cooldown over &
                  +---------------------------+         |  Self-stake >= Min)
                  |          Active           |         |
                  +---------------------------+         |
                     |         |                        |
        Unbonding /  |         | Equivocation           |
     Below Min Stake |         | Slashed                |
                     v         v                        |
                  +---------------------------+         |
                  |         Inactive          |         |
                  +---------------------------+         |
                               |                        |
                               | Equivocation Slashed   |
                               v                        |
                  +---------------------------+         |
                  |          Jailed           | --------+
                  +---------------------------+
```

### 2.1 State Definitions
* **`Candidate`**: Registered validator with $\ge 100,000\text{ TOLA}$ self-stake awaiting epoch rotation.
* **`Active`**: Ranked within the top $K$ (default 100) validators by total bonded stake for the current epoch; actively participates in block proposal and QC formation.
* **`Inactive`**: Validator dropped below the top $K$ or voluntarily fell below self-stake requirements.
* **`Jailed`**: Penalized for Byzantine equivocation; barred from proposing, voting, and committee election for a minimum of $1,296,000\text{ blocks}$ (~30 days).

---

## 3. Share-Based Staking Pool Mathematics

To eliminate state bloat and unbounded iteration during rewards and penalties, each validator maintains a share-based staking pool.

Let:
* $B$ = Total bonded TOLA stake in pool
* $S$ = Total minted pool shares
* $A_{\text{dep}}$ = Amount deposited by delegator
* $S_{\text{del}}$ = Shares redeemed by delegator

### 3.1 Share Issuance on Delegation
When a delegator deposits tokens into a validator pool:
$$S_{\text{minted}} = \begin{cases} A_{\text{dep}} & \text{if } B = 0 \text{ or } S = 0 \\ \left\lfloor \frac{A_{\text{dep}} \cdot S}{B} \right\rfloor & \text{otherwise} \end{cases}$$

State transitions atomically:
$$B \leftarrow B + A_{\text{dep}}$$
$$S \leftarrow S + S_{\text{minted}}$$

### 3.2 Token Redemption on Undelegation
When a delegator redeems $S_{\text{redeem}}$ shares:
$$A_{\text{redeemed}} = \left\lfloor \frac{S_{\text{redeem}} \cdot B}{S} \right\rfloor$$

State transitions atomically:
$$S \leftarrow S - S_{\text{redeem}}$$
$$B \leftarrow B - A_{\text{redeemed}}$$

### 3.3 $O(1)$ Slashing Absorption
When a validator is slashed by fraction $p = 10\%$ ($1,000\text{ basis points}$):
$$B \leftarrow B - \left\lfloor \frac{B \cdot 1000}{10000} \right\rfloor$$
$$S \leftarrow S \quad (\text{remains strictly unchanged})$$

Because $S$ is invariant, the redemption value of **every single share** across all delegators immediately drops by exactly $10\%$ in $O(1)$ time complexity without modifying any delegator record.

---

## 4. Unbonding Lock Queue & Weak Subjectivity Protection

To protect light clients and bootstrapping nodes from weak subjectivity attacks:
* **Unbonding Delay:** $907,200\text{ blocks}$ (~21 days at 2.0s block target).
* **Queue Invariant:** Undelegated tokens do not immediately return to the delegator's liquid balance. Instead, an `UnbondingEntry` is enqueued:
  $$\text{CompletionHeight} = \text{CurrentHeight} + \text{UnbondingDelayBlocks}$$
* **Slashing Protection:** If the validator commits an equivocation while unbonding entries are pending, all pending unbonding entries are slashed by the same penalty fraction ($10\%$).
* **Maturity Claim:** Once $\text{BlockHeight} \ge \text{CompletionHeight}$, the delegator issues a claim transaction to withdraw liquid funds.

---

## 5. Automated Byzantine Slashing Engine

The `SlashingEngine` evaluates cryptographic evidence of double voting (`SlashingEvidence`):

$$\mathcal{E} = \{ H, R, V_A, V_B \}$$

### 5.1 Verification Invariants
Evidence is deemed valid if and only if all of the following conditions hold:
1. **Identical Validator:** $V_A.\text{validator} = V_B.\text{validator}$
2. **Matching View:** $V_A.H = V_B.H = \mathcal{E}.H \quad \land \quad V_A.R = V_B.R = \mathcal{E}.R$
3. **Identical Vote Phase:** $V_A.\text{vote\_type} = V_B.\text{vote\_type} \in \{\text{Prevote}, \text{Precommit}\}$
4. **Conflicting Candidates:** $V_A.\text{block\_hash} \ne V_B.\text{block\_hash}$
5. **Cryptographic Signatures:**
   $$\text{VerifyEd25519}(PK_{\text{val}}, \text{VoteMsg}(V_A), V_A.\text{sig}) = \text{Ok}$$
   $$\text{VerifyEd25519}(PK_{\text{val}}, \text{VoteMsg}(V_B), V_B.\text{sig}) = \text{Ok}$$
   where:
   $$\text{VoteMsg}(V) = \text{BLAKE3}(b\text{"TOLA-BFT-VOTE-v1--"} \parallel \text{type} \parallel H \parallel R \parallel \text{block\_hash})$$

### 5.2 Penalties Enacted
Upon proof verification, the engine emits a canonical `SlashReceipt` and executes:
1. **Stake Burn:** $10\%$ of `total_bonded_stake` is burned permanently.
2. **Unbonding Queue Slash:** All pending unbonding entries for this validator are slashed by $10\%$.
3. **Jailing:** Validator is marked `ValidatorStatus::Jailed` for $1,296,000\text{ blocks}$.
4. **Unjail Eligibility:** After $1,296,000\text{ blocks}$, the operator can call `unjail` only if remaining bonded stake still satisfies `min_self_stake \ge 100,000\text{ TOLA}`.
