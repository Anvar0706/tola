# TOLA Network: Transaction Model & Fee Economics Specification

## 1. Overview & Architecture
TOLA Network enforces a deterministic, high-throughput transaction model incorporating **EIP-1559 Dynamic Fee Elasticity**, **BLAKE3 canonical transaction IDs**, and **Secp256k1 recoverable ECDSA signatures**.

---

## 2. Canonical Transaction Envelope

```rust
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Transaction {
    pub chain_id: ChainId,                     // 8 bytes (u64 LE)
    pub nonce: Nonce,                          // 8 bytes (u64 LE)
    pub from: Address,                         // 20 bytes
    pub to: Option<Address>,                   // 1 byte tag + optional 20 bytes
    pub value: Amount,                         // 16 bytes (u128 LE)
    pub gas_limit: Gas,                        // 8 bytes (u64 LE)
    pub max_fee_per_gas: Amount,               // 16 bytes (u128 LE)
    pub max_priority_fee_per_gas: Amount,      // 16 bytes (u128 LE)
    pub valid_until_block: BlockHeight,        // 8 bytes (u64 LE)
    pub data: Vec<u8>,                         // 4 bytes len + payload (<= 128 KB)
    pub memo: Option<String>,                  // Optional UTF-8 string (<= 256 bytes)
    pub signature: Signature,                  // 1 byte tag + 65 bytes payload
    pub tx_type: u8,                           // 1 byte: 0 = Legacy, 1 = EIP-2930, 2 = EIP-1559
    pub access_list: Vec<AccessListItem>,      // EIP-2930 / EIP-1559 warm access list (<= 256 items)
}
```

---

## 3. Cryptographic Signature Formulation & Dual Dispatch

TOLA supports two distinct transaction execution paradigms depending on the signature envelope:

1. **Native TOLA Transactions (`!tx.is_evm()`):**
   Signed with `Signature::Secp256k1` over the native BLAKE3 domain-separated envelope:
   $$\text{SigningDigest} = \text{BLAKE3}(\text{DOMAIN\_TAG} \parallel \text{ChainID} \parallel \text{Epoch} \parallel \text{BLAKE3}(\text{BorshEncode}(\text{UnsignedTx})))$$
   Where:
   - `DOMAIN_TAG`: `b"TOLA-TX-ENVELOPE-v1.0.0-"` (24 bytes).
   - `ChainID`: Little-endian 8-byte network identifier.
   - `Epoch`: Little-endian 8-byte epoch identifier.
   - `UnsignedTx`: Borsh serialization of all transaction fields excluding `signature`.

2. **Ethereum EVM Transactions (`tx.is_evm()`):**
   Signed with `Signature::Secp256k1Eth { r, s, y_parity, chain_id }` conforming to standard Ethereum RLP envelopes:
   - **Legacy Type-0 (EIP-155):** Keccak-256 over RLP tuple with replay-protected chain ID.
   - **Type-1 (EIP-2930):** Keccak-256 over `0x01 || rlp([chainId, nonce, gasPrice, gasLimit, to, value, data, accessList])`.
   - **Type-2 (EIP-1559):** Keccak-256 over `0x02 || rlp([chainId, nonce, maxPriorityFeePerGas, maxFeePerGas, gasLimit, to, value, data, accessList])`.

EVM transactions are dispatched directly to the REVM 43.0.3 execution pipeline for smart contract deployment, inter-contract calls, and ERC-20/721 token transfers.

---

## 4. EIP-1559 Fee Economics & Supply Conservation

Every block header defines a dynamic `base_fee_per_gas`. The effective price paid by the user is dynamically determined:

$$\text{EffectivePrice} = \text{BaseFee} + \min(\text{MaxPriorityFeePerGas}, \text{MaxFeePerGas} - \text{BaseFee})$$

### 4.1 Fee Distribution & Burn Mechanism
For an executed transaction using $\text{GasUsed}$:
1. **Base Fee Burned:**
   $$\text{Burned} = \text{GasUsed} \times \text{BaseFee}$$
   Permanently removed from the circulating token supply.
2. **Priority Fee Paid to Proposer:**
   $$\text{ProposerFee} = \text{GasUsed} \times (\text{EffectivePrice} - \text{BaseFee})$$
   Directly credited to the validator proposing the block.
3. **Unused Gas Refunded to Sender:**
   $$\text{Refund} = (\text{GasLimit} - \text{GasUsed}) \times \text{MaxFeePerGas} + \text{GasUsed} \times (\text{MaxFeePerGas} - \text{EffectivePrice})$$
   Returned to sender's balance immediately upon transaction finalization.

### 4.2 Exact Supply Conservation Invariant
$$\text{UpfrontDebit} = \text{Refund} + \text{ProposerFee} + \text{Burned}$$
Every attotola is mathematically accounted for, preventing any inflation bugs or unmetered gas leaks.
