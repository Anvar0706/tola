# SUBSYSTEM: Transaction Architecture & Validation Pipeline

## Objective
Design the canonical transaction data structure, validation pipeline, replay protection, fee payment semantics, and execution envelope for TOLA Network.

## Why It Exists
Transactions represent the sole mechanism through which external users mutate blockchain state. A flawed transaction model allows replay attacks, front-running, gas theft, and state corruption.

## Dependencies
- `prompts/03-cryptography.md`
- `prompts/04-account-model.md`
- `prompts/05-addresses.md`

## Non-Goals
- Off-chain state channels (handled in Layer 2 extensions).

## Requirements
- Fields:
  - `chain_id`: 64-bit network identifier to prevent cross-chain replay.
  - `nonce`: Monotonically increasing account nonce.
  - `from`: Originating account address.
  - `to`: Recipient account address (None for contract creation).
  - `value`: Amount of native TOLA transferred (u128).
  - `gas_limit`: Maximum computational fuel allowed.
  - `gas_price`: Fee per unit of gas (or EIP-1559 base + priority fee).
  - `payload`: Binary calldata or contract creation bytecode.
  - `signature`: Cryptographic signature (65 bytes for Secp256k1 recovery, or 64 bytes for Ed25519).
- Expiration: Optional block-height expiration window to prevent pending transactions from hanging indefinitely.

## Architecture
```
Transaction Validation Pipeline:
Raw Bytes -> Deserialization -> Structural Validation -> Signature Verification
          -> Nonce Check -> Balance Check (Value + GasLimit * GasPrice) -> Mempool
```

## Interfaces
```rust
pub trait TransactionValidator {
    fn validate_stateless(&self, tx: &Transaction) -> Result<(), TxError>;
    fn validate_stateful(&self, tx: &Transaction, state: &StateReader) -> Result<(), TxError>;
    fn extract_sender(&self, tx: &Transaction) -> Result<Address, CryptoError>;
}
```

## Data Structures
```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transaction {
    pub chain_id: u64,
    pub nonce: u64,
    pub from: Address,
    pub to: Option<Address>,
    pub value: u128,
    pub gas_limit: u64,
    pub gas_price: u128,
    pub data: Vec<u8>,
    pub signature: Vec<u8>,
}
```

## Security Requirements
- Strict replay protection via `chain_id` included in signature hash pre-image.
- Strict sender balance verification: $\text{Balance} \ge \text{Value} + (\text{GasLimit} \times \text{GasPrice})$.
- Zero signature malleability.

## Failure Modes
- Replay across networks.
- Nonce gap stalling transaction queue.
- Insufficient gas causing out-of-gas reverts with burned fees.

## Testing Requirements
- Negative tests for malformed payloads, invalid signatures, expired nonces, and underfunded accounts.
- Differential serialization fuzz tests.

## Performance Requirements
- Stateless transaction validation $< 50\mu s$ per transaction.

## Migration Requirements
- Typed transaction envelope (similar to EIP-2718) to support future transaction formats.

## Acceptance Criteria
- Transaction specification documented in `docs/protocol/transaction-model.md`.
- Wire format and signature hash serialization defined.

## Open Questions
- Should we adopt EIP-1559 dynamic fee burning natively from Genesis?

## Implementation Tasks
1. Specify binary wire format.
2. Define signature hashing algorithm.
3. Author validation state machine.

## Completion Criteria
Transaction specification approved and ready for serialization engine implementation.
