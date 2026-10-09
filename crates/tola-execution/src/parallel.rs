//! Production Parallel Block Engine utilizing Block-STM optimistic concurrency.
//!
//! Orchestrates multi-threaded transaction execution against MV-Memory, validates
//! read sets, re-executes aborted transactions, and guarantees mathematical equivalence
//! to sequential execution with an automatic fallback mechanism.

use std::sync::Mutex;
use tola_core::{BlockEngine, BlockValidationError, StateDB, StateDelta, VMError};
use tola_types::*;

use crate::mv_memory::{MemoryKey, MemoryValue, MvMemory};
use crate::scheduler::{BlockStmScheduler, Task};
use crate::{calculate_intrinsic_gas, compute_transaction_id, SequentialBlockEngine};

#[derive(Clone, Debug)]
pub struct ParallelEngineConfig {
    pub num_workers: usize,
    pub max_aborts: usize,
}

impl Default for ParallelEngineConfig {
    fn default() -> Self {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        Self {
            num_workers: cpus.clamp(2, 16),
            max_aborts: 10_000,
        }
    }
}

pub struct ParallelBlockEngine {
    pub chain_id: ChainId,
    pub config: ParallelEngineConfig,
    sequential_fallback: SequentialBlockEngine,
}

impl ParallelBlockEngine {
    pub fn new(chain_id: ChainId, config: ParallelEngineConfig) -> Self {
        Self {
            chain_id,
            config,
            sequential_fallback: SequentialBlockEngine::new(chain_id),
        }
    }

    /// Execute a single transaction against MV-Memory speculatively.
    fn execute_speculative_tx(
        tx_index: usize,
        block: &Block,
        tx: &Transaction,
        mv: &MvMemory,
        base_state: &dyn StateDB,
    ) -> Result<(Vec<(MemoryKey, MemoryValue)>, Receipt), VMError> {
        mv.clear_read_set(tx_index);

        // 1. Read sender
        let sender_key = MemoryKey::Account(tx.from);
        let sender_val = mv
            .read(tx_index, &sender_key, base_state)
            .map_err(|_| VMError::Revert("Read failed".into()))?;
        let mut sender = match sender_val {
            MemoryValue::Account(Some(a)) => a,
            _ => AccountState::EMPTY,
        };

        // 2. Upfront gas reservation
        let upfront_gas_cost = tx
            .max_fee_per_gas
            .checked_mul(tx.gas_limit.as_u64())
            .ok_or(VMError::OutOfGas)?;

        sender
            .debit(upfront_gas_cost)
            .map_err(|_| VMError::OutOfGas)?;
        sender
            .increment_nonce()
            .map_err(|_| VMError::InvalidOpcode)?;

        // 3. Intrinsic gas calculation & refund
        let intrinsic_gas = calculate_intrinsic_gas(&tx.data);
        let gas_used = intrinsic_gas.min(tx.gas_limit);
        let effective_price = tx.effective_gas_price(block.header.base_fee_per_gas);
        let actual_gas_cost = effective_price
            .checked_mul(gas_used.as_u64())
            .ok_or(VMError::OutOfGas)?;

        let refund = upfront_gas_cost
            .checked_sub(actual_gas_cost)
            .ok_or(VMError::OutOfGas)?;

        sender.credit(refund).map_err(|_| VMError::InvalidOpcode)?;

        let mut dirty: std::collections::HashMap<Address, AccountState> =
            std::collections::HashMap::new();
        dirty.insert(tx.from, sender);

        // 4. Value transfer
        if tx.value > Amount::ZERO {
            if let Some(to_addr) = tx.to {
                if to_addr != tx.from {
                    let mut to_acct = match dirty.remove(&to_addr) {
                        Some(a) => a,
                        None => {
                            let to_key = MemoryKey::Account(to_addr);
                            match mv
                                .read(tx_index, &to_key, base_state)
                                .map_err(|_| VMError::Revert("Read failed".into()))?
                            {
                                MemoryValue::Account(Some(a)) => a,
                                _ => AccountState::EMPTY,
                            }
                        }
                    };
                    dirty
                        .get_mut(&tx.from)
                        .unwrap()
                        .debit(tx.value)
                        .map_err(|_| VMError::OutOfGas)?;
                    to_acct
                        .credit(tx.value)
                        .map_err(|_| VMError::InvalidOpcode)?;
                    dirty.insert(to_addr, to_acct);
                }
            } else {
                dirty
                    .get_mut(&tx.from)
                    .unwrap()
                    .debit(tx.value)
                    .map_err(|_| VMError::OutOfGas)?;
            }
        }

        // 5. Gas revenue split: 75% Proposer / 25% Protocol Treasury per approved Decisions D01 & D04
        let total_fee = actual_gas_cost.as_u128();
        let validator_fee_u128 = (total_fee * 75) / 100;
        let treasury_fee_u128 = total_fee.saturating_sub(validator_fee_u128);

        let validator_fee = Amount::from_u128(validator_fee_u128);
        let treasury_fee = Amount::from_u128(treasury_fee_u128);

        // Credit Validator Reward Pool
        if validator_fee > Amount::ZERO {
            let pool_addr = Address::VALIDATOR_REWARD_POOL;
            let mut pool_acct = match dirty.remove(&pool_addr) {
                Some(a) => a,
                None => {
                    let pool_key = MemoryKey::Account(pool_addr);
                    match mv
                        .read(tx_index, &pool_key, base_state)
                        .map_err(|_| VMError::Revert("Read failed".into()))?
                    {
                        MemoryValue::Account(Some(a)) => a,
                        _ => AccountState::EMPTY,
                    }
                }
            };
            pool_acct
                .credit(validator_fee)
                .map_err(|_| VMError::InvalidOpcode)?;
            dirty.insert(pool_addr, pool_acct);
        }

        // Credit Protocol Treasury
        if treasury_fee > Amount::ZERO {
            let treas_addr = Address::PROTOCOL_TREASURY;
            let mut treas_acct = match dirty.remove(&treas_addr) {
                Some(a) => a,
                None => {
                    let treas_key = MemoryKey::Account(treas_addr);
                    match mv
                        .read(tx_index, &treas_key, base_state)
                        .map_err(|_| VMError::Revert("Read failed".into()))?
                    {
                        MemoryValue::Account(Some(a)) => a,
                        _ => AccountState::EMPTY,
                    }
                }
            };
            treas_acct
                .credit(treasury_fee)
                .map_err(|_| VMError::InvalidOpcode)?;
            dirty.insert(treas_addr, treas_acct);
        }

        // 6. Assemble writes
        let writes = dirty
            .into_iter()
            .map(|(addr, acct)| (MemoryKey::Account(addr), MemoryValue::Account(Some(acct))))
            .collect();

        let tx_id = compute_transaction_id(tx);
        let receipt = Receipt {
            tx_id,
            success: true,
            gas_used,
            cumulative_gas_used: Gas::ZERO,
            logs: vec![],
            return_data: vec![],
        };

        Ok((writes, receipt))
    }
}

impl BlockEngine for ParallelBlockEngine {
    fn validate_block(
        &self,
        block: &Block,
        state: &dyn StateDB,
    ) -> Result<(), BlockValidationError> {
        self.sequential_fallback.validate_block(block, state)
    }

    fn execute_block(
        &self,
        block: &Block,
        state: &mut dyn StateDB,
    ) -> Result<Vec<Receipt>, VMError> {
        let num_txs = block.transactions.len();
        if num_txs == 0 {
            return Ok(vec![]);
        }

        // If block has contract calls or very few transactions, running sequentially avoids thread overhead
        let has_contract_call = block
            .transactions
            .iter()
            .any(|tx| tx.to == Some(EXTERNAL_ASSET_CONTRACT_ADDRESS) || tx.is_evm());
        if num_txs <= 2 || self.config.num_workers <= 1 || has_contract_call {
            return self.sequential_fallback.execute_block(block, state);
        }

        let mv = MvMemory::new();
        let scheduler = BlockStmScheduler::new(num_txs, self.config.max_aborts);
        let receipts_slots: Vec<Mutex<Option<Receipt>>> =
            (0..num_txs).map(|_| Mutex::new(None)).collect();

        // Multi-threaded speculative execution wave
        std::thread::scope(|s| {
            for _ in 0..self.config.num_workers {
                s.spawn(|| {
                    while let Some(task) = scheduler.pop_task() {
                        match task {
                            Task::Execution(tx_idx, inc) => {
                                let tx = &block.transactions[tx_idx];
                                mv.clear_read_set(tx_idx);
                                match Self::execute_speculative_tx(tx_idx, block, tx, &mv, state) {
                                    Ok((writes, receipt)) => {
                                        mv.record_writes(tx_idx, inc, writes);
                                        *receipts_slots[tx_idx].lock().unwrap() = Some(receipt);
                                        scheduler.finish_execution(tx_idx, inc);
                                    }
                                    Err(_) => {
                                        let accepted =
                                            scheduler.finish_validation(tx_idx, inc, false);
                                        if accepted {
                                            mv.mark_estimates(tx_idx, inc);
                                        }
                                    }
                                }
                            }
                            Task::Validation(tx_idx, inc) => {
                                let valid = mv.validate_read_set(tx_idx);
                                let accepted = scheduler.finish_validation(tx_idx, inc, valid);
                                if !valid && accepted {
                                    mv.mark_estimates(tx_idx, inc);
                                }
                            }
                        }
                    }
                });
            }
        });

        // If conflict rate exceeded safety threshold, safely fallback to deterministic sequential execution
        if scheduler.is_fallback_required() {
            return self.sequential_fallback.execute_block(block, state);
        }

        // Apply final committed state to StateDB
        let committed = match mv.extract_committed_state(num_txs) {
            Ok(c) => c,
            Err(_) => return self.sequential_fallback.execute_block(block, state),
        };
        let mut account_deltas = Vec::new();
        let mut storage_deltas = Vec::new();

        for (key, val) in committed {
            match (key, val) {
                (MemoryKey::Account(addr), MemoryValue::Account(acct)) => {
                    account_deltas.push((addr, acct));
                }
                (MemoryKey::Storage(addr, slot), MemoryValue::Storage(val)) => {
                    storage_deltas.push(((addr, slot), val));
                }
                _ => {}
            }
        }

        state
            .apply_delta(StateDelta {
                accounts: account_deltas,
                storage: storage_deltas,
                code: vec![],
            })
            .map_err(|_| VMError::Revert("Failed to commit parallel state delta".into()))?;

        // Collect receipts in strict canonical transaction order
        let mut receipts = Vec::with_capacity(num_txs);
        let mut cumulative_gas = Gas::ZERO;

        for i in 0..num_txs {
            let mut r = receipts_slots[i]
                .lock()
                .unwrap()
                .take()
                .ok_or(VMError::Revert("Missing execution receipt".into()))?;
            cumulative_gas = cumulative_gas
                .checked_add(r.gas_used)
                .ok_or(VMError::OutOfGas)?;
            r.cumulative_gas_used = cumulative_gas;
            receipts.push(r);
        }

        Ok(receipts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compute_tx_signing_digest;
    use tola_crypto::Secp256k1Keypair;
    use tola_state::MemoryStateJournal;

    fn create_signed_tx(
        keypair: &Secp256k1Keypair,
        chain_id: ChainId,
        nonce: Nonce,
        to: Option<Address>,
        value: Amount,
        gas_limit: Gas,
        max_fee: Amount,
        priority_fee: Amount,
        epoch: u64,
    ) -> Transaction {
        let from = keypair.address();
        let mut unsigned_tx = Transaction {
            chain_id,
            nonce,
            from,
            to,
            value,
            gas_limit,
            max_fee_per_gas: max_fee,
            max_priority_fee_per_gas: priority_fee,
            valid_until_block: BlockHeight(100),
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

        let digest = compute_tx_signing_digest(&unsigned_tx, epoch);
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
    fn test_parallel_independent_transfers_match_sequential_state() {
        let proposer_addr = Address([0x99; 20]);
        let mut seq_state = MemoryStateJournal::new();
        let mut par_state = MemoryStateJournal::new();

        let mut transactions = Vec::new();
        let num_txs = 10;

        for i in 0..num_txs {
            let sender = Secp256k1Keypair::generate();
            let recipient = Address([i as u8 + 1; 20]);
            let initial_bal = Amount::from_u128(10_000_000);

            seq_state.set_account(
                sender.address(),
                AccountState::new_with_balance(initial_bal),
            );
            par_state.set_account(
                sender.address(),
                AccountState::new_with_balance(initial_bal),
            );

            let tx = create_signed_tx(
                &sender,
                ChainId::MAINNET,
                Nonce::ZERO,
                Some(recipient),
                Amount::from_u128(1_000),
                Gas(21_000),
                Amount::from_u128(10),
                Amount::from_u128(2),
                0,
            );
            transactions.push(tx);
        }

        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(0),
                timestamp: Timestamp(1000),
                proposer: proposer_addr,
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(21_000 * num_txs as u64),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(5),
                extra_data: vec![],
            },
            transactions,
            last_commit_qc: None,
        };

        // 1. Execute sequentially
        let seq_engine = SequentialBlockEngine::new(ChainId::MAINNET);
        let seq_receipts = seq_engine.execute_block(&block, &mut seq_state).unwrap();

        // 2. Execute in parallel with 4 threads
        let par_engine = ParallelBlockEngine::new(
            ChainId::MAINNET,
            ParallelEngineConfig {
                num_workers: 4,
                max_aborts: 100,
            },
        );
        let par_receipts = par_engine.execute_block(&block, &mut par_state).unwrap();

        // 3. Assert receipts equivalence
        assert_eq!(seq_receipts.len(), par_receipts.len());
        for (sr, pr) in seq_receipts.iter().zip(par_receipts.iter()) {
            assert_eq!(sr.tx_id, pr.tx_id);
            assert_eq!(sr.success, pr.success);
            assert_eq!(sr.gas_used, pr.gas_used);
            assert_eq!(sr.cumulative_gas_used, pr.cumulative_gas_used);
        }

        // 4. Assert exact state root & balances equivalence
        assert_eq!(
            seq_state.compute_state_root().unwrap(),
            par_state.compute_state_root().unwrap()
        );
    }

    #[test]
    fn test_parallel_dependent_transfer_chain_aborts_and_converges() {
        // Transfer chain: User 0 -> User 1 -> User 2 -> User 3
        let kp0 = Secp256k1Keypair::generate();
        let kp1 = Secp256k1Keypair::generate();
        let kp2 = Secp256k1Keypair::generate();
        let kp3 = Secp256k1Keypair::generate();

        let proposer_addr = Address([0x99; 20]);
        let mut seq_state = MemoryStateJournal::new();
        let mut par_state = MemoryStateJournal::new();

        // Only User 0 has initial balance
        let initial_bal = Amount::from_u128(1_000_000_000);
        seq_state.set_account(kp0.address(), AccountState::new_with_balance(initial_bal));
        par_state.set_account(kp0.address(), AccountState::new_with_balance(initial_bal));

        // User 0 sends to User 1
        let tx0 = create_signed_tx(
            &kp0,
            ChainId::MAINNET,
            Nonce::ZERO,
            Some(kp1.address()),
            Amount::from_u128(500_000_000),
            Gas(21_000),
            Amount::from_u128(10),
            Amount::from_u128(2),
            0,
        );

        // User 1 sends to User 2 (dependent on User 1 receiving funds from tx0!)
        let tx1 = create_signed_tx(
            &kp1,
            ChainId::MAINNET,
            Nonce::ZERO,
            Some(kp2.address()),
            Amount::from_u128(200_000_000),
            Gas(21_000),
            Amount::from_u128(10),
            Amount::from_u128(2),
            0,
        );

        // User 2 sends to User 3 (dependent on tx1!)
        let tx2 = create_signed_tx(
            &kp2,
            ChainId::MAINNET,
            Nonce::ZERO,
            Some(kp3.address()),
            Amount::from_u128(100_000_000),
            Gas(21_000),
            Amount::from_u128(10),
            Amount::from_u128(2),
            0,
        );

        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(0),
                timestamp: Timestamp(1000),
                proposer: proposer_addr,
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(63_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(5),
                extra_data: vec![],
            },
            transactions: vec![tx0, tx1, tx2],
            last_commit_qc: None,
        };

        let seq_engine = SequentialBlockEngine::new(ChainId::MAINNET);
        let seq_receipts = seq_engine.execute_block(&block, &mut seq_state).unwrap();

        let par_engine = ParallelBlockEngine::new(
            ChainId::MAINNET,
            ParallelEngineConfig {
                num_workers: 4,
                max_aborts: 100,
            },
        );
        let par_receipts = par_engine.execute_block(&block, &mut par_state).unwrap();

        assert_eq!(seq_receipts.len(), par_receipts.len());
        assert_eq!(
            seq_state.compute_state_root().unwrap(),
            par_state.compute_state_root().unwrap()
        );
    }

    #[test]
    fn test_parallel_fallback_on_excessive_aborts() {
        let kp0 = Secp256k1Keypair::generate();
        let kp1 = Secp256k1Keypair::generate();
        let proposer_addr = Address([0x99; 20]);
        let mut seq_state = MemoryStateJournal::new();
        let mut par_state = MemoryStateJournal::new();

        let initial_bal = Amount::from_u128(1_000_000_000);
        seq_state.set_account(kp0.address(), AccountState::new_with_balance(initial_bal));
        par_state.set_account(kp0.address(), AccountState::new_with_balance(initial_bal));

        let tx0 = create_signed_tx(
            &kp0,
            ChainId::MAINNET,
            Nonce::ZERO,
            Some(kp1.address()),
            Amount::from_u128(100),
            Gas(21_000),
            Amount::from_u128(10),
            Amount::from_u128(2),
            0,
        );
        let tx1 = create_signed_tx(
            &kp0,
            ChainId::MAINNET,
            Nonce(1),
            Some(kp1.address()),
            Amount::from_u128(100),
            Gas(21_000),
            Amount::from_u128(10),
            Amount::from_u128(2),
            0,
        );
        let tx2 = create_signed_tx(
            &kp0,
            ChainId::MAINNET,
            Nonce(2),
            Some(kp1.address()),
            Amount::from_u128(100),
            Gas(21_000),
            Amount::from_u128(10),
            Amount::from_u128(2),
            0,
        );

        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(0),
                timestamp: Timestamp(1000),
                proposer: proposer_addr,
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(63_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(5),
                extra_data: vec![],
            },
            transactions: vec![tx0, tx1, tx2],
            last_commit_qc: None,
        };

        let seq_engine = SequentialBlockEngine::new(ChainId::MAINNET);
        let seq_receipts = seq_engine.execute_block(&block, &mut seq_state).unwrap();

        // Configure engine with max_aborts: 1 to force fallback trigger upon conflict
        let par_engine = ParallelBlockEngine::new(
            ChainId::MAINNET,
            ParallelEngineConfig {
                num_workers: 4,
                max_aborts: 1,
            },
        );
        let par_receipts = par_engine.execute_block(&block, &mut par_state).unwrap();

        assert_eq!(seq_receipts.len(), par_receipts.len());
        assert_eq!(
            seq_state.compute_state_root().unwrap(),
            par_state.compute_state_root().unwrap()
        );
    }

    #[test]
    fn test_parallel_cyclic_transfers_match_sequential_state() {
        let num_users = 10;
        let mut keypairs = Vec::new();
        let mut seq_state = MemoryStateJournal::new();
        let mut par_state = MemoryStateJournal::new();
        let initial_bal = Amount::from_u128(50_000 * 1_000_000_000_000_000_000);

        for _ in 0..num_users {
            let kp = Secp256k1Keypair::generate();
            seq_state.set_account(kp.address(), AccountState::new_with_balance(initial_bal));
            par_state.set_account(kp.address(), AccountState::new_with_balance(initial_bal));
            keypairs.push(kp);
        }

        let mut txs = Vec::new();
        for s in 0..num_users {
            let recipient = keypairs[(s + 1) % num_users].address();
            let tx = create_signed_tx(
                &keypairs[s],
                ChainId::TESTNET,
                Nonce::ZERO,
                Some(recipient),
                Amount::from_u128(10 * 1_000_000_000_000_000_000),
                Gas(21_000),
                Amount::from_u128(1_000_000_000),
                Amount::from_u128(1_000_000_000),
                0,
            );
            txs.push(tx);
        }

        let proposer_addr = Address([0x01; 20]);
        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(0),
                timestamp: Timestamp(1000),
                proposer: proposer_addr,
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(210_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(1_000_000_000),
                extra_data: vec![],
            },
            transactions: txs,
            last_commit_qc: None,
        };

        let seq_engine = SequentialBlockEngine::new(ChainId::TESTNET);
        let seq_receipts = seq_engine.execute_block(&block, &mut seq_state).unwrap();

        let par_engine = ParallelBlockEngine::new(
            ChainId::TESTNET,
            ParallelEngineConfig {
                num_workers: 16,
                max_aborts: 5000,
            },
        );
        let par_receipts = par_engine.execute_block(&block, &mut par_state).unwrap();

        assert_eq!(seq_receipts.len(), par_receipts.len());
        for i in 0..num_users {
            let seq_acct = seq_state
                .get_account(&keypairs[i].address())
                .unwrap()
                .unwrap();
            let par_acct = par_state
                .get_account(&keypairs[i].address())
                .unwrap()
                .unwrap();
            assert_eq!(
                seq_acct.balance, par_acct.balance,
                "User {} balance mismatch: seq={:?}, par={:?}",
                i, seq_acct.balance, par_acct.balance
            );
            assert_eq!(
                seq_acct.nonce, par_acct.nonce,
                "User {} nonce mismatch: seq={:?}, par={:?}",
                i, seq_acct.nonce, par_acct.nonce
            );
        }
        assert_eq!(
            seq_state.compute_state_root().unwrap(),
            par_state.compute_state_root().unwrap()
        );
    }
}
