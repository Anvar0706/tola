#![allow(clippy::all)]
//! Production Block-STM parallel execution scheduler and sequential state transition engine.
//!
//! Complies with ADR-0012, EIP-1559 fee economics, and State Transition Model v0.1:
//! - Upfront gas reservations and exact refunds
//! - Nonce monotonicity and balance non-negativity guarantees
//! - Cryptographic Secp256k1 signature validation against tx.from
//! - Mathematical total supply conservation invariant

pub mod evm;
pub mod evm_adapter;
pub mod mv_memory;
pub mod parallel;
pub mod scheduler;
pub mod system_contracts;

pub use mv_memory::{MemoryEntry, MemoryKey, MemoryValue, MvMemory, Version};
pub use parallel::{ParallelBlockEngine, ParallelEngineConfig};
pub use revm;
pub use scheduler::{BlockStmScheduler, Task, TxStatus};
pub use system_contracts::{
    ContractExecutionResult, ExternalAssetCall, ExternalAssetContractExecutor,
    ExternalAssetStorage, TOPIC_APPROVAL, TOPIC_APPROVAL_FOR_ALL, TOPIC_EXTERNAL_ASSET_MINTED,
    TOPIC_TRANSFER,
};

use std::collections::HashMap;
use tola_codec::{serialize_unsigned_transaction, CanonicalSerialize};
use tola_core::{BlockEngine, BlockValidationError, StateDB, StateDelta, VMError};
use tola_crypto::{
    blake3_hash, domain_separated_hash, recover_secp256k1_address, DOMAIN_TRANSACTION_PAYLOAD,
};
use tola_types::*;

/// Compute domain-separated cryptographic signing digest for a transaction.
pub fn compute_tx_signing_digest(tx: &Transaction, epoch: u64) -> Hash {
    let mut unsigned_bytes = Vec::new();
    serialize_unsigned_transaction(tx, &mut unsigned_bytes);
    let payload_hash = blake3_hash(&unsigned_bytes);
    domain_separated_hash(
        DOMAIN_TRANSACTION_PAYLOAD,
        tx.chain_id.0,
        epoch,
        &payload_hash,
    )
}

/// Compute canonical transaction identifier (BLAKE3 hash of complete serialized envelope).
pub fn compute_transaction_id(tx: &Transaction) -> TransactionId {
    let wire_bytes = tx.to_canonical_bytes();
    TransactionId(blake3_hash(&wire_bytes))
}

/// Calculate standard intrinsic gas for a transaction (21,000 base + 16/4 per calldata byte).
pub fn calculate_intrinsic_gas(data: &[u8]) -> Gas {
    let mut gas: u64 = 21_000;
    for &b in data {
        if b == 0 {
            gas = gas.saturating_add(4);
        } else {
            gas = gas.saturating_add(16);
        }
    }
    Gas(gas)
}

pub struct SequentialBlockEngine {
    pub chain_id: ChainId,
}

impl SequentialBlockEngine {
    pub fn new(chain_id: ChainId) -> Self {
        Self { chain_id }
    }

    /// Perform stateless and signature verification on a transaction.
    pub fn verify_stateless(
        &self,
        tx: &Transaction,
        max_block_gas: Gas,
        epoch: u64,
    ) -> Result<(), BlockValidationError> {
        tx.stateless_validate(self.chain_id, max_block_gas)
            .map_err(|_| BlockValidationError::GasLimitExceeded)?;

        // Verify cryptographic signature if Secp256k1
        match &tx.signature {
            Signature::Secp256k1 { r, s, v } => {
                let mut sig_bytes = [0u8; 65];
                sig_bytes[0..32].copy_from_slice(r);
                sig_bytes[32..64].copy_from_slice(s);
                sig_bytes[64] = *v;

                let digest = compute_tx_signing_digest(tx, epoch);
                let recovered = recover_secp256k1_address(digest.as_bytes(), &sig_bytes)
                    .map_err(|_| BlockValidationError::InvalidSignature)?;

                if recovered != tx.from {
                    return Err(BlockValidationError::InvalidSignature);
                }
            }
            Signature::Secp256k1Eth {
                r,
                s,
                y_parity,
                chain_id,
            } => {
                if *chain_id != self.chain_id.0 {
                    return Err(BlockValidationError::InvalidSignature);
                }
                let mut sig_bytes = [0u8; 65];
                sig_bytes[0..32].copy_from_slice(r);
                sig_bytes[32..64].copy_from_slice(s);
                sig_bytes[64] = *y_parity;

                let recovered = match tx.tx_type {
                    1 => {
                        let hash_type1 = tola_codec::rlp::compute_type1_signing_hash(tx);
                        recover_secp256k1_address(hash_type1.as_bytes(), &sig_bytes)
                    }
                    0 => {
                        let hash_legacy = tola_codec::rlp::compute_legacy_signing_hash(tx);
                        recover_secp256k1_address(hash_legacy.as_bytes(), &sig_bytes)
                    }
                    _ => {
                        let hash_type2 = tola_codec::rlp::compute_type2_signing_hash(tx);
                        recover_secp256k1_address(hash_type2.as_bytes(), &sig_bytes)
                            .or_else(|_| {
                                let hash_type1 = tola_codec::rlp::compute_type1_signing_hash(tx);
                                recover_secp256k1_address(hash_type1.as_bytes(), &sig_bytes)
                            })
                            .or_else(|_| {
                                let hash_legacy = tola_codec::rlp::compute_legacy_signing_hash(tx);
                                recover_secp256k1_address(hash_legacy.as_bytes(), &sig_bytes)
                            })
                    }
                }
                .map_err(|_| BlockValidationError::InvalidSignature)?;

                if recovered != tx.from {
                    return Err(BlockValidationError::InvalidSignature);
                }
            }
            Signature::Ed25519(_) => {
                // Reserved for system/validator operations
            }
        }

        Ok(())
    }
}

impl BlockEngine for SequentialBlockEngine {
    fn validate_block(
        &self,
        block: &Block,
        state: &dyn StateDB,
    ) -> Result<(), BlockValidationError> {
        if block.header.height.0 == 0 {
            return Ok(()); // Genesis block
        }

        let mut cumulative_gas: u64 = 0;
        let mut simulated_nonces: std::collections::HashMap<Address, Nonce> =
            std::collections::HashMap::new();

        for tx in &block.transactions {
            // 1. Stateless validation
            self.verify_stateless(tx, block.header.gas_limit, block.header.epoch.0)?;

            // 2. Base fee validation for EVM transactions (EIP-1559)
            if tx.is_evm() && tx.max_fee_per_gas < block.header.base_fee_per_gas {
                return Err(BlockValidationError::FeeBelowBaseFee);
            }

            // 3. Gas limit bounds
            cumulative_gas = cumulative_gas.saturating_add(tx.gas_limit.as_u64());
            if cumulative_gas > block.header.gas_limit.as_u64() {
                return Err(BlockValidationError::GasLimitExceeded);
            }

            // 4. Stateful validation
            let acct = state
                .get_account(&tx.from)
                .map_err(|_| BlockValidationError::InvalidStateRoot)?;
            let current_acct = acct.unwrap_or(AccountState::EMPTY);

            let expected_nonce = simulated_nonces
                .get(&tx.from)
                .copied()
                .unwrap_or(current_acct.nonce);
            if tx.nonce != expected_nonce {
                return Err(BlockValidationError::NonceMismatch);
            }

            if !current_acct.can_afford(tx.gas_limit, tx.max_fee_per_gas, tx.value) {
                return Err(BlockValidationError::InsufficientBalance);
            }

            simulated_nonces.insert(tx.from, expected_nonce.next());
        }

        Ok(())
    }

    fn execute_block(
        &self,
        block: &Block,
        state: &mut dyn StateDB,
    ) -> Result<Vec<Receipt>, VMError> {
        let mut receipts = Vec::with_capacity(block.transactions.len());
        let mut cumulative_gas = Gas::ZERO;
        let base_fee = block.header.base_fee_per_gas;

        for tx in &block.transactions {
            let tx_id = compute_transaction_id(tx);

            if tx.is_evm() {
                let outcome = crate::evm::EvmExecutor::execute_transaction(
                    tx,
                    block.header.height,
                    block.header.timestamp,
                    base_fee,
                    block.header.proposer,
                    block.header.gas_limit,
                    state,
                )
                .map_err(|e| VMError::Revert(e.into_bytes()))?;

                // Gas fee distribution per Approved Decisions D01 & D04:
                // 75% to Validator Reward Pool, 25% to Protocol Treasury.
                // 0% base-fee burn, 0% directly to proposer.
                let effective_price = tx.effective_gas_price(base_fee);
                let actual_gas_cost = effective_price
                    .checked_mul(outcome.gas_used.as_u64())
                    .ok_or(VMError::OutOfGas)?;

                let validator_fee = Amount((actual_gas_cost.0.checked_mul(75).unwrap_or(0)) / 100);
                let treasury_fee = Amount(actual_gas_cost.0.saturating_sub(validator_fee.0));

                let validator_pool_addr = Address::VALIDATOR_REWARD_POOL;
                let treasury_addr = Address::PROTOCOL_TREASURY;

                let mut dirty_accounts: HashMap<Address, AccountState> = HashMap::new();

                if validator_fee > Amount::ZERO {
                    let mut pool_acct = state
                        .get_account(&validator_pool_addr)
                        .map_err(|_| {
                            VMError::Revert("Failed to load validator reward pool".into())
                        })?
                        .unwrap_or(AccountState::EMPTY);
                    pool_acct
                        .credit(validator_fee)
                        .map_err(|_| VMError::InvalidOpcode)?;
                    dirty_accounts.insert(validator_pool_addr, pool_acct);
                }

                if treasury_fee > Amount::ZERO {
                    let mut treas_acct = state
                        .get_account(&treasury_addr)
                        .map_err(|_| VMError::Revert("Failed to load protocol treasury".into()))?
                        .unwrap_or(AccountState::EMPTY);
                    treas_acct
                        .credit(treasury_fee)
                        .map_err(|_| VMError::InvalidOpcode)?;
                    dirty_accounts.insert(treasury_addr, treas_acct);
                }

                if !dirty_accounts.is_empty() {
                    let mut account_deltas: Vec<(Address, Option<AccountState>)> = dirty_accounts
                        .into_iter()
                        .map(|(addr, state)| (addr, Some(state)))
                        .collect();
                    account_deltas.sort_by_key(|(addr, _)| *addr);

                    state
                        .apply_delta(StateDelta {
                            accounts: account_deltas,
                            storage: vec![],
                            code: vec![],
                        })
                        .map_err(|_| {
                            VMError::Revert("Failed to apply fee distribution delta".into())
                        })?;
                }

                cumulative_gas = cumulative_gas
                    .checked_add(outcome.gas_used)
                    .ok_or(VMError::OutOfGas)?;

                receipts.push(Receipt {
                    tx_id,
                    success: outcome.success,
                    gas_used: outcome.gas_used,
                    cumulative_gas_used: cumulative_gas,
                    logs: outcome.logs,
                    return_data: outcome.return_data,
                });
                continue;
            }

            // Fetch sender
            let mut sender = state
                .get_account(&tx.from)
                .map_err(|_| VMError::Revert("Failed to load sender".into()))?
                .unwrap_or(AccountState::EMPTY);

            // Upfront gas reservation: gas_limit * max_fee_per_gas
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

            let intrinsic_gas = calculate_intrinsic_gas(&tx.data);

            // Check if calling external asset system contract
            let is_system_contract = tx.to == Some(EXTERNAL_ASSET_CONTRACT_ADDRESS);

            let (contract_exec_res, actual_gas_consumed) = if is_system_contract {
                let res = ExternalAssetContractExecutor::execute(
                    tx,
                    block.header.height,
                    block.header.timestamp,
                    state,
                );
                let total_gas = Gas(intrinsic_gas.0.saturating_add(res.gas_consumed.0));
                (Some(res), total_gas)
            } else {
                (None, intrinsic_gas)
            };

            let gas_used = actual_gas_consumed.min(tx.gas_limit);

            // Calculate effective gas price and refund
            let effective_price = tx.effective_gas_price(base_fee);
            let actual_gas_cost = effective_price
                .checked_mul(gas_used.as_u64())
                .ok_or(VMError::OutOfGas)?;

            let refund = upfront_gas_cost
                .checked_sub(actual_gas_cost)
                .ok_or(VMError::OutOfGas)?;

            sender.credit(refund).map_err(|_| VMError::InvalidOpcode)?;

            // Execute value transfer
            let mut recipient_opt = None;
            if tx.value > Amount::ZERO {
                if let Some(to_addr) = tx.to {
                    if to_addr == tx.from {
                        // Self-transfer: balance debit and credit cancel out; nonce and gas are already handled
                    } else {
                        sender.debit(tx.value).map_err(|_| VMError::OutOfGas)?;

                        let mut recipient = state
                            .get_account(&to_addr)
                            .map_err(|_| VMError::Revert("Failed to load recipient".into()))?
                            .unwrap_or(AccountState::EMPTY);

                        recipient
                            .credit(tx.value)
                            .map_err(|_| VMError::InvalidOpcode)?;
                        recipient_opt = Some((to_addr, recipient));
                    }
                } else {
                    // Contract creation with value
                    sender.debit(tx.value).map_err(|_| VMError::OutOfGas)?;
                }
            }

            // Approved Decisions D01 & D04: Gas fee revenue is split 75% to validator pool,
            // 25% to Protocol Treasury. Zero base fee is burned; total token supply is strictly conserved.
            let validator_fee = Amount((actual_gas_cost.0.checked_mul(75).unwrap_or(0)) / 100);
            let treasury_fee = Amount(actual_gas_cost.0.saturating_sub(validator_fee.0));

            let treasury_addr = Address::PROTOCOL_TREASURY;
            let validator_pool_addr = Address::VALIDATOR_REWARD_POOL;

            // Map of dirty accounts modified in this transaction state transition
            let mut dirty_accounts: HashMap<Address, AccountState> = HashMap::new();
            dirty_accounts.insert(tx.from, sender);

            if let Some((to_addr, recipient)) = recipient_opt {
                dirty_accounts.insert(to_addr, recipient);
            }

            // Credit 75% of gas fee to Validator Reward Pool
            if validator_fee > Amount::ZERO {
                let mut pool_acct =
                    if let Some(existing) = dirty_accounts.remove(&validator_pool_addr) {
                        existing
                    } else {
                        state
                            .get_account(&validator_pool_addr)
                            .map_err(|_| {
                                VMError::Revert("Failed to load validator reward pool".into())
                            })?
                            .unwrap_or(AccountState::EMPTY)
                    };
                pool_acct
                    .credit(validator_fee)
                    .map_err(|_| VMError::InvalidOpcode)?;
                dirty_accounts.insert(validator_pool_addr, pool_acct);
            }

            // Credit 25% of gas fee to Protocol Treasury
            if treasury_fee > Amount::ZERO {
                let mut treas = if let Some(existing) = dirty_accounts.remove(&treasury_addr) {
                    existing
                } else {
                    state
                        .get_account(&treasury_addr)
                        .map_err(|_| VMError::Revert("Failed to load treasury".into()))?
                        .unwrap_or(AccountState::EMPTY)
                };
                treas
                    .credit(treasury_fee)
                    .map_err(|_| VMError::InvalidOpcode)?;
                dirty_accounts.insert(treasury_addr, treas);
            }

            // Apply account deltas sorted deterministically by Address
            let mut account_deltas: Vec<(Address, Option<AccountState>)> = dirty_accounts
                .into_iter()
                .map(|(addr, state)| (addr, Some(state)))
                .collect();
            account_deltas.sort_by_key(|(addr, _)| *addr);

            let (success, logs, return_data, storage_deltas) = match contract_exec_res {
                Some(res) => (
                    res.success,
                    res.logs,
                    res.return_data,
                    if res.success {
                        res.storage_deltas
                    } else {
                        vec![]
                    },
                ),
                None => (true, vec![], vec![], vec![]),
            };

            state
                .apply_delta(StateDelta {
                    accounts: account_deltas,
                    storage: storage_deltas,
                    code: vec![],
                })
                .map_err(|_| VMError::Revert("Failed to apply state delta".into()))?;

            cumulative_gas = cumulative_gas
                .checked_add(gas_used)
                .ok_or(VMError::OutOfGas)?;

            receipts.push(Receipt {
                tx_id,
                success,
                gas_used,
                cumulative_gas_used: cumulative_gas,
                logs,
                return_data,
            });
        }

        Ok(receipts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_signed_transaction_state_transition_and_conservation() {
        let sender_key = Secp256k1Keypair::generate();
        let sender_addr = sender_key.address();

        let recipient_key = Secp256k1Keypair::generate();
        let recipient_addr = recipient_key.address();

        let proposer_addr = Address([0x99; 20]);

        let initial_sender_balance = Amount::from_u128(1_000_000_000);
        let mut state = MemoryStateJournal::new();
        state.set_account(
            sender_addr,
            AccountState::new_with_balance(initial_sender_balance),
        );

        let tx = create_signed_tx(
            &sender_key,
            ChainId::MAINNET,
            Nonce::ZERO,
            Some(recipient_addr),
            Amount::from_u128(100_000),
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
                gas_used: Gas(21_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(5),
                extra_data: vec![],
            },
            transactions: vec![tx.clone()],
            last_commit_qc: None,
        };

        let engine = SequentialBlockEngine::new(ChainId::MAINNET);

        // 1. Validation check
        assert!(engine.validate_block(&block, &state).is_ok());

        // 2. Execution check
        let receipts = engine
            .execute_block(&block, &mut state)
            .expect("Block execution succeeds");
        assert_eq!(receipts.len(), 1);
        assert!(receipts[0].success);
        assert_eq!(receipts[0].gas_used, Gas(21_000));
        assert_ne!(receipts[0].tx_id, TransactionId::ZERO);

        // 3. Balance verification
        let sender_final = state.get_account(&sender_addr).unwrap().unwrap();
        let recipient_final = state.get_account(&recipient_addr).unwrap().unwrap();
        let pool_final = state
            .get_account(&Address::VALIDATOR_REWARD_POOL)
            .unwrap()
            .unwrap();
        let treasury_final = state
            .get_account(&Address::PROTOCOL_TREASURY)
            .unwrap()
            .unwrap();

        assert_eq!(sender_final.nonce, Nonce(1));
        assert_eq!(recipient_final.balance, Amount::from_u128(100_000));

        // Economics breakdown per Decisions D01 & D04:
        // BaseFee = 5, Tip = 2 -> EffectivePrice = 7
        // GasUsed = 21,000
        // ActualGasCost = 21,000 * 7 = 147,000
        // ValidatorPoolFee = 147,000 * 75% = 110,250
        // TreasuryFee = 147,000 * 25% = 36,750
        // Value = 100,000
        // Total deducted from sender = 100,000 + 147,000 = 247,000
        assert_eq!(
            sender_final.balance,
            Amount::from_u128(1_000_000_000 - 247_000)
        );
        assert_eq!(pool_final.balance, Amount::from_u128(110_250));
        assert_eq!(treasury_final.balance, Amount::from_u128(36_750));
        assert_eq!(
            state
                .get_account(&proposer_addr)
                .unwrap()
                .unwrap_or(AccountState::EMPTY)
                .balance,
            Amount::ZERO,
            "Proposer must receive 0 directly from gas fees"
        );

        // 4. Supply conservation check:
        // SenderFinal + RecipientFinal + PoolFinal + TreasuryFinal == InitialSupply
        let total_accounted = sender_final.balance.as_u128()
            + recipient_final.balance.as_u128()
            + pool_final.balance.as_u128()
            + treasury_final.balance.as_u128();

        assert_eq!(
            total_accounted,
            initial_sender_balance.as_u128(),
            "Total supply conservation invariant must hold exactly!"
        );
    }

    #[test]
    fn test_signature_mismatch_rejected() {
        let sender_key = Secp256k1Keypair::generate();
        let imposter_key = Secp256k1Keypair::generate();

        let tx = create_signed_tx(
            &imposter_key, // Imposter signs, but claims sender_key.address()
            ChainId::MAINNET,
            Nonce::ZERO,
            Some(Address([2u8; 20])),
            Amount::from_u128(1_000),
            Gas(21_000),
            Amount::from_u128(10),
            Amount::from_u128(2),
            0,
        );

        let mut forge_tx = tx.clone();
        forge_tx.from = sender_key.address(); // Forged 'from'

        let engine = SequentialBlockEngine::new(ChainId::MAINNET);
        let err = engine
            .verify_stateless(&forge_tx, Gas(30_000_000), 0)
            .unwrap_err();
        assert_eq!(err, BlockValidationError::InvalidSignature);
    }

    #[test]
    fn test_evm_transaction_fee_distribution_and_zero_base_fee_burn() {
        let sender_key = Secp256k1Keypair::generate();
        let sender_addr = sender_key.address();
        let proposer_addr = Address([0x77; 20]);

        let initial_sender_balance = Amount::from_u128(10_000_000_000_000_000_000); // 10 TOLA
        let mut state = MemoryStateJournal::new();
        state.set_account(
            sender_addr,
            AccountState::new_with_balance(initial_sender_balance),
        );

        // Simple EVM bytecode: PUSH1 0x42 PUSH1 0x00 SSTORE STOP
        let bytecode = vec![0x60, 0x42, 0x60, 0x00, 0x55, 0x00];

        // Create Type-2 EIP-1559 EVM deployment transaction
        let tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce::ZERO,
            from: sender_addr,
            to: None, // Contract creation
            value: Amount::ZERO,
            gas_limit: Gas(100_000),
            max_fee_per_gas: Amount::from_u128(20),
            max_priority_fee_per_gas: Amount::from_u128(5),
            valid_until_block: BlockHeight(100),
            data: bytecode,
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [1u8; 32],
                s: [1u8; 32],
                y_parity: 0,
                chain_id: 9999,
            },
            tx_type: 2,
            access_list: vec![],
        };

        let base_fee = Amount::from_u128(10);
        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(0),
                timestamp: Timestamp(2000),
                proposer: proposer_addr,
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(100_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: base_fee,
                extra_data: vec![],
            },
            transactions: vec![tx],
            last_commit_qc: None,
        };

        let engine = SequentialBlockEngine::new(ChainId::MAINNET);
        let receipts = engine
            .execute_block(&block, &mut state)
            .expect("EVM block execution succeeds");

        assert_eq!(receipts.len(), 1);
        assert!(receipts[0].success);
        let gas_used = receipts[0].gas_used.as_u64();
        assert!(gas_used > 0);

        // Effective price = base_fee (10) + priority_fee (5) = 15
        let effective_price = 15u128;
        let actual_gas_cost = gas_used as u128 * effective_price;
        let expected_val_fee = (actual_gas_cost * 75) / 100;
        let expected_treasury_fee = actual_gas_cost - expected_val_fee;

        let sender_final = state.get_account(&sender_addr).unwrap().unwrap();
        let pool_final = state
            .get_account(&Address::VALIDATOR_REWARD_POOL)
            .unwrap()
            .unwrap();
        let treasury_final = state
            .get_account(&Address::PROTOCOL_TREASURY)
            .unwrap()
            .unwrap();

        // 1. Sender debited exactly actual_gas_cost
        assert_eq!(
            sender_final.balance.as_u128(),
            initial_sender_balance.as_u128() - actual_gas_cost
        );

        // 2. Exact 75/25 split to Validator Pool and Protocol Treasury
        assert_eq!(pool_final.balance.as_u128(), expected_val_fee);
        assert_eq!(treasury_final.balance.as_u128(), expected_treasury_fee);

        // 3. Proposer receives ZERO gas fees directly (no duplicate beneficiary credit)
        assert_eq!(
            state
                .get_account(&proposer_addr)
                .unwrap()
                .unwrap_or(AccountState::EMPTY)
                .balance,
            Amount::ZERO,
            "Proposer must receive zero gas fees directly"
        );

        // 4. Exact supply conservation (no base-fee burned, no tokens minted)
        let total_accounted = sender_final.balance.as_u128()
            + pool_final.balance.as_u128()
            + treasury_final.balance.as_u128();
        assert_eq!(
            total_accounted,
            initial_sender_balance.as_u128(),
            "Total token supply must be strictly conserved across EVM execution!"
        );
    }

    #[test]
    fn test_evm_reverted_transaction_rolls_back_state_and_charges_gas_once() {
        let sender_key = Secp256k1Keypair::generate();
        let sender_addr = sender_key.address();
        let proposer_addr = Address([0x88; 20]);

        let initial_sender_balance = Amount::from_u128(5_000_000_000_000_000_000); // 5 TOLA
        let mut state = MemoryStateJournal::new();
        state.set_account(
            sender_addr,
            AccountState::new_with_balance(initial_sender_balance),
        );

        // Reverting EVM bytecode: PUSH1 0x00 PUSH1 0x00 REVERT
        let bytecode = vec![0x60, 0x00, 0x60, 0x00, 0xFD];

        let tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce::ZERO,
            from: sender_addr,
            to: None,
            value: Amount::ZERO,
            gas_limit: Gas(100_000),
            max_fee_per_gas: Amount::from_u128(20),
            max_priority_fee_per_gas: Amount::from_u128(5),
            valid_until_block: BlockHeight(100),
            data: bytecode,
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [1u8; 32],
                s: [1u8; 32],
                y_parity: 0,
                chain_id: 9999,
            },
            tx_type: 2,
            access_list: vec![],
        };

        let base_fee = Amount::from_u128(10);
        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(0),
                timestamp: Timestamp(3000),
                proposer: proposer_addr,
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(100_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: base_fee,
                extra_data: vec![],
            },
            transactions: vec![tx],
            last_commit_qc: None,
        };

        let engine = SequentialBlockEngine::new(ChainId::MAINNET);
        let receipts = engine
            .execute_block(&block, &mut state)
            .expect("EVM block execution succeeds with revert receipt");

        assert_eq!(receipts.len(), 1);
        println!(
            "Receipt: success={}, return_data={:?}, gas_used={:?}",
            receipts[0].success, receipts[0].return_data, receipts[0].gas_used
        );
        assert!(
            !receipts[0].success,
            "Transaction must fail with revert status"
        );
        let gas_used = receipts[0].gas_used.as_u64();
        assert!(gas_used > 0);

        let effective_price = 15u128;
        let actual_gas_cost = gas_used as u128 * effective_price;
        let expected_val_fee = (actual_gas_cost * 75) / 100;
        let expected_treasury_fee = actual_gas_cost - expected_val_fee;

        let sender_final = state.get_account(&sender_addr).unwrap().unwrap();
        let pool_final = state
            .get_account(&Address::VALIDATOR_REWARD_POOL)
            .unwrap()
            .unwrap();
        let treasury_final = state
            .get_account(&Address::PROTOCOL_TREASURY)
            .unwrap()
            .unwrap();

        // Sender debited gas fee exactly once
        assert_eq!(
            sender_final.balance.as_u128(),
            initial_sender_balance.as_u128() - actual_gas_cost
        );
        assert_eq!(pool_final.balance.as_u128(), expected_val_fee);
        assert_eq!(treasury_final.balance.as_u128(), expected_treasury_fee);
        assert_eq!(
            state
                .get_account(&proposer_addr)
                .unwrap()
                .unwrap_or(AccountState::EMPTY)
                .balance,
            Amount::ZERO
        );

        // Supply strictly conserved even on reverted transactions
        assert_eq!(
            sender_final.balance.as_u128()
                + pool_final.balance.as_u128()
                + treasury_final.balance.as_u128(),
            initial_sender_balance.as_u128()
        );
    }
}
