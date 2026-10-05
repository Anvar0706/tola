# SUBSYSTEM: Block Engine & Chain Management

## Objective
Design the block structure, header format, block assembly engine, block validation pipeline, and fork-choice / chain progression management.

## Why It Exists
Blocks are the discrete units of consensus progress. The block engine packages mempool transactions, coordinates state execution, verifies block proofs, and updates the local canonical tip of the chain.

## Dependencies
- `prompts/06-transaction-model.md`
- `prompts/07-state-model.md`
- `prompts/09-storage.md`

## Non-Goals
- Validator voting protocol (handled in `prompts/13-consensus.md`).

## Requirements
- Block Header:
  - `parent_hash`: 32-byte cryptographic digest of the prior block.
  - `height`: Monotonically increasing 64-bit integer.
  - `timestamp`: UTC UNIX timestamp (milliseconds).
  - `state_root`: 32-byte Sparse Merkle Tree root after executing all transactions.
  - `txs_root`: 32-byte Merkle root of transactions.
  - `receipts_root`: 32-byte Merkle root of transaction receipts.
  - `proposer`: Address of the block proposer validator.
  - `gas_used`: Total computational gas consumed.
  - `gas_limit`: Block gas capacity limit.
  - `extra_data`: Arbitrary validator payload (max 32 bytes).
- Block Body: Array of serialized canonical transactions.

## Architecture
```
Block Production Pipeline:
Mempool Pull -> Gas Limit Packing -> VM Execution -> Receipts Generation
             -> State Root Calculation -> Header Assembly -> Consensus Sign
```

## Interfaces
```rust
pub trait BlockEngine {
    fn assemble_block(&self, proposer: Address, max_gas: u64) -> Result<Block, BlockError>;
    fn verify_block(&self, block: &Block) -> Result<(), BlockValidationError>;
    fn commit_block(&mut self, block: Block) -> Result<CommitOutcome, BlockError>;
    fn get_canonical_tip(&self) -> BlockHeader;
}
```

## Data Structures
```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockHeader {
    pub parent_hash: [u8; 32],
    pub height: u64,
    pub timestamp: u64,
    pub state_root: [u8; 32],
    pub txs_root: [u8; 32],
    pub receipts_root: [u8; 32],
    pub proposer: Address,
    pub gas_used: u64,
    pub gas_limit: u64,
    pub extra_data: Vec<u8>,
}

pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}
```

## Security Requirements
- Strict timestamp sanity checks: Block timestamp must be $> \text{parent.timestamp}$ and $< \text{local\_time} + \Delta$ (e.g. 5 seconds clock drift limit).
- Gas limit enforcement: Total `tx.gas_limit` sum and actual `gas_used` must never exceed `header.gas_limit`.

## Failure Modes
- Proposing invalid blocks leading to proposer slashing.
- Chain reorganization beyond finality depth (must be mathematically impossible under BFT).

## Testing Requirements
- Unit tests validating header sanity checks (timestamp drift, gas overrun, invalid state roots).
- Fork resolution tests simulating competing proposals before finality.

## Performance Requirements
- Block assembly time $< 200\text{ ms}$ for 2,000 standard transactions.

## Migration Requirements
- Extensible header fields to allow future sharding receipts and zero-knowledge validity roots.

## Acceptance Criteria
- Block header and body specification documented in `docs/protocol/block-engine.md`.
- Invariant tests passing for block assembly and validation.

## Open Questions
- What is the optimal baseline block time target (1.0 second vs 2.0 seconds)?

## Implementation Tasks
1. Specify block header fields and serialization.
2. Define Merkle tree root calculation for transactions and receipts.
3. Author block validation state machine.

## Completion Criteria
Block engine specification reviewed and aligned with consensus and execution modules.
