# TOLA Network: Protocol Specification v0.1

## 1. Protocol Identity & Genesis Constants
- **Network Name:** TOLA Network
- **Native Denomination:** TOLA ($1\text{ TOLA} = 10^{18}\text{ nano-tola}$)
- **Chain Identifiers:**
  - Mainnet: `9999` (`0x270F`)
  - Testnet: `9998` (`0x270E`)
  - Devnet: `9997` (`0x270D`)
- **Block Time Target:** $2.0\text{ seconds}$
- **Target Gas per Block:** $30,000,000\text{ gas}$ (Elastic limit: $60,000,000\text{ gas}$)
- **Finality Depth:** 2 Blocks (Deterministic BFT Commit Certificate)

---

## 2. Global State Transition Function

The canonical state of TOLA Network progresses deterministically via discrete blocks:
$$S_{H} = \text{ApplyBlock}(S_{H-1}, B_H)$$

Where $B_H$ is a valid block containing:
$$B_H = \langle \text{Header}_H, \text{Transactions}_H, \text{CommitCertificate}_{H-1} \rangle$$

### State Transition Invariants:
1. **Supply Conservation:** No currency can be created or destroyed outside authorized block reward issuance and EIP-1559 base fee burning.
2. **Strict Nonce Monotonicity:** A transaction from account $A$ is valid if and only if $\text{tx.nonce} == S[A].\text{nonce}$. Upon execution, $S'[A].\text{nonce} = S[A].\text{nonce} + 1$.
3. **Deterministic Gas Consumption:** For identical state $S$ and transaction $T$, $\text{GasUsed}(S, T)$ evaluates to the identical integer on all compliant node implementations.

---

## 3. Subsystem Lifecycle & Inter-Layer Contracts

```
Network Wire (P2P GossipSub)
           │ Raw Block / Tx Bytes
           ▼
[Layer 0: Networking] ──► Deserialization & Noise Validation
           │
           ▼
[Layer 1: Consensus Engine] ──► Pipelined BFT Leader Proposal & Voting
           │ (Orders Block H)
           ▼
[Layer 2: Execution Engine] ──► Block-STM Parallel EVM State Transition
           │ (Computes State Root & Receipts)
           ▼
[Storage Engine (MDBX)] ──► Atomic WriteBatch Commit to Local Disk
           │
           ▼
[Layer 3: RPC Engine] ──► Emits Logs to WebSocket / JSON-RPC Clients
```
