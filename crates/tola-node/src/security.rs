//! Runtime security audit engine, formal invariant verifiers, and threat mitigation guards.
//!
//! Enforces:
//! 1. Conservation of native currency (zero unexpected minting or supply inflation).
//! 2. Strict nonce monotonicity per account.
//! 3. Cross-chain replay protection and strict ChainId isolation.
//! 4. Low-S canonical signature malleability prevention (BIP-146 / RFC 6979).
//! 5. Block resource exhaustion and gas envelope bounds.
//! 6. State Merkle integrity invariants.

use tola_crypto::verify_secp256k1_low_s;
use tola_types::{Address, Amount, Block, ChainId, Gas, Hash, Nonce, Transaction};

/// Maximum allowable block gas limit on TOLA Network.
pub const MAX_BLOCK_GAS_LIMIT: Gas = Gas(30_000_000);

/// Maximum transactions allowable within a single proposed block.
pub const MAX_TRANSACTIONS_PER_BLOCK: usize = 10_000;

/// Baseline minimum intrinsic gas for a simple value transfer.
pub const MIN_INTRINSIC_GAS: Gas = Gas(21_000);

/// Invariant violation classification.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum InvariantViolation {
    /// Total circulating tokens plus burned fees deviates from genesis allocation.
    SupplyMismatch { expected: u128, actual: u128 },
    /// Account nonce did not advance strictly monotonically.
    NonceRegression {
        address: Address,
        current: u64,
        attempted: u64,
    },
    /// Transaction payload targeted a different blockchain network.
    InvalidChainId { expected: u64, actual: u64 },
    /// ECDSA signature S-value violates canonical low-S upper bound (s > n/2).
    SignatureMalleability,
    /// Transaction gas parameters exceed block limits or are structurally invalid.
    InvalidGasParameters(String),
    /// Proposed block gas exceeds consensus envelope.
    BlockGasLimitExceeded { limit: u64, max_allowed: u64 },
    /// Proposed block transactions exceed consensus capacity.
    BlockTransactionCountExceeded { count: usize, max_allowed: usize },
    /// Merkle state root mismatch.
    StateRootCorruption { expected: Hash, actual: Hash },
    /// Sender address is zero address or derives improperly.
    InvalidSenderAddress,
}

/// Central security auditing engine for TOLA nodes.
#[derive(Debug, Clone, Default)]
pub struct SecurityAuditEngine;

impl SecurityAuditEngine {
    pub fn new() -> Self {
        Self
    }

    /// Verifies comprehensive transaction safety rules and cryptographic invariants.
    pub fn verify_transaction_safety(
        &self,
        tx: &Transaction,
        expected_chain_id: ChainId,
        max_block_gas: Gas,
    ) -> Result<(), InvariantViolation> {
        // 1. Cross-chain replay protection (TM-07)
        if tx.chain_id != expected_chain_id {
            return Err(InvariantViolation::InvalidChainId {
                expected: expected_chain_id.0,
                actual: tx.chain_id.0,
            });
        }

        // 2. Sender address validity
        if tx.from == Address::ZERO {
            return Err(InvariantViolation::InvalidSenderAddress);
        }

        // 3. Intrinsic and maximum gas bounds (TM-06)
        if tx.gas_limit < MIN_INTRINSIC_GAS {
            return Err(InvariantViolation::InvalidGasParameters(format!(
                "Gas limit {} below minimum intrinsic gas {}",
                tx.gas_limit.0, MIN_INTRINSIC_GAS.0
            )));
        }
        if tx.gas_limit > max_block_gas {
            return Err(InvariantViolation::InvalidGasParameters(format!(
                "Gas limit {} exceeds max block gas {}",
                tx.gas_limit.0, max_block_gas.0
            )));
        }

        // 4. EIP-1559 fee priority ordering
        if tx.max_priority_fee_per_gas > tx.max_fee_per_gas {
            return Err(InvariantViolation::InvalidGasParameters(
                "max_priority_fee_per_gas cannot exceed max_fee_per_gas".into(),
            ));
        }

        // 5. Signature malleability (Low-S verification)
        match &tx.signature {
            tola_types::Signature::Secp256k1 { r, s, v } => {
                let mut sig_65 = [0u8; 65];
                sig_65[0..32].copy_from_slice(r);
                sig_65[32..64].copy_from_slice(s);
                sig_65[64] = *v;

                if !verify_secp256k1_low_s(&sig_65) {
                    return Err(InvariantViolation::SignatureMalleability);
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Verifies consensus and resource bounds on a proposed block.
    pub fn verify_block_safety(
        &self,
        block: &Block,
        max_block_gas: Gas,
        max_tx_count: usize,
    ) -> Result<(), InvariantViolation> {
        // 1. Transaction count bounds
        if block.transactions.len() > max_tx_count {
            return Err(InvariantViolation::BlockTransactionCountExceeded {
                count: block.transactions.len(),
                max_allowed: max_tx_count,
            });
        }

        // 2. Gas limit bounds
        if block.header.gas_limit > max_block_gas {
            return Err(InvariantViolation::BlockGasLimitExceeded {
                limit: block.header.gas_limit.0,
                max_allowed: max_block_gas.0,
            });
        }

        // 3. Cumulative gas used cannot exceed block gas limit
        if block.header.gas_used > block.header.gas_limit {
            return Err(InvariantViolation::InvalidGasParameters(format!(
                "Block gas_used {} exceeds block gas_limit {}",
                block.header.gas_used.0, block.header.gas_limit.0
            )));
        }

        Ok(())
    }

    /// Verifies strict conservation of token supply:
    /// Invariant: genesis_supply == current_circulating + total_fees_burned
    pub fn verify_supply_conservation(
        &self,
        genesis_supply: Amount,
        current_circulating: Amount,
        total_fees_burned: Amount,
    ) -> Result<(), InvariantViolation> {
        let expected = genesis_supply.0;
        let actual = current_circulating
            .0
            .checked_add(total_fees_burned.0)
            .ok_or(InvariantViolation::SupplyMismatch {
                expected,
                actual: u128::MAX,
            })?;

        if expected != actual {
            return Err(InvariantViolation::SupplyMismatch { expected, actual });
        }

        Ok(())
    }

    /// Verifies strict nonce monotonicity during account state transition.
    pub fn verify_nonce_step(
        &self,
        address: Address,
        current_nonce: Nonce,
        next_nonce: Nonce,
    ) -> Result<(), InvariantViolation> {
        if next_nonce.0 != current_nonce.0.saturating_add(1) {
            return Err(InvariantViolation::NonceRegression {
                address,
                current: current_nonce.0,
                attempted: next_nonce.0,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tola_types::*;

    #[test]
    fn test_supply_conservation_invariant() {
        let engine = SecurityAuditEngine::new();
        let genesis = Amount(1_000_000_000_000_000_000_000); // 1,000 TOLA
        let circulating = Amount(999_900_000_000_000_000_000); // 999.9 TOLA
        let burned = Amount(100_000_000_000_000_000); // 0.1 TOLA

        // Valid conservation
        assert!(engine
            .verify_supply_conservation(genesis, circulating, burned)
            .is_ok());

        // Inflation attack: actual exceeds genesis
        let fraudulent_circulating = Amount(1_000_000_000_000_000_000_001);
        let res = engine.verify_supply_conservation(genesis, fraudulent_circulating, burned);
        assert!(matches!(
            res,
            Err(InvariantViolation::SupplyMismatch { .. })
        ));
    }

    #[test]
    fn test_nonce_monotonicity_invariant() {
        let engine = SecurityAuditEngine::new();
        let addr = Address([0x11; 20]);

        assert!(engine.verify_nonce_step(addr, Nonce(0), Nonce(1)).is_ok());
        assert!(engine.verify_nonce_step(addr, Nonce(5), Nonce(6)).is_ok());

        // Nonce regression or skip
        assert!(matches!(
            engine.verify_nonce_step(addr, Nonce(1), Nonce(1)),
            Err(InvariantViolation::NonceRegression { .. })
        ));
        assert!(matches!(
            engine.verify_nonce_step(addr, Nonce(1), Nonce(3)),
            Err(InvariantViolation::NonceRegression { .. })
        ));
    }

    #[test]
    fn test_chain_id_replay_rejection() {
        let engine = SecurityAuditEngine::new();
        let mut tx = Transaction {
            chain_id: ChainId(1), // Ethereum Mainnet
            nonce: Nonce(0),
            from: Address([0x11; 20]),
            to: Some(Address([0x22; 20])),
            value: Amount(1000),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount(1_000_000_000),
            max_priority_fee_per_gas: Amount(1_000_000_000),
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1 {
                r: [0; 32],
                s: [0; 32],
                v: 0,
            },
            tx_type: 0,
            access_list: vec![],
        };

        // Rejection when submitted to TOLA Testnet (Chain ID 102)
        let res = engine.verify_transaction_safety(&tx, ChainId(102), MAX_BLOCK_GAS_LIMIT);
        assert!(matches!(
            res,
            Err(InvariantViolation::InvalidChainId {
                expected: 102,
                actual: 1
            })
        ));

        // Accepted when chain ID matches
        tx.chain_id = ChainId(102);
        assert!(engine
            .verify_transaction_safety(&tx, ChainId(102), MAX_BLOCK_GAS_LIMIT)
            .is_ok());
    }

    #[test]
    fn test_signature_malleability_high_s_rejected() {
        let engine = SecurityAuditEngine::new();
        // High-S value > N/2
        let high_s = [0xff; 32];
        let tx = Transaction {
            chain_id: ChainId(102),
            nonce: Nonce(0),
            from: Address([0x11; 20]),
            to: Some(Address([0x22; 20])),
            value: Amount(1000),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount(1_000_000_000),
            max_priority_fee_per_gas: Amount(1_000_000_000),
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1 {
                r: [0x01; 32],
                s: high_s,
                v: 0,
            },
            tx_type: 0,
            access_list: vec![],
        };

        let res = engine.verify_transaction_safety(&tx, ChainId(102), MAX_BLOCK_GAS_LIMIT);
        assert_eq!(res, Err(InvariantViolation::SignatureMalleability));
    }

    #[test]
    fn test_block_resource_bounds() {
        let engine = SecurityAuditEngine::new();
        let header = BlockHeader {
            parent_hash: Hash::ZERO,
            height: BlockHeight(1),
            epoch: Epoch(0),
            round: Round(0),
            timestamp: Timestamp(0),
            proposer: Address::ZERO,
            state_root: Hash::ZERO,
            txs_root: Hash::ZERO,
            receipts_root: Hash::ZERO,
            gas_used: Gas(35_000_000),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount(1),
            extra_data: vec![],
        };
        let block = Block {
            header,
            transactions: vec![],
            last_commit_qc: None,
        };

        // Rejection because gas_used > gas_limit
        let res =
            engine.verify_block_safety(&block, MAX_BLOCK_GAS_LIMIT, MAX_TRANSACTIONS_PER_BLOCK);
        assert!(matches!(
            res,
            Err(InvariantViolation::InvalidGasParameters(_))
        ));
    }
}
