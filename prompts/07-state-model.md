# SUBSYSTEM: Global State Model & Commitment Tree

## Objective
Design the global state representation, state transition function, and cryptographic state commitment tree for TOLA Network.

## Why It Exists
The state model defines the definitive ledger state at any block height. The cryptographic commitment guarantees that all nodes agree on the exact same state without transmitting all data over the wire.

## Dependencies
- `prompts/04-account-model.md`
- `prompts/06-transaction-model.md`

## Non-Goals
- Full historical state retention on every node (pruning is handled in `prompts/09-storage.md`).

## Requirements
- State Transition: $S' = \text{ApplyBlock}(S, B)$.
- Deterministic, collision-resistant cryptographic state root.
- Efficient Merkle proofs of account balance and contract storage for light clients.
- Atomic state rollback capability in the event of an orphaned or unfinalized speculative block.

## Architecture
```
Global State Commitment: Sparse Merkle Tree (SMT)
                        Root Hash
                       /         \
                  Node A         Node B
                 /      \       /      \
             Leaf 1   Leaf 2  Leaf 3   Leaf 4
             (Acct 1) (Acct 2)(Acct 3) (Acct 4)
```

## Interfaces
```rust
pub trait StateManager {
    fn get_account(&self, addr: &Address) -> Option<Account>;
    fn set_account(&mut self, addr: &Address, acct: Account);
    fn commit(&mut self) -> Result<Hash256, StateError>;
    fn rollback(&mut self);
    fn generate_proof(&self, key: &[u8]) -> Result<MerkleProof, StateError>;
}
```

## Data Structures
```rust
pub struct StateDelta {
    pub modified_accounts: HashMap<Address, Account>,
    pub storage_updates: HashMap<(Address, [u8; 32]), [u8; 32]>,
    pub destroyed_accounts: HashSet<Address>,
}
```

## Security Requirements
- State transitions must be completely deterministic across all CPU architectures (x86_64 and ARM64).
- Floating-point operations strictly forbidden in state transition logic.
- Resistance to tree re-hashing DDoS attacks via cached branch hashes.

## Failure Modes
- Consensus fork caused by non-deterministic state evaluation.
- State corruption due to uncommitted journal rollbacks.

## Testing Requirements
- Differential state tests across 10,000 random block executions.
- Invariant tests: Sum of all balances plus burned fees equals initial supply plus minted rewards.

## Performance Requirements
- State root calculation $< 50\text{ ms}$ for blocks containing 5,000 transactions.

## Migration Requirements
- Support versioned state trie schemas to allow future zero-knowledge friendly hash upgrades (e.g. Poseidon).

## Acceptance Criteria
- Formal state transition specification documented in `docs/protocol/state-model.md`.
- SMT tree architecture and proof verification algorithms specified.

## Open Questions
- Is Sparse Merkle Tree (SMT) superior to Merkle Patricia Trie (MPT) for our execution engine?

## Implementation Tasks
1. Define state delta journaling mechanism.
2. Specify SMT node hashing rules.
3. Author state transition formal proofs.

## Completion Criteria
State transition model formal specification approved and reviewed against threat models.
