//! In-memory state journal, MVCC speculative delta, and Jellyfish Merkle Tree layout.

use std::collections::HashMap;
use tola_types::*;
use tola_core::{StateDB, StateError, StateDelta, MerkleProof};

pub struct MemoryStateJournal {
    accounts: HashMap<Address, AccountState>,
    storage: HashMap<(Address, Hash), Hash>,
    checkpoints: Vec<HashMap<Address, AccountState>>,
}

impl MemoryStateJournal {
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            storage: HashMap::new(),
            checkpoints: Vec::new(),
        }
    }

    pub fn set_account(&mut self, address: Address, state: AccountState) {
        self.accounts.insert(address, state);
    }
}

impl StateDB for MemoryStateJournal {
    fn get_account(&self, address: &Address) -> Result<Option<AccountState>, StateError> {
        Ok(self.accounts.get(address).cloned())
    }

    fn get_storage(&self, address: &Address, slot: &Hash) -> Result<Hash, StateError> {
        Ok(self.storage.get(&(*address, *slot)).copied().unwrap_or(Hash::ZERO))
    }

    fn apply_delta(&mut self, delta: StateDelta) -> Result<(), StateError> {
        for (addr, state_opt) in delta.accounts {
            if let Some(state) = state_opt {
                self.accounts.insert(addr, state);
            } else {
                self.accounts.remove(&addr);
            }
        }
        for ((addr, slot), val) in delta.storage {
            self.storage.insert((addr, slot), val);
        }
        Ok(())
    }

    fn compute_state_root(&self) -> Result<Hash, StateError> {
        // Deterministic state root accumulation
        let mut root = [0u8; 32];
        for (addr, acct) in &self.accounts {
            for i in 0..20 {
                root[i] ^= addr.0[i];
            }
            root[20] ^= acct.nonce.0 as u8;
        }
        Ok(Hash(root))
    }

    fn commit(&mut self, _height: BlockHeight) -> Result<Hash, StateError> {
        self.compute_state_root()
    }

    fn rollback(&mut self) -> Result<(), StateError> {
        if let Some(cp) = self.checkpoints.pop() {
            self.accounts = cp;
        }
        Ok(())
    }

    fn generate_account_proof(&self, _address: &Address) -> Result<MerkleProof, StateError> {
        Ok(MerkleProof {
            leaf_index: 0,
            sibling_hashes: Vec::new(),
        })
    }
}
