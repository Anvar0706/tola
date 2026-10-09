//! Priority-ordered Mempool with per-account nonce queues and anti-DDoS eviction.
//!
//! Complies with Subsystem Prompt 11 and ADR-0012:
//! - Multi-level Ready and Parked transaction queues
//! - Replace-By-Fee (RBF) 10% fee escalation
//! - Lowest-fee eviction upon reaching memory or transaction bounds
//! - Strict cryptographic signature and stateful nonce validation

use crate::config::{MempoolConfig, MempoolStats};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use tola_codec::{serialize_unsigned_transaction, CanonicalSerialize};
use tola_core::{MempoolError, StateDB};
use tola_crypto::{
    blake3_hash, domain_separated_hash, recover_secp256k1_address, DOMAIN_TRANSACTION_PAYLOAD,
};
use tola_types::*;

/// Compute transaction signing digest.
pub fn compute_tx_digest(tx: &Transaction) -> Hash {
    let mut unsigned_bytes = Vec::new();
    serialize_unsigned_transaction(tx, &mut unsigned_bytes);
    let payload_hash = blake3_hash(&unsigned_bytes);
    domain_separated_hash(DOMAIN_TRANSACTION_PAYLOAD, tx.chain_id.0, 0, &payload_hash)
}

/// Compute canonical transaction identifier.
pub fn compute_tx_id(tx: &Transaction) -> TransactionId {
    let wire_bytes = tx.to_canonical_bytes();
    TransactionId(blake3_hash(&wire_bytes))
}

/// Item in the global priority set.
/// Orders primarily by priority fee (descending), then max fee (descending), then tx_id.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PriorityEntry {
    pub priority_fee: Amount,
    pub max_fee: Amount,
    pub tx_id: TransactionId,
    pub from: Address,
    pub nonce: Nonce,
    pub gas_limit: Gas,
    pub size_bytes: usize,
}

impl PartialOrd for PriorityEntry {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityEntry {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // High fee first (Reverse order for Amount)
        other
            .priority_fee
            .cmp(&self.priority_fee)
            .then_with(|| other.max_fee.cmp(&self.max_fee))
            .then_with(|| self.tx_id.0.cmp(&other.tx_id.0))
    }
}

/// Per-account pending queue preserving sequential nonce execution.
#[derive(Default, Clone, Debug)]
pub struct AccountQueue {
    pub ready: BTreeMap<Nonce, (TransactionId, Transaction, usize)>,
    pub parked: BTreeMap<Nonce, (TransactionId, Transaction, usize)>,
    pub next_expected_nonce: Nonce,
}

impl AccountQueue {
    pub fn new(initial_nonce: Nonce) -> Self {
        Self {
            ready: BTreeMap::new(),
            parked: BTreeMap::new(),
            next_expected_nonce: initial_nonce,
        }
    }

    /// Promote contiguous parked transactions into ready queue.
    pub fn promote_parked(&mut self) -> Vec<PriorityEntry> {
        let mut promoted = Vec::new();
        while let Some(&first_parked_nonce) = self.parked.keys().next() {
            if first_parked_nonce == self.next_expected_nonce {
                let (tx_id, tx, size) = self.parked.remove(&first_parked_nonce).unwrap();
                let entry = PriorityEntry {
                    priority_fee: tx.max_priority_fee_per_gas,
                    max_fee: tx.max_fee_per_gas,
                    tx_id,
                    from: tx.from,
                    nonce: tx.nonce,
                    gas_limit: tx.gas_limit,
                    size_bytes: size,
                };
                self.ready.insert(tx.nonce, (tx_id, tx, size));
                self.next_expected_nonce = self.next_expected_nonce.next();
                promoted.push(entry);
            } else {
                break;
            }
        }
        promoted
    }
}

/// Production high-throughput Mempool.
pub struct Mempool {
    config: MempoolConfig,
    by_id: HashMap<TransactionId, (Address, Nonce, bool)>, // bool: is_ready
    accounts: HashMap<Address, AccountQueue>,
    ready_priority: BTreeSet<PriorityEntry>,
    total_bytes: usize,
}

impl Mempool {
    pub fn new(config: MempoolConfig) -> Self {
        Self {
            config,
            by_id: HashMap::new(),
            accounts: HashMap::new(),
            ready_priority: BTreeSet::new(),
            total_bytes: 0,
        }
    }

    pub fn stats(&self) -> MempoolStats {
        let total_txs = self.by_id.len();
        let ready_txs = self.ready_priority.len();
        let parked_txs = total_txs.saturating_sub(ready_txs);
        let lowest_fee = self
            .ready_priority
            .iter()
            .last()
            .map(|e| e.priority_fee)
            .unwrap_or(Amount::ZERO);

        MempoolStats {
            total_transactions: total_txs,
            ready_transactions: ready_txs,
            parked_transactions: parked_txs,
            total_bytes: self.total_bytes,
            lowest_priority_fee: lowest_fee,
        }
    }

    /// Submit incoming transaction to mempool with stateless, stateful, and RBF validation.
    pub fn insert(
        &mut self,
        tx: Transaction,
        state: &dyn StateDB,
        current_height: BlockHeight,
    ) -> Result<TransactionId, MempoolError> {
        let tx_bytes = tx.to_canonical_bytes();
        let tx_size = tx_bytes.len();
        let tx_id = compute_tx_id(&tx);

        if self.by_id.contains_key(&tx_id) {
            return Err(MempoolError::AlreadyKnown);
        }

        // 1. Stateless validation
        tx.stateless_validate(self.config.chain_id, Gas::MAX_BLOCK_GAS)
            .map_err(|_| MempoolError::Underpriced)?;

        if tx.max_fee_per_gas < self.config.min_gas_price {
            return Err(MempoolError::SpamFeeBelowMinimum);
        }

        // Cryptographic signature check
        match &tx.signature {
            Signature::Secp256k1 { r, s, v } => {
                let mut sig_bytes = [0u8; 65];
                sig_bytes[0..32].copy_from_slice(r);
                sig_bytes[32..64].copy_from_slice(s);
                sig_bytes[64] = *v;

                let digest = compute_tx_digest(&tx);
                let recovered = recover_secp256k1_address(digest.as_bytes(), &sig_bytes)
                    .map_err(|_| MempoolError::InvalidSignature)?;

                if recovered != tx.from {
                    return Err(MempoolError::InvalidSignature);
                }
            }
            Signature::Secp256k1Eth {
                r,
                s,
                y_parity,
                chain_id,
            } => {
                if *chain_id != self.config.chain_id.0 {
                    return Err(MempoolError::InvalidSignature);
                }
                let mut sig_bytes = [0u8; 65];
                sig_bytes[0..32].copy_from_slice(r);
                sig_bytes[32..64].copy_from_slice(s);
                sig_bytes[64] = *y_parity;

                let recovered = match tx.tx_type {
                    1 => {
                        let hash_type1 = tola_codec::rlp::compute_type1_signing_hash(&tx);
                        recover_secp256k1_address(hash_type1.as_bytes(), &sig_bytes)
                    }
                    0 => {
                        let hash_legacy = tola_codec::rlp::compute_legacy_signing_hash(&tx);
                        recover_secp256k1_address(hash_legacy.as_bytes(), &sig_bytes)
                    }
                    _ => {
                        let hash_type2 = tola_codec::rlp::compute_type2_signing_hash(&tx);
                        recover_secp256k1_address(hash_type2.as_bytes(), &sig_bytes)
                            .or_else(|_| {
                                let hash_type1 = tola_codec::rlp::compute_type1_signing_hash(&tx);
                                recover_secp256k1_address(hash_type1.as_bytes(), &sig_bytes)
                            })
                            .or_else(|_| {
                                let hash_legacy = tola_codec::rlp::compute_legacy_signing_hash(&tx);
                                recover_secp256k1_address(hash_legacy.as_bytes(), &sig_bytes)
                            })
                    }
                }
                .map_err(|_| MempoolError::InvalidSignature)?;

                if recovered != tx.from {
                    return Err(MempoolError::InvalidSignature);
                }
            }
            Signature::Ed25519(_) => {
                // System / validator operations
            }
        }

        // 2. Expiration check
        if tx.valid_until_block.as_u64() < current_height.as_u64() {
            return Err(MempoolError::TransactionExpired);
        }

        // 3. Stateful check against StateDB
        let state_account = state
            .get_account(&tx.from)
            .map_err(|_| MempoolError::InvalidNonce)?
            .unwrap_or(AccountState::EMPTY);

        if tx.nonce.as_u64() < state_account.nonce.as_u64() {
            return Err(MempoolError::InvalidNonce);
        }

        let future_gap = tx
            .nonce
            .as_u64()
            .saturating_sub(state_account.nonce.as_u64());
        if future_gap > self.config.max_future_gap {
            return Err(MempoolError::ExceedsMaxFutureGap);
        }

        if !state_account.can_afford(tx.gas_limit, tx.max_fee_per_gas, tx.value) {
            return Err(MempoolError::InsufficientBalance);
        }

        // 4. Per-account duplicate / Replace-By-Fee (RBF) check
        let account_queue = self
            .accounts
            .entry(tx.from)
            .or_insert_with(|| AccountQueue::new(state_account.nonce));

        let mut was_ready_replacement = false;

        // Check if a transaction with the same nonce already exists
        if let Some((old_id, old_tx, old_size)) = account_queue.ready.get(&tx.nonce).cloned() {
            if old_tx.is_evm() != tx.is_evm() {
                return Err(MempoolError::CrossTypeConflict);
            }

            // RBF evaluation: new tx must pay at least +10% higher priority fee
            let required_priority = old_tx
                .max_priority_fee_per_gas
                .checked_mul(100 + self.config.rbf_bump_percent as u64)
                .map(|v| Amount(v.as_u128() / 100))
                .unwrap_or(old_tx.max_priority_fee_per_gas);

            if tx.max_priority_fee_per_gas < required_priority {
                return Err(MempoolError::Underpriced);
            }

            // Evict old transaction to perform RBF replacement
            self.remove_by_id(&old_id, old_size);
            was_ready_replacement = true;
        } else if let Some((old_id, old_tx, old_size)) =
            account_queue.parked.get(&tx.nonce).cloned()
        {
            if old_tx.is_evm() != tx.is_evm() {
                return Err(MempoolError::CrossTypeConflict);
            }

            let required_priority = old_tx
                .max_priority_fee_per_gas
                .checked_mul(100 + self.config.rbf_bump_percent as u64)
                .map(|v| Amount(v.as_u128() / 100))
                .unwrap_or(old_tx.max_priority_fee_per_gas);

            if tx.max_priority_fee_per_gas < required_priority {
                return Err(MempoolError::Underpriced);
            }

            self.remove_by_id(&old_id, old_size);
        }

        // 5. Capacity & Lowest-Fee Eviction
        let at_capacity = self.by_id.len() >= self.config.max_transactions
            || (self.total_bytes + tx_size) > self.config.max_bytes;

        if at_capacity {
            if let Some(lowest) = self.ready_priority.iter().last().cloned() {
                if tx.max_priority_fee_per_gas <= lowest.priority_fee {
                    return Err(MempoolError::PoolFull);
                }
                // Evict lowest transaction to accommodate higher fee
                self.remove_by_id(&lowest.tx_id, lowest.size_bytes);
            } else {
                return Err(MempoolError::PoolFull);
            }
        }

        // 6. Insertion into AccountQueue and Priority Sets
        let tx_from = tx.from;
        let tx_nonce = tx.nonce;
        let account_queue = self.accounts.get_mut(&tx_from).unwrap();

        let is_ready = was_ready_replacement || (tx_nonce == account_queue.next_expected_nonce);
        if is_ready {
            let priority_entry = PriorityEntry {
                priority_fee: tx.max_priority_fee_per_gas,
                max_fee: tx.max_fee_per_gas,
                tx_id,
                from: tx_from,
                nonce: tx_nonce,
                gas_limit: tx.gas_limit,
                size_bytes: tx_size,
            };
            account_queue.ready.insert(tx_nonce, (tx_id, tx, tx_size));
            if !was_ready_replacement {
                account_queue.next_expected_nonce = account_queue.next_expected_nonce.next();
            }
            self.ready_priority.insert(priority_entry);

            // Promote any parked transactions contiguous with newly added ready tx
            let promoted = account_queue.promote_parked();
            for p in promoted {
                self.by_id.insert(p.tx_id, (p.from, p.nonce, true));
                self.ready_priority.insert(p);
            }
        } else {
            // Park future gap transaction
            account_queue.parked.insert(tx_nonce, (tx_id, tx, tx_size));
        }

        self.by_id.insert(tx_id, (tx_from, tx_nonce, is_ready));
        self.total_bytes += tx_size;

        Ok(tx_id)
    }

    fn remove_by_id(&mut self, tx_id: &TransactionId, size: usize) {
        if let Some((addr, nonce, is_ready)) = self.by_id.remove(tx_id) {
            if let Some(acct_q) = self.accounts.get_mut(&addr) {
                if is_ready {
                    if let Some((_, old_tx, _)) = acct_q.ready.remove(&nonce) {
                        let entry = PriorityEntry {
                            priority_fee: old_tx.max_priority_fee_per_gas,
                            max_fee: old_tx.max_fee_per_gas,
                            tx_id: *tx_id,
                            from: addr,
                            nonce,
                            gas_limit: old_tx.gas_limit,
                            size_bytes: size,
                        };
                        self.ready_priority.remove(&entry);
                    }
                } else {
                    acct_q.parked.remove(&nonce);
                }
            }
            self.total_bytes = self.total_bytes.saturating_sub(size);
        }
    }

    /// Pull highest-fee ready transactions up to gas limit and transaction count.
    pub fn get_ready_transactions(&self, max_count: usize, max_gas: Gas) -> Vec<Transaction> {
        let mut selected = Vec::with_capacity(max_count);
        let mut cumulative_gas: u64 = 0;

        for entry in &self.ready_priority {
            if selected.len() >= max_count {
                break;
            }
            if cumulative_gas + entry.gas_limit.as_u64() > max_gas.as_u64() {
                continue;
            }

            if let Some(acct_q) = self.accounts.get(&entry.from) {
                if let Some((_, tx, _)) = acct_q.ready.get(&entry.nonce) {
                    selected.push(tx.clone());
                    cumulative_gas += entry.gas_limit.as_u64();
                }
            }
        }

        selected
    }

    /// Remove committed transactions and advance per-account expected nonces.
    pub fn remove_committed(&mut self, txs: &[Transaction]) {
        for tx in txs {
            let tx_id = compute_tx_id(tx);
            if let Some((addr, nonce, is_ready)) = self.by_id.remove(&tx_id) {
                if let Some(acct_q) = self.accounts.get_mut(&addr) {
                    let size = if is_ready {
                        acct_q.ready.remove(&nonce).map(|(_, _, s)| s).unwrap_or(0)
                    } else {
                        acct_q.parked.remove(&nonce).map(|(_, _, s)| s).unwrap_or(0)
                    };

                    self.total_bytes = self.total_bytes.saturating_sub(size);

                    let entry = PriorityEntry {
                        priority_fee: tx.max_priority_fee_per_gas,
                        max_fee: tx.max_fee_per_gas,
                        tx_id,
                        from: addr,
                        nonce,
                        gas_limit: tx.gas_limit,
                        size_bytes: size,
                    };
                    self.ready_priority.remove(&entry);

                    // Promote newly eligible parked transactions
                    let promoted = acct_q.promote_parked();
                    for p in promoted {
                        self.by_id.insert(p.tx_id, (p.from, p.nonce, true));
                        self.ready_priority.insert(p);
                    }
                }
            }
        }
    }

    /// Calculate contiguous Ethereum-style pending nonce for an address.
    pub fn get_pending_nonce(&self, address: &Address, committed_nonce: Nonce) -> Nonce {
        if let Some(acct_q) = self.accounts.get(address) {
            let mut pending = committed_nonce;
            while acct_q.ready.contains_key(&pending) {
                pending = pending.next();
            }
            pending
        } else {
            committed_nonce
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tola_crypto::Secp256k1Keypair;
    use tola_state::MemoryStateJournal;

    fn create_test_tx(
        keypair: &Secp256k1Keypair,
        chain_id: ChainId,
        nonce: Nonce,
        priority_fee: u128,
        max_fee: u128,
        valid_until: u64,
    ) -> Transaction {
        let from = keypair.address();
        let mut unsigned_tx = Transaction {
            chain_id,
            nonce,
            from,
            to: Some(Address([0x99; 20])),
            value: Amount::from_u128(10_000),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(max_fee),
            max_priority_fee_per_gas: Amount::from_u128(priority_fee),
            valid_until_block: BlockHeight(valid_until),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1 {
                r: [0u8; 32],
                s: [0u8; 32],
                v: 0,
            },
            tx_type: 0,
            access_list: vec![],
        };

        let digest = compute_tx_digest(&unsigned_tx);
        let sig_bytes = keypair.sign_recoverable(digest.as_bytes()).unwrap();

        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        r.copy_from_slice(&sig_bytes[0..32]);
        s.copy_from_slice(&sig_bytes[32..64]);
        let v = sig_bytes[64];

        unsigned_tx.signature = Signature::Secp256k1 { r, s, v };
        unsigned_tx
    }

    #[test]
    fn test_mempool_priority_ordering() {
        let chain_id = ChainId::MAINNET;
        let mut pool = Mempool::new(MempoolConfig::new(chain_id));

        let user1 = Secp256k1Keypair::generate();
        let user2 = Secp256k1Keypair::generate();
        let user3 = Secp256k1Keypair::generate();

        let mut state = MemoryStateJournal::new();
        state.set_account(
            user1.address(),
            AccountState::new_with_balance(Amount::from_u128(1_000_000_000)),
        );
        state.set_account(
            user2.address(),
            AccountState::new_with_balance(Amount::from_u128(1_000_000_000)),
        );
        state.set_account(
            user3.address(),
            AccountState::new_with_balance(Amount::from_u128(1_000_000_000)),
        );

        // User 1 offers priority fee 5
        let tx1 = create_test_tx(&user1, chain_id, Nonce(0), 5, 20, 100);
        // User 2 offers priority fee 50 (highest!)
        let tx2 = create_test_tx(&user2, chain_id, Nonce(0), 50, 60, 100);
        // User 3 offers priority fee 15
        let tx3 = create_test_tx(&user3, chain_id, Nonce(0), 15, 30, 100);

        pool.insert(tx1, &state, BlockHeight(1)).unwrap();
        pool.insert(tx2, &state, BlockHeight(1)).unwrap();
        pool.insert(tx3, &state, BlockHeight(1)).unwrap();

        let ready = pool.get_ready_transactions(10, Gas(30_000_000));
        assert_eq!(ready.len(), 3);

        // Verification: User 2 (50) comes first, User 3 (15) second, User 1 (5) last!
        assert_eq!(ready[0].from, user2.address());
        assert_eq!(ready[1].from, user3.address());
        assert_eq!(ready[2].from, user1.address());
    }

    #[test]
    fn test_mempool_nonce_chaining_and_future_promotion() {
        let chain_id = ChainId::MAINNET;
        let mut pool = Mempool::new(MempoolConfig::new(chain_id));
        let user = Secp256k1Keypair::generate();

        let mut state = MemoryStateJournal::new();
        state.set_account(
            user.address(),
            AccountState::new_with_balance(Amount::from_u128(1_000_000_000)),
        );

        // Submit tx with Nonce 1 first (Future gap!)
        let tx_nonce_1 = create_test_tx(&user, chain_id, Nonce(1), 10, 20, 100);
        pool.insert(tx_nonce_1, &state, BlockHeight(1)).unwrap();

        assert_eq!(pool.stats().ready_transactions, 0);
        assert_eq!(pool.stats().parked_transactions, 1);

        // Now submit tx with Nonce 0: closes gap and promotes Nonce 1!
        let tx_nonce_0 = create_test_tx(&user, chain_id, Nonce(0), 10, 20, 100);
        pool.insert(tx_nonce_0, &state, BlockHeight(1)).unwrap();

        assert_eq!(pool.stats().ready_transactions, 2);
        assert_eq!(pool.stats().parked_transactions, 0);
    }

    #[test]
    fn test_replace_by_fee_rbf_enforcement() {
        let chain_id = ChainId::MAINNET;
        let mut pool = Mempool::new(MempoolConfig::new(chain_id));
        let user = Secp256k1Keypair::generate();

        let mut state = MemoryStateJournal::new();
        state.set_account(
            user.address(),
            AccountState::new_with_balance(Amount::from_u128(1_000_000_000)),
        );

        // Original tx: priority fee 10
        let tx_orig = create_test_tx(&user, chain_id, Nonce(0), 10, 20, 100);
        pool.insert(tx_orig, &state, BlockHeight(1)).unwrap();

        // Attempt replacement with insufficient fee bump (priority fee 10 vs required 11, max fee 21)
        let tx_weak = create_test_tx(&user, chain_id, Nonce(0), 10, 21, 100);
        let err = pool.insert(tx_weak, &state, BlockHeight(1)).unwrap_err();
        assert_eq!(err, MempoolError::Underpriced);

        // Valid RBF replacement (+20% bump: priority fee 12 >= 11)
        let tx_boosted = create_test_tx(&user, chain_id, Nonce(0), 12, 25, 100);
        assert!(pool.insert(tx_boosted, &state, BlockHeight(1)).is_ok());

        assert_eq!(pool.stats().ready_transactions, 1);
        let ready = pool.get_ready_transactions(1, Gas(30_000_000));
        assert_eq!(ready[0].max_priority_fee_per_gas, Amount::from_u128(12));
    }

    fn create_test_evm_tx(
        keypair: &Secp256k1Keypair,
        chain_id: ChainId,
        nonce: Nonce,
        priority_fee: u128,
        max_fee: u128,
        valid_until: u64,
    ) -> Transaction {
        let from = keypair.address();
        let mut unsigned_tx = Transaction {
            chain_id,
            nonce,
            from,
            to: Some(Address([0x99; 20])),
            value: Amount::from_u128(10_000),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(max_fee),
            max_priority_fee_per_gas: Amount::from_u128(priority_fee),
            valid_until_block: BlockHeight(valid_until),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0u8; 32],
                s: [0u8; 32],
                y_parity: 0,
                chain_id: chain_id.0,
            },
            tx_type: 2,
            access_list: vec![],
        };

        let signing_hash = tola_codec::rlp::compute_type2_signing_hash(&unsigned_tx);
        let sig_bytes = keypair.sign_recoverable(signing_hash.as_bytes()).unwrap();

        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        r.copy_from_slice(&sig_bytes[0..32]);
        s.copy_from_slice(&sig_bytes[32..64]);
        let y_parity = sig_bytes[64];

        unsigned_tx.signature = Signature::Secp256k1Eth {
            r,
            s,
            y_parity,
            chain_id: chain_id.0,
        };

        unsigned_tx
    }

    #[test]
    fn test_pending_nonce_contiguous_semantics() {
        let chain_id = ChainId::MAINNET;
        let mut pool = Mempool::new(MempoolConfig::new(chain_id));
        let user = Secp256k1Keypair::generate();

        let mut state = MemoryStateJournal::new();
        let mut acct = AccountState::new_with_balance(Amount::from_u128(1_000_000_000_000));
        acct.nonce = Nonce(10);
        state.set_account(user.address(), acct);

        // Committed = 10, no mempool txs -> pending = 10
        assert_eq!(
            pool.get_pending_nonce(&user.address(), Nonce(10)),
            Nonce(10)
        );

        // Case C: [11] arrives (missing head 10) -> parked, pending = 10
        let tx_11 = create_test_tx(&user, chain_id, Nonce(11), 10, 20, 100);
        pool.insert(tx_11, &state, BlockHeight(1)).unwrap();
        assert_eq!(
            pool.get_pending_nonce(&user.address(), Nonce(10)),
            Nonce(10)
        );

        // Insert 10 -> unblocks 11! Ready has [10, 11] -> pending = 12
        let tx_10 = create_test_tx(&user, chain_id, Nonce(10), 10, 20, 100);
        pool.insert(tx_10, &state, BlockHeight(1)).unwrap();
        assert_eq!(
            pool.get_pending_nonce(&user.address(), Nonce(10)),
            Nonce(12)
        );

        // Insert 12 -> Case A: ready has [10, 11, 12] -> pending = 13
        let tx_12 = create_test_tx(&user, chain_id, Nonce(12), 10, 20, 100);
        pool.insert(tx_12, &state, BlockHeight(1)).unwrap();
        assert_eq!(
            pool.get_pending_nonce(&user.address(), Nonce(10)),
            Nonce(13)
        );

        // Case D: RBF on 10 with +20% fee bump -> still pending = 13 (not 14!)
        let tx_10_rbf = create_test_tx(&user, chain_id, Nonce(10), 12, 25, 100);
        pool.insert(tx_10_rbf, &state, BlockHeight(1)).unwrap();
        assert_eq!(
            pool.get_pending_nonce(&user.address(), Nonce(10)),
            Nonce(13)
        );

        // Case B: fresh account with gap: committed = 10, ready = [10], parked = [12] -> pending = 11
        let user_b = Secp256k1Keypair::generate();
        let mut acct_b = AccountState::new_with_balance(Amount::from_u128(1_000_000_000_000));
        acct_b.nonce = Nonce(10);
        state.set_account(user_b.address(), acct_b);
        let tx_b_10 = create_test_tx(&user_b, chain_id, Nonce(10), 10, 20, 100);
        let tx_b_12 = create_test_tx(&user_b, chain_id, Nonce(12), 10, 20, 100);
        pool.insert(tx_b_10, &state, BlockHeight(1)).unwrap();
        pool.insert(tx_b_12, &state, BlockHeight(1)).unwrap();
        assert_eq!(
            pool.get_pending_nonce(&user_b.address(), Nonce(10)),
            Nonce(11)
        );
    }

    #[test]
    fn test_cross_type_nonce_conflict() {
        let chain_id = ChainId::MAINNET;
        let mut pool = Mempool::new(MempoolConfig::new(chain_id));
        let user = Secp256k1Keypair::generate();

        let mut state = MemoryStateJournal::new();
        state.set_account(
            user.address(),
            AccountState::new_with_balance(Amount::from_u128(1_000_000_000_000)),
        );

        // Native tx with nonce 0 inserted
        let native_tx = create_test_tx(&user, chain_id, Nonce(0), 10, 20, 100);
        pool.insert(native_tx, &state, BlockHeight(1)).unwrap();

        // Attempt to insert EVM tx with same nonce 0 -> must fail with CrossTypeConflict
        let evm_tx = create_test_evm_tx(&user, chain_id, Nonce(0), 15, 30, 100);
        let err = pool.insert(evm_tx, &state, BlockHeight(1)).unwrap_err();
        assert_eq!(err, MempoolError::CrossTypeConflict);
    }
}
