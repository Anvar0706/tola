//! Block Assembler and Block-STM Dependency Spreading Engine.
//!
//! Complies with ADR-0012:
//! - Pulls high-fee ready transactions from Mempool
//! - Mitigates Block-STM abort cascades by round-robin interleaving target contract destinations
//! - Enforces strict block gas limits and transaction count bounds

use crate::pool::Mempool;
use std::collections::{HashMap, VecDeque};
use tola_types::*;

pub struct BlockAssembler {
    pub chain_id: ChainId,
}

impl BlockAssembler {
    pub fn new(chain_id: ChainId) -> Self {
        Self { chain_id }
    }

    /// Assemble a proposed block candidate from the Mempool applying dependency spreading.
    pub fn assemble_block(
        &self,
        mempool: &Mempool,
        max_count: usize,
        max_gas: Gas,
        base_fee_per_gas: Amount,
        proposer: Address,
        parent_hash: Hash,
        height: BlockHeight,
        timestamp: Timestamp,
    ) -> Block {
        // 1. Pull pool candidates (fetching up to 2x requested limit to allow interleaving headroom)
        let candidates = mempool.get_ready_transactions(max_count * 2, max_gas);

        // 2. Apply Block-STM dependency spreading
        let selected_txs = self.spread_dependencies(candidates, max_count, max_gas);

        let total_gas_used = selected_txs.iter().fold(Gas::ZERO, |acc, tx| {
            acc.checked_add(tx.gas_limit).unwrap_or(acc)
        });

        let header = BlockHeader {
            parent_hash,
            height,
            epoch: Epoch(height.as_u64() / 1000),
            round: Round(0),
            timestamp,
            proposer,
            state_root: Hash::ZERO,
            txs_root: Hash::ZERO,
            receipts_root: Hash::ZERO,
            gas_used: total_gas_used,
            gas_limit: max_gas,
            base_fee_per_gas,
            extra_data: vec![],
        };

        Block {
            header,
            transactions: selected_txs,
            last_commit_qc: None,
        }
    }

    /// Interleave transactions by recipient address to space out hot contract accesses
    /// and maximize parallel execution concurrency in Block-STM.
    pub fn spread_dependencies(
        &self,
        txs: Vec<Transaction>,
        max_count: usize,
        max_gas: Gas,
    ) -> Vec<Transaction> {
        if txs.is_empty() {
            return Vec::new();
        }

        // Group by recipient address (None is grouped under contract deployment bucket)
        let mut groups: HashMap<Option<Address>, VecDeque<Transaction>> = HashMap::new();
        for tx in txs {
            groups.entry(tx.to).or_default().push_back(tx);
        }

        let mut out = Vec::with_capacity(max_count);
        let mut cumulative_gas = Gas::ZERO;
        let mut sender_next_nonce: HashMap<Address, u64> = HashMap::new();

        // Round-robin pull from distinct destination buckets while respecting sender nonce order
        let mut progressed = true;
        while out.len() < max_count && !groups.is_empty() && progressed {
            progressed = false;
            let mut keys: Vec<Option<Address>> = groups.keys().cloned().collect();
            keys.sort();

            for key in keys {
                if out.len() >= max_count {
                    break;
                }
                if let Some(queue) = groups.get_mut(&key) {
                    let can_emit = if let Some(front) = queue.front() {
                        match sender_next_nonce.get(&front.from) {
                            None => true,
                            Some(&exp) => front.nonce.0 == exp,
                        }
                    } else {
                        false
                    };

                    if can_emit {
                        if let Some(tx) = queue.pop_front() {
                            if let Some(next_gas) = cumulative_gas.checked_add(tx.gas_limit) {
                                if next_gas <= max_gas {
                                    cumulative_gas = next_gas;
                                    sender_next_nonce.insert(tx.from, tx.nonce.0 + 1);
                                    out.push(tx);
                                    progressed = true;
                                }
                            }
                        }
                    }
                    if queue.is_empty() {
                        groups.remove(&key);
                    }
                }
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MempoolConfig;
    use tola_crypto::Secp256k1Keypair;
    use tola_state::MemoryStateJournal;

    fn create_tx_to(sender: &Secp256k1Keypair, to: Address, nonce: u64, fee: u128) -> Transaction {
        let from = sender.address();
        let mut unsigned_tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(nonce),
            from,
            to: Some(to),
            value: Amount::from_u128(100),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(fee),
            max_priority_fee_per_gas: Amount::from_u128(fee),
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

        let digest = crate::pool::compute_tx_digest(&unsigned_tx);
        let sig_bytes = sender.sign_recoverable(digest.as_bytes()).unwrap();

        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        r.copy_from_slice(&sig_bytes[0..32]);
        s.copy_from_slice(&sig_bytes[32..64]);
        let v = sig_bytes[64];

        unsigned_tx.signature = Signature::Secp256k1 { r, s, v };
        unsigned_tx
    }

    #[test]
    fn test_dependency_spreading_interleaves_destinations() {
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

        let hot_amm_pool = Address([0xAA; 20]);
        let cold_token_contract = Address([0xBB; 20]);

        // Submit 2 transactions to Hot AMM Pool from different users
        let tx1 = create_tx_to(&user1, hot_amm_pool, 0, 50);
        let tx2 = create_tx_to(&user2, hot_amm_pool, 0, 45);

        // Submit 1 transaction to Cold Token Contract
        let tx3 = create_tx_to(&user3, cold_token_contract, 0, 40);

        pool.insert(tx1, &state, BlockHeight(1)).unwrap();
        pool.insert(tx2, &state, BlockHeight(1)).unwrap();
        pool.insert(tx3, &state, BlockHeight(1)).unwrap();

        let assembler = BlockAssembler::new(chain_id);
        let block = assembler.assemble_block(
            &pool,
            10,
            Gas(30_000_000),
            Amount::from_u128(5),
            Address([0x99; 20]),
            Hash::ZERO,
            BlockHeight(1),
            Timestamp(1000),
        );

        assert_eq!(block.transactions.len(), 3);

        // Interleaving guarantee: consecutive transactions do not target the same destination!
        // tx at index 0 and index 1 must target different addresses
        let dest0 = block.transactions[0].to;
        let dest1 = block.transactions[1].to;
        assert_ne!(
            dest0, dest1,
            "Dependency spreading must interleave destinations!"
        );
    }
}
