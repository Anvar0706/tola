//! Multi-Version Concurrency Memory (MV-Memory) for optimistic Block-STM execution.
//!
//! Stores versioned values per key indexed by (tx_index, incarnation), tracks transaction
//! read sets, and validates read versions against concurrently committed writes.

use std::collections::{BTreeMap, HashMap};
use std::sync::RwLock;
use tola_core::StateDB;
use tola_types::{AccountState, Address, Hash};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum MemoryKey {
    Account(Address),
    Storage(Address, Hash),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MemoryValue {
    Account(Option<AccountState>),
    Storage(Hash),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Version {
    pub tx_index: usize,
    pub incarnation: usize,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MemoryEntry {
    Value(MemoryValue),
    Estimate,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MvReadError {
    ReadEstimate,
    StorageFailure(String),
}

pub struct MvMemory {
    /// MemoryKey -> BTreeMap<tx_index, (incarnation, MemoryEntry)>
    locations: RwLock<HashMap<MemoryKey, BTreeMap<usize, (usize, MemoryEntry)>>>,
    /// tx_index -> Vec<(MemoryKey, Option<Version>)>
    read_sets: RwLock<HashMap<usize, Vec<(MemoryKey, Option<Version>)>>>,
    /// tx_index -> Vec<MemoryKey> (keys written in most recent incarnation)
    write_sets: RwLock<HashMap<usize, Vec<MemoryKey>>>,
}

impl MvMemory {
    pub fn new() -> Self {
        Self {
            locations: RwLock::new(HashMap::new()),
            read_sets: RwLock::new(HashMap::new()),
            write_sets: RwLock::new(HashMap::new()),
        }
    }

    /// Read a key at `tx_index`.
    ///
    /// Searches for the highest write with index < `tx_index`.
    /// - If an entry exists and is `Value`, returns the value and records the version in read set.
    /// - If an entry exists and is `Estimate`, returns `MvReadError::ReadEstimate`.
    /// - If no entry exists with index < `tx_index`, reads from `base_state` and records `None` version.
    pub fn read(
        &self,
        tx_index: usize,
        key: &MemoryKey,
        base_state: &dyn StateDB,
    ) -> Result<MemoryValue, MvReadError> {
        let loc_guard = self.locations.read().unwrap();
        let maybe_entry = loc_guard.get(key).and_then(|history| {
            history
                .range(..tx_index)
                .next_back()
                .map(|(w_idx, (inc, entry))| (*w_idx, *inc, entry.clone()))
        });

        match maybe_entry {
            Some((w_idx, inc, MemoryEntry::Value(val))) => {
                let version = Some(Version {
                    tx_index: w_idx,
                    incarnation: inc,
                });
                drop(loc_guard);
                self.record_read(tx_index, key.clone(), version);
                Ok(val)
            }
            Some((_, _, MemoryEntry::Estimate)) => Err(MvReadError::ReadEstimate),
            None => {
                drop(loc_guard);
                let val = match key {
                    MemoryKey::Account(addr) => {
                        let acct = base_state
                            .get_account(addr)
                            .map_err(|e| MvReadError::StorageFailure(format!("{:?}", e)))?;
                        MemoryValue::Account(acct)
                    }
                    MemoryKey::Storage(addr, slot) => {
                        let slot_val = base_state
                            .get_storage(addr, slot)
                            .map_err(|e| MvReadError::StorageFailure(format!("{:?}", e)))?;
                        MemoryValue::Storage(slot_val)
                    }
                };

                self.record_read(tx_index, key.clone(), None);
                Ok(val)
            }
        }
    }

    /// Record a read dependency for a transaction.
    pub fn record_read(&self, tx_index: usize, key: MemoryKey, version: Option<Version>) {
        let mut rs_guard = self.read_sets.write().unwrap();
        rs_guard.entry(tx_index).or_default().push((key, version));
    }

    /// Clear read set for a transaction prior to speculative re-execution.
    pub fn clear_read_set(&self, tx_index: usize) {
        let mut rs_guard = self.read_sets.write().unwrap();
        rs_guard.remove(&tx_index);
    }

    /// Record newly produced writes for a transaction incarnation.
    pub fn record_writes(
        &self,
        tx_index: usize,
        incarnation: usize,
        writes: Vec<(MemoryKey, MemoryValue)>,
    ) {
        let mut written_keys = Vec::with_capacity(writes.len());
        let mut loc_guard = self.locations.write().unwrap();
        let mut ws_guard = self.write_sets.write().unwrap();

        // Remove old written keys that are no longer part of this incarnation
        if let Some(old_keys) = ws_guard.get(&tx_index) {
            let new_key_set: std::collections::HashSet<&MemoryKey> =
                writes.iter().map(|(k, _)| k).collect();
            for old_key in old_keys {
                if !new_key_set.contains(old_key) {
                    if let Some(history) = loc_guard.get_mut(old_key) {
                        if let Some((existing_inc, _)) = history.get(&tx_index) {
                            if *existing_inc <= incarnation {
                                history.remove(&tx_index);
                            }
                        }
                    }
                }
            }
        }

        for (key, val) in writes {
            written_keys.push(key.clone());
            let history = loc_guard.entry(key).or_default();
            if let Some((existing_inc, _)) = history.get(&tx_index) {
                if *existing_inc > incarnation {
                    continue; // Newer incarnation already recorded writes
                }
            }
            history.insert(tx_index, (incarnation, MemoryEntry::Value(val)));
        }
        ws_guard.insert(tx_index, written_keys);
    }

    /// Mark all writes produced by a transaction as `Estimate` when an abort occurs.
    pub fn mark_estimates(&self, tx_index: usize, incarnation: usize) {
        let keys = {
            let ws_guard = self.write_sets.read().unwrap();
            ws_guard.get(&tx_index).cloned()
        };

        if let Some(keys) = keys {
            let mut loc_guard = self.locations.write().unwrap();
            for key in keys {
                if let Some(history) = loc_guard.get_mut(&key) {
                    if let Some((existing_inc, _)) = history.get(&tx_index) {
                        if *existing_inc > incarnation {
                            continue; // Newer incarnation already exists, do not overwrite
                        }
                    }
                    history.insert(tx_index, (incarnation, MemoryEntry::Estimate));
                }
            }
        }
    }

    /// Validates whether all reads recorded by a transaction remain consistent with
    /// current memory writes.
    pub fn validate_read_set(&self, tx_index: usize) -> bool {
        let reads = {
            let rs_guard = self.read_sets.read().unwrap();
            match rs_guard.get(&tx_index) {
                Some(r) => r.clone(),
                None => return false,
            }
        };

        let loc_guard = self.locations.read().unwrap();

        for (key, expected_version) in &reads {
            let latest_entry = loc_guard
                .get(key)
                .and_then(|history| history.range(..tx_index).next_back());

            match latest_entry {
                Some((_, (_, MemoryEntry::Estimate))) => {
                    // Estimate indicates an in-flight abort of a preceding write; read is invalid
                    return false;
                }
                Some((w_idx, (inc, MemoryEntry::Value(_)))) => {
                    let version = Some(Version {
                        tx_index: *w_idx,
                        incarnation: *inc,
                    });
                    if version != *expected_version {
                        return false;
                    }
                }
                None => {
                    // No preceding write exists; transaction must have read from base state
                    if expected_version.is_some() {
                        return false;
                    }
                }
            }
        }

        true
    }

    /// Extract the final, committed state across all transactions in the block.
    ///
    /// For each unique memory key, selects the write with the highest tx_index < num_txs.
    /// Returns Err if the latest write for any key is an uncommitted Estimate.
    pub fn extract_committed_state(
        &self,
        num_txs: usize,
    ) -> Result<HashMap<MemoryKey, MemoryValue>, String> {
        let loc_guard = self.locations.read().unwrap();
        let mut final_state = HashMap::new();

        for (key, history) in loc_guard.iter() {
            if let Some((_, (_, entry))) = history.range(..num_txs).next_back() {
                match entry {
                    MemoryEntry::Value(val) => {
                        final_state.insert(key.clone(), val.clone());
                    }
                    MemoryEntry::Estimate => {
                        return Err(format!("Uncommitted estimate remaining for key {:?}", key));
                    }
                }
            }
        }

        Ok(final_state)
    }
}

impl Default for MvMemory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tola_state::MemoryStateJournal;

    #[test]
    fn test_mv_memory_base_read_and_versioned_write() {
        let mv = MvMemory::new();
        let mut base = MemoryStateJournal::new();
        let addr = Address([0x11; 20]);
        let initial_acct = AccountState::new_with_balance(tola_types::Amount::from_u128(1_000));
        base.set_account(addr, initial_acct.clone());

        let key = MemoryKey::Account(addr);

        // Tx 0 reads base state -> None version
        let val0 = mv.read(0, &key, &base).unwrap();
        assert_eq!(val0, MemoryValue::Account(Some(initial_acct)));
        assert!(mv.validate_read_set(0));

        // Tx 0 writes new balance
        let updated_acct = AccountState::new_with_balance(tola_types::Amount::from_u128(900));
        mv.record_writes(
            0,
            0,
            vec![(
                key.clone(),
                MemoryValue::Account(Some(updated_acct.clone())),
            )],
        );

        // Tx 1 reads -> sees Tx 0's write at version (0, 0)
        let val1 = mv.read(1, &key, &base).unwrap();
        assert_eq!(val1, MemoryValue::Account(Some(updated_acct)));
        assert!(mv.validate_read_set(1));

        // Tx 0 aborts and marks estimates
        mv.mark_estimates(0, 1);

        // Tx 1 validation must fail because Tx 0's write became an Estimate
        assert!(!mv.validate_read_set(1));
    }
}
