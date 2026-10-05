# TOLA Network: Transaction Specification v0.1

## 1. Canonical Wire Encoding & Layout

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transaction {
    pub chain_id: u64,
    pub nonce: u64,
    pub from: Address,
    pub to: Option<Address>,
    pub value: u128,
    pub gas_limit: u64,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
    pub valid_until_block: u64,
    pub data: Vec<u8>,
    pub signature: [u8; 65], // Secp256k1 (r: 32, s: 32, v: 1)
}
```

---

## 2. Signature Pre-Image Formulation

The signature pre-image is strictly serialized using Borsh:

$$\text{SigningHash} = \text{BLAKE3}(\text{DOMAIN\_TX} \parallel \text{BorshEncode}(\text{TxWithoutSignature}))$$

$$\text{Signature} = \text{Sign}_{\text{Secp256k1}}(\text{PrivateKey}, \text{SigningHash})$$

---

## 3. Transaction Validation Pipeline

```
Raw Bytes Received
       │
       ▼
1. Stateless Check:
   - Size <= 128 KB
   - ChainID == CurrentNetwork
   - GasLimit >= 21,000 && GasLimit <= BlockGasLimit
   - Signature Verification (ecrecover) -> Matches 'from'
       │
       ▼
2. Stateful Check (against MDBX Snapshot):
   - Nonce == State[from].nonce
   - Balance >= (GasLimit * MaxFeePerGas) + Value
       │
       ▼
3. Mempool Admission -> Priority Queue by PriorityFeePerGas
```
