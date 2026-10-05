# Research 11: Account & Transaction Model Validation

## 1. Executive Summary & Challenge of Phase 0 Assumptions
This paper formalizes the canonical, byte-level data layout for TOLA accounts, transactions, receipts, and address encodings.

**Core Findings:**
1. **Account Model:** An account-based state model is required for expressive smart contract execution and balance lookups. UTXO models add unnecessary transaction complexity for high-frequency social micro-transactions.
2. **Anti-Replay Architecture:**
   - 64-bit monotonically increasing account `nonce`.
   - Explicit 64-bit `chain_id` in signature hash pre-image.
   - Optional `valid_until_block` expiration height to prune pending transactions from the mempool and prevent indefinite transaction hanging.
3. **Transaction Serialization:** Adopt **Borsh (Binary Object Representation Serializer for Hashing)** for consensus-critical internal node envelopes, and standard RLP/EIP-2718 envelopes for external EVM JSON-RPC compatibility.

---

## 2. Canonical Account State Layout

Each account in the global state is represented as a structured leaf in the Jellyfish Merkle Tree:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountState {
    pub nonce: u64,                     // 8 Bytes: Monotonically increasing
    pub balance: u128,                   // 16 Bytes: In nano-tola (10^-18 TOLA)
    pub code_hash: [u8; 32],            // 32 Bytes: Hash of contract bytecode (Empty for EOA)
    pub storage_root: [u8; 32],         // 32 Bytes: Root of contract JMT sub-tree
    pub flags: u32,                     // 4 Bytes: Bitmask (EOA, Contract, Validator, System)
}
```
**Total Leaf Size:** 92 Bytes (Compact, high cache density).

---

## 3. Canonical Transaction Envelope Layout

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionEnvelope {
    pub chain_id: u64,                  // 8 Bytes: Network ID
    pub nonce: u64,                     // 8 Bytes: Account nonce
    pub from: Address,                  // 20 Bytes: Sender address
    pub to: Option<Address>,            // 21 Bytes: Recipient address (None for contract creation)
    pub value: u128,                    // 16 Bytes: Value transferred in nano-tola
    pub gas_limit: u64,                 // 8 Bytes: Computational gas limit
    pub max_fee_per_gas: u128,          // 16 Bytes: Max total fee (Base + Tip)
    pub max_priority_fee_per_gas: u128, // 16 Bytes: Tip paid directly to proposer
    pub valid_until_block: u64,         // 8 Bytes: Expiration height (0 = No expiration)
    pub data: Vec<u8>,                  // Variable: Calldata or init bytecode
    pub signature: SignaturePayload,    // 65 Bytes: Secp256k1 (r, s, v) or Ed25519 (64 bytes)
}
```

---

## 4. Address Encoding Specification: Bech32m

Addresses are 20-byte identifiers derived from the public key:

$$\text{AddressBytes} = \text{Keccak256}(\text{PublicKey}_{\text{uncompressed}}[1..65])[12..32]$$

For Ed25519 public keys:
$$\text{AddressBytes} = \text{BLAKE3}(\text{PublicKey}_{\text{ed25519}})[0..20]$$

Encoded via **Bech32m (BIP-350)** with human-readable prefixes:
- Mainnet: `tola1<bech32m-payload>`
- Testnet: `ttola1<bech32m-payload>`
- Devnet: `dtola1<bech32m-payload>`

---

## 5. Architectural Decision Recommendation
**ACCEPT ADR-0006**: Account model, Bech32m addresses, and EIP-1559 transaction semantics validated without modifications.
