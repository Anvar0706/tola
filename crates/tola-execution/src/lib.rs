//! Block-STM parallel execution scheduler and state transition harness.

use tola_types::*;
use tola_core::{BlockEngine, BlockValidationError, VMError, StateDB};

pub struct SequentialBlockEngine;

impl BlockEngine for SequentialBlockEngine {
    fn validate_block(&self, block: &Block, state: &dyn StateDB) -> Result<(), BlockValidationError> {
        if block.header.height.0 == 0 {
            return Ok(()); // Genesis
        }
        for tx in &block.transactions {
            let acct = state.get_account(&tx.from).map_err(|_| BlockValidationError::InvalidStateRoot)?;
            if let Some(a) = acct {
                if tx.nonce != a.nonce {
                    return Err(BlockValidationError::NonceMismatch);
                }
                if a.balance.0 < tx.value.0 {
                    return Err(BlockValidationError::InsufficientBalance);
                }
            } else {
                return Err(BlockValidationError::InsufficientBalance);
            }
        }
        Ok(())
    }

    fn execute_block(&self, block: &Block, state: &mut dyn StateDB) -> Result<Vec<Receipt>, VMError> {
        let mut receipts = Vec::new();
        for tx in &block.transactions {
            let mut sender = state.get_account(&tx.from).unwrap().unwrap_or_else(|| AccountState {
                nonce: Nonce(0),
                balance: Amount(0),
                code_hash: Hash::ZERO,
                storage_root: Hash::ZERO,
                flags: 0,
            });
            sender.nonce.0 += 1;
            sender.balance.0 -= tx.value.0;
            state.apply_delta(tola_core::StateDelta {
                accounts: vec![(tx.from, Some(sender))],
                storage: vec![],
            }).map_err(|_| VMError::OutOfGas)?;

            if let Some(to_addr) = tx.to {
                let mut recipient = state.get_account(&to_addr).unwrap().unwrap_or_else(|| AccountState {
                    nonce: Nonce(0),
                    balance: Amount(0),
                    code_hash: Hash::ZERO,
                    storage_root: Hash::ZERO,
                    flags: 0,
                });
                recipient.balance.0 += tx.value.0;
                state.apply_delta(tola_core::StateDelta {
                    accounts: vec![(to_addr, Some(recipient))],
                    storage: vec![],
                }).map_err(|_| VMError::OutOfGas)?;
            }

            receipts.push(Receipt {
                tx_id: TransactionId(Hash::ZERO),
                success: true,
                gas_used: Gas(21_000),
                cumulative_gas_used: Gas(21_000),
                logs: vec![],
                return_data: vec![],
            });
        }
        Ok(receipts)
    }
}
