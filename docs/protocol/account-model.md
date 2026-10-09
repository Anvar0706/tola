# TOLA Network: Account Model & Invariant Specification

## 1. Overview & Architectural Policy
TOLA Network implements an explicit **Account-Based State Model** (analogous to Ethereum and Cosmos, distinct from UTXO models). Accounts represent either Externally Owned Accounts (EOAs) controlled by cryptographic private keys, or Smart Contracts controlled by immutable EVM/WASM bytecode and internal storage tries.

Every active account is represented on-chain as a compact, fixed-size 92-byte leaf serialized into the Jellyfish Merkle Tree (JMT) and persisted in the MDBX state database.

---

## 2. Canonical Account State Schema

```rust
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AccountState {
    pub nonce: Nonce,           // 8 bytes (u64 LE)
    pub balance: Amount,         // 16 bytes (u128 LE, 18 decimal places)
    pub code_hash: Hash,         // 32 bytes (BLAKE3/Keccak digest)
    pub storage_root: Hash,      // 32 bytes (JMT storage trie root)
    pub flags: u32,              // 4 bytes (u32 LE bitfield)
}
```

### 2.1 Field Definitions & Semantics
1. **`nonce` (8 Bytes):**
   - Strictly monotonic sequential integer starting at `0`.
   - Increments by exactly `1` for every committed transaction originated by this account.
   - For contracts, tracks contract-initiated actions and creates deterministic deployment addresses via `CREATE` / `CREATE2`.
2. **`balance` (16 Bytes):**
   - Denominated in base units ($1 \text{ TOLA} = 10^{18} \text{ attotola}$).
   - Backed by checked unsigned 128-bit arithmetic (`u128`) capable of representing up to $3.4 \times 10^{38}$ base units without risk of overflow.
3. **`code_hash` (32 Bytes):**
   - For EOAs: strictly `Hash::ZERO` (`[0u8; 32]`).
   - For smart contracts: BLAKE3/Keccak digest of the deployed runtime bytecode.
4. **`storage_root` (32 Bytes):**
   - For EOAs: strictly `Hash::ZERO`.
   - For smart contracts: cryptographic root of the account's internal JMT storage slot mapping.
5. **`flags` (4 Bytes):**
   - Protocol-level bitfield encoding account permissions and attributes:
     - `FLAG_NONE (0x0000_0000)`: Standard EOA.
     - `FLAG_CONTRACT (0x0000_0001)`: Account contains executable contract code.
     - `FLAG_FROZEN (0x0000_0002)`: Account temporarily restricted by governance or security circuit breakers.
     - `FLAG_VALIDATOR (0x0000_0004)`: Account registered in the active validator registry.

---

## 3. Mathematical State Invariants

### 3.1 Balance Non-Negativity Invariant
$$\forall A \in \text{Accounts}, \quad S[A].\text{balance} \ge 0$$
No account balance can underflow. In `tola-types::account`, any debit operation that would reduce balance below zero immediately halts with `AccountError::InsufficientBalance`.

### 3.2 Nonce Monotonicity Invariant
$$\text{Nonce}_{t+1}(A) > \text{Nonce}_t(A)$$
Nonces never regress or repeat. Transactions submitting nonces non-contiguous with the current account nonce are rejected at the stateful validation barrier.

### 3.3 Upfront Affordability Invariant
Prior to transaction execution, the sender must possess sufficient balance to satisfy the maximum upfront fee reservation and transferred value:
$$\text{Balance}(A) \ge (\text{GasLimit} \times \text{MaxFeePerGas}) + \text{Value}$$

---

## 4. Account Lifecycle Transitions

```
                    ┌────────────────────────┐
                    │  Non-Existent Account  │
                    │      (Balance = 0)     │
                    └───────────┬────────────┘
                                │ Credit Value > 0
                                ▼
                    ┌────────────────────────┐
                    │       Active EOA       │
                    │   (Nonce >= 0, Bal > 0)│
                    └───────────┬────────────┘
                                │ Deploy Contract
                                ▼
                    ┌────────────────────────┐
                    │ Smart Contract Account │
                    │ (FLAG_CONTRACT set,    │
                    │  code_hash != ZERO)    │
                    └────────────────────────┘
```
