# SUBSYSTEM: Account & State Identity Model

## Objective
Design the account architecture of TOLA Network, supporting externally owned accounts, smart contracts, validator identities, and system accounts.

## Why It Exists
The account model defines how state is organized, balances are tracked, code is attached, and ownership is validated.

## Dependencies
- `prompts/03-cryptography.md`

## Non-Goals
- UTXO-based transaction tracking (TOLA uses an account-based state model).
- Anonymous mixers embedded directly into the base account model.

## Requirements
- Account-based state model similar to modern high-performance architectures.
- Explicit account types:
  1. **Externally Owned Account (EOA)**: Controlled by private key (Secp256k1 or Ed25519).
  2. **Smart Contract Account**: Contains bytecode and persistent key-value storage.
  3. **Validator Account**: Contains bonded stake, commission rate, and consensus public keys.
  4. **System Governance Account**: Built-in protocol parameters and upgrade gates.
- Strictly monotonic nonce per account to guarantee replay protection.

## Architecture
```
Account Record (State Leaf)
┌──────────────────────────────────────────────┐
│ Nonce: u64                                   │
│ Balance: u128 (TOLA base units)              │
│ CodeHash: Hash256 (Empty for EOA)            │
│ StorageRoot: Hash256 (Empty for EOA)         │
│ AccountFlags: u16 (Contract / Validator / ..)│
└──────────────────────────────────────────────┘
```

## Interfaces
```rust
pub trait AccountStateReader {
    fn get_account(&self, address: &Address) -> Option<Account>;
    fn get_balance(&self, address: &Address) -> u128;
    fn get_nonce(&self, address: &Address) -> u64;
}
```

## Data Structures
```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub nonce: u64,
    pub balance: u128,
    pub code_hash: [u8; 32],
    pub storage_root: [u8; 32],
    pub flags: AccountFlags,
}
```

## Security Requirements
- Balances represented as 128-bit unsigned integers to prevent overflow while supporting micro-denominations.
- Zero-balance accounts with zero nonce automatically pruned or reclaimed to prevent state bloat.

## Failure Modes
- Nonce desynchronization between mempool and committed state.
- Integer overflow during massive balance credit operations.

## Testing Requirements
- State transitions tested against overflow, underflow, and re-initialization exploits.
- Nonce collision and reordering unit tests.

## Performance Requirements
- Account record serialization $< 128$ bytes.
- Point lookup latency from cache $< 5\mu s$.

## Migration Requirements
- Extensible account flags field allows adding account abstraction (ERC-4337 style) in later phases.

## Acceptance Criteria
- Formal account model specification documented in `docs/protocol/account-model.md`.
- Data structure layouts and serializations validated.

## Open Questions
- Should account storage support native multi-token balances or enforce standard contract storage?

## Implementation Tasks
1. Define account struct and flags bitmask.
2. Specify balance arithmetic rules with checked operations.
3. Author account lifecycle state transitions.

## Completion Criteria
Account specification complete, tested with unit assertions, and integrated into state transition model.
