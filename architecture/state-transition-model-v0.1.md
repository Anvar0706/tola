# TOLA Network: State Transition Model & Invariants v0.1

## 1. Formal Mathematical Specification

The state of TOLA Network is modeled as a key-value mapping $S: \text{Key} \to \text{Value}$ where keys represent 20-byte account addresses and contract storage slots.

### 1.1 Single Transaction Transition Function
$$\text{Transition}(S, T) \to (S', R)$$

Given state $S$ and transaction $T$:
1. **Stateless Verification:**
   - $\text{VerifySig}(T.\text{from}, \text{SigningDigest}(T), T.\text{sig}) == \text{True}$.
   - $T.\text{chain\_id} == \text{CurrentChainId}$.
   - $T.\text{gas\_limit} \ge 21,000 \land T.\text{gas\_limit} \le \text{BlockGasLimit}$.
2. **Stateful Verification:**
   - $T.\text{nonce} == S[T.\text{from}].\text{nonce}$.
   - $S[T.\text{from}].\text{balance} \ge (T.\text{gas\_limit} \times T.\text{max\_fee\_per\_gas}) + T.\text{value}$.
3. **Execution & Upfront Gas Charge:**
   - $S_1 = S$.
   - $S_1[T.\text{from}].\text{nonce} = S[T.\text{from}].\text{nonce} + 1$.
   - $S_1[T.\text{from}].\text{balance} -= (T.\text{gas\_limit} \times T.\text{max\_fee\_per\_gas})$.
4. **VM Invocation:**
   - $(S_2, \text{GasUsed}, \text{Logs}, \text{Success}) = \text{VMRun}(S_1, T)$.
5. **Fee Reconciliation & Refund:**
   - $\text{EffectiveGasPrice} = \text{BaseFee} + \min(T.\text{max\_priority\_fee}, T.\text{max\_fee} - \text{BaseFee})$.
   - $\text{RefundAmount} = (T.\text{gas\_limit} - \text{GasUsed}) \times T.\text{max\_fee\_per\_gas} + \text{GasUsed} \times (T.\text{max\_fee\_per\_gas} - \text{EffectiveGasPrice})$.
   - $S'[T.\text{from}].\text{balance} += \text{RefundAmount}$.
   - $S'[\text{Proposer}].\text{balance} += \text{GasUsed} \times \text{PriorityFee}$.
   - $\text{BurnedTotal} += \text{GasUsed} \times \text{BaseFee}$.

### 1.2 Block Execution Function
$$\text{Execute}(B, S) \to (S_{\text{final}}, \vec{R}, \text{StateRoot})$$

For an ordered block of transactions $B.\text{transactions} = [T_0, T_1, \dots, T_{n-1}]$:
$$S_0 = S$$
$$S_{i+1} = \text{Transition}(S_i, T_i) \quad \forall i \in [0, n-1]$$
$$\text{StateRoot} = \text{JellyfishMerkleTree::Root}(S_n)$$

---

## 2. Definitive Protocol Invariants

1. **Total Supply Conservation Invariant:**
   $$\sum_{A \in \text{Accounts}} S_H[A].\text{balance} + \text{TotalStaked}_H + \text{TotalBurned}_H = \text{GenesisSupply} + \sum_{h=1}^H \text{BlockReward}_h$$
2. **Balance Non-Negativity Invariant:**
   $$\forall A, \quad S[A].\text{balance} \ge 0$$
3. **Nonce Monotonicity Invariant:**
   If transaction $T$ from sender $A$ commits at height $H$, then $S_H[A].\text{nonce} > S_{H-1}[A].\text{nonce}$.
4. **Deterministic Equivalence Invariant:**
   For any execution engine (sequential or Block-STM parallel):
   $$\text{BlockSTM\_Execute}(B, S) \equiv \text{Sequential\_Execute}(B, S)$$
5. **State Root Uniqueness:**
   For any two identical states $S_A$ and $S_B$, $\text{JMT::Root}(S_A) == \text{JMT::Root}(S_B)$.

---

## 3. Property Testing Strategy

Property-based testing is implemented via the `proptest` framework:
- **Random Transfer Invariant:** Generate arbitrary graphs of 10,000 accounts with random transfers; verify sum of balances is identical before and after block.
- **Nonce Sequence Invariant:** Attempt out-of-order nonces; assert 100% rejection.
- **Differential Execution Invariant:** Run identical random blocks through single-threaded EVM and multi-threaded Block-STM; assert exact byte-for-byte state root equality.
