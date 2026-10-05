//! Node daemon coordination, configuration, and integration tests.

pub struct NodeConfig {
    pub chain_id: u64,
    pub data_dir: String,
}

#[cfg(test)]
mod tests {
    use tola_types::*;
    use tola_state::MemoryStateJournal;
    use tola_execution::SequentialBlockEngine;
    use tola_core::{BlockEngine, StateDB};

    #[test]
    fn test_valid_transaction_state_transition() {
        let mut state = MemoryStateJournal::new();
        let alice = Address([1u8; 20]);
        let bob = Address([2u8; 20]);

        state.set_account(alice, AccountState {
            nonce: Nonce(0),
            balance: Amount(100_000_000),
            code_hash: Hash::ZERO,
            storage_root: Hash::ZERO,
            flags: 0,
        });

        let tx = Transaction {
            chain_id: ChainId(9999),
            nonce: Nonce(0),
            from: alice,
            to: Some(bob),
            value: Amount(40_000_000),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount(1),
            max_priority_fee_per_gas: Amount(1),
            valid_until_block: BlockHeight(100),
            data: vec![],
            signature: Signature::Ed25519([0u8; 64]),
        };

        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(0),
                timestamp: Timestamp(1000),
                proposer: alice,
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(21_000),
                gas_limit: Gas(30_000_000),
                extra_data: vec![],
            },
            transactions: vec![tx],
            last_commit_qc: None,
        };

        let engine = SequentialBlockEngine;
        assert!(engine.validate_block(&block, &state).is_ok());
        let receipts = engine.execute_block(&block, &mut state).unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(state.get_account(&alice).unwrap().unwrap().balance.0, 60_000_000);
        assert_eq!(state.get_account(&bob).unwrap().unwrap().balance.0, 40_000_000);
    }

    #[test]
    fn test_invalid_nonce_rejected() {
        let mut state = MemoryStateJournal::new();
        let alice = Address([1u8; 20]);
        state.set_account(alice, AccountState {
            nonce: Nonce(5),
            balance: Amount(100_000),
            code_hash: Hash::ZERO,
            storage_root: Hash::ZERO,
            flags: 0,
        });

        let tx = Transaction {
            chain_id: ChainId(9999),
            nonce: Nonce(0), // Out of order!
            from: alice,
            to: None,
            value: Amount(10),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount(1),
            max_priority_fee_per_gas: Amount(1),
            valid_until_block: BlockHeight(100),
            data: vec![],
            signature: Signature::Ed25519([0u8; 64]),
        };

        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(0),
                timestamp: Timestamp(1000),
                proposer: alice,
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(21_000),
                gas_limit: Gas(30_000_000),
                extra_data: vec![],
            },
            transactions: vec![tx],
            last_commit_qc: None,
        };

        let engine = SequentialBlockEngine;
        assert!(engine.validate_block(&block, &state).is_err());
    }

    #[test]
    fn test_insufficient_balance_rejected() {
        let mut state = MemoryStateJournal::new();
        let alice = Address([1u8; 20]);
        state.set_account(alice, AccountState {
            nonce: Nonce(0),
            balance: Amount(500),
            code_hash: Hash::ZERO,
            storage_root: Hash::ZERO,
            flags: 0,
        });

        let tx = Transaction {
            chain_id: ChainId(9999),
            nonce: Nonce(0),
            from: alice,
            to: None,
            value: Amount(10_000), // Exceeds balance!
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount(1),
            max_priority_fee_per_gas: Amount(1),
            valid_until_block: BlockHeight(100),
            data: vec![],
            signature: Signature::Ed25519([0u8; 64]),
        };

        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(0),
                timestamp: Timestamp(1000),
                proposer: alice,
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(21_000),
                gas_limit: Gas(30_000_000),
                extra_data: vec![],
            },
            transactions: vec![tx],
            last_commit_qc: None,
        };

        let engine = SequentialBlockEngine;
        assert!(engine.validate_block(&block, &state).is_err());
    }
}
