# TOLA Network: Trait Contracts & Component Interfaces v0.1

## 1. Architectural Role of Trait Contracts
Trait contracts establish the strict compile-time boundaries between TOLA subsystems. Implementations of consensus, storage, execution, and networking interact exclusively through these zero-allocation Rust traits.

---

## 2. StateDB Trait Contract

```rust
use crate::types::*;

pub trait StateDB: Send + Sync {
    /// Read account state at the current working version.
    fn get_account(&self, address: &Address) -> Result<Option<AccountState>, StateError>;

    /// Read contract storage slot at the current working version.
    fn get_storage(&self, address: &Address, slot: &Hash) -> Result<Hash, StateError>;

    /// Check if account exists.
    fn account_exists(&self, address: &Address) -> Result<bool, StateError> {
        Ok(self.get_account(address)?.is_some())
    }

    /// Read account balance.
    fn get_balance(&self, address: &Address) -> Result<Amount, StateError> {
        Ok(self.get_account(address)?.map(|a| a.balance).unwrap_or_default())
    }

    /// Read account nonce.
    fn get_nonce(&self, address: &Address) -> Result<Nonce, StateError> {
        Ok(self.get_account(address)?.map(|a| a.nonce).unwrap_or_default())
    }

    /// Apply batch of state updates atomically into the local state journal.
    fn apply_delta(&mut self, delta: StateDelta) -> Result<(), StateError>;

    /// Compute cryptographic Jellyfish Merkle Tree state root.
    fn compute_state_root(&self) -> Result<Hash, StateError>;

    /// Commit active state journal to persistent MDBX storage at block height.
    fn commit(&mut self, height: BlockHeight) -> Result<Hash, StateError>;

    /// Roll back uncommitted state journal changes to the last commit checkpoint.
    fn rollback(&mut self) -> Result<(), StateError>;

    /// Generate cryptographic Merkle inclusion/non-inclusion proof for an account.
    fn generate_account_proof(&self, address: &Address) -> Result<MerkleProof, StateError>;

    /// Verify a cryptographic account proof against a given state root.
    fn verify_account_proof(
        &self,
        state_root: &Hash,
        address: &Address,
        proof: &MerkleProof,
    ) -> Result<Option<AccountState>, StateError>;
}
```

---

## 3. BlockEngine Trait Contract

```rust
use crate::types::*;

pub trait BlockEngine: Send + Sync {
    /// Validate candidate block statelessly and statefully prior to voting.
    fn validate_block(&self, block: &Block, state: &dyn StateDB) -> Result<(), BlockValidationError>;

    /// Construct a candidate block from mempool transactions for height and round.
    fn assemble_block(
        &self,
        height: BlockHeight,
        round: Round,
        proposer: Address,
        mempool: &dyn MempoolService,
        state: &dyn StateDB,
    ) -> Result<Block, BlockCreationError>;

    /// Execute transactions in a block and return execution receipts and state delta.
    fn execute_block(
        &self,
        block: &Block,
        state: &mut dyn StateDB,
    ) -> Result<BlockExecutionOutcome, ExecutionError>;

    /// Commit block, receipts, and state atomically to canonical chain storage.
    fn commit_block(&mut self, block: Block, outcome: BlockExecutionOutcome) -> Result<(), StorageError>;
}
```

---

## 4. ConsensusEngine Trait Contract

```rust
use crate::types::*;

pub trait ConsensusEngine: Send + Sync {
    /// Process incoming block proposal from designated round leader.
    fn handle_proposal(&mut self, proposal: BlockProposal) -> Result<Option<Vote>, ConsensusError>;

    /// Ingest validator vote and evaluate quorum thresholds.
    fn handle_vote(&mut self, vote: Vote) -> Result<Option<QuorumCertificate>, ConsensusError>;

    /// Trigger pacemaker timeout when view timer expires.
    fn on_timeout(&mut self, height: BlockHeight, round: Round) -> Result<Option<TimeoutMsg>, ConsensusError>;

    /// Retrieve the currently active validator set for a given epoch.
    fn get_validator_set(&self, epoch: Epoch) -> Result<ValidatorSet, ConsensusError>;

    /// Return the latest irrevocably finalized block height.
    fn get_last_finalized_height(&self) -> BlockHeight;
}
```

---

## 5. VMRuntime Trait Contract

```rust
use crate::types::*;

pub trait VMRuntime: Send + Sync {
    /// Execute a single transaction sequentially or within a Block-STM worker thread.
    fn execute_transaction(
        &self,
        tx: &Transaction,
        state_reader: &dyn StateReader,
        block_ctx: &BlockContext,
    ) -> Result<ExecutionResult, VMError>;
}

pub struct ExecutionResult {
    pub success: bool,
    pub gas_used: Gas,
    pub state_updates: StateDelta,
    pub logs: Vec<Log>,
    pub return_data: Vec<u8>,
}
```

---

## 6. MempoolService Trait Contract

```rust
use crate::types::*;

pub trait MempoolService: Send + Sync {
    /// Validate and insert transaction into the pool.
    fn insert(&self, tx: Transaction) -> Result<TransactionId, MempoolError>;

    /// Select optimal transactions ordered by priority fee for block packaging.
    fn select_transactions(&self, max_count: usize, max_gas: Gas) -> Vec<Transaction>;

    /// Remove transactions committed into the canonical chain.
    fn remove_committed(&self, tx_ids: &[TransactionId]);

    /// Query mempool capacity and status.
    fn stats(&self) -> MempoolStats;
}
```

---

## 7. P2PService Trait Contract

```rust
use crate::types::*;

pub trait P2PService: Send + Sync {
    /// Broadcast a transaction to the network via GossipSub.
    fn broadcast_transaction(&self, tx: &Transaction) -> Result<(), NetworkError>;

    /// Broadcast a proposed block to validators.
    fn broadcast_block(&self, block: &Block) -> Result<(), NetworkError>;

    /// Broadcast a consensus vote or Quorum Certificate.
    fn broadcast_vote(&self, vote: &Vote) -> Result<(), NetworkError>;

    /// Query connected peer count.
    fn peer_count(&self) -> usize;
}
```
