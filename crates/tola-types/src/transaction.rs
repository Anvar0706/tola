//! Canonical transaction layout, fee economics, and validation rules.
//!
//! Complies with ADR-0012, EIP-1559 fee elasticity, and Transaction Specification v0.1.

use crate::address::Address;
use crate::primitives::{Amount, BlockHeight, ChainId, Gas, Hash, Nonce, Signature};
use core::fmt;

pub const MAX_TRANSACTION_PAYLOAD_BYTES: usize = 128 * 1024; // 128 KB
pub const MAX_INITCODE_SIZE: usize = 49152; // 48 KB per EIP-3860
pub const MAX_MEMO_BYTES: usize = 256; // 256 bytes UTF-8 max
pub const MIN_TRANSACTION_GAS_LIMIT: u64 = 21_000;
pub const MAX_ACCESS_LIST_ITEMS: usize = 256;

/// EIP-2930 / EIP-1559 Access List Item specifying an account address and associated storage keys.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct AccessListItem {
    pub address: Address,
    pub storage_keys: Vec<Hash>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TransactionError {
    ChainIdMismatch { expected: ChainId, got: ChainId },
    GasLimitTooLow { minimum: u64, got: u64 },
    GasLimitExceedsBlock { maximum: u64, got: u64 },
    FeeCapLessThanPriorityFee,
    PayloadTooLarge { max_bytes: usize, got_bytes: usize },
    InitcodeTooLarge { max_bytes: usize, got_bytes: usize },
    AccessListTooLarge { max_items: usize, got_items: usize },
    MemoTooLarge { max_bytes: usize, got_bytes: usize },
    ArithmeticOverflow,
    InvalidSignature(String),
}

impl fmt::Display for TransactionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransactionError::ChainIdMismatch { expected, got } => {
                write!(f, "ChainId mismatch: expected {expected:?}, got {got:?}")
            }
            TransactionError::GasLimitTooLow { minimum, got } => {
                write!(f, "Gas limit too low: minimum {minimum}, got {got}")
            }
            TransactionError::GasLimitExceedsBlock { maximum, got } => {
                write!(f, "Gas limit exceeds block max: limit {maximum}, got {got}")
            }
            TransactionError::FeeCapLessThanPriorityFee => {
                write!(
                    f,
                    "max_fee_per_gas cannot be less than max_priority_fee_per_gas"
                )
            }
            TransactionError::PayloadTooLarge {
                max_bytes,
                got_bytes,
            } => {
                write!(
                    f,
                    "Transaction payload exceeds bound: max {max_bytes}B, got {got_bytes}B"
                )
            }
            TransactionError::InitcodeTooLarge {
                max_bytes,
                got_bytes,
            } => {
                write!(
                    f,
                    "Initcode exceeds EIP-3860 limit: max {max_bytes}B, got {got_bytes}B"
                )
            }
            TransactionError::AccessListTooLarge {
                max_items,
                got_items,
            } => {
                write!(
                    f,
                    "Access list exceeds bound: max {max_items} items, got {got_items} items"
                )
            }
            TransactionError::MemoTooLarge {
                max_bytes,
                got_bytes,
            } => {
                write!(
                    f,
                    "Transaction memo exceeds bound: max {max_bytes}B, got {got_bytes}B"
                )
            }
            TransactionError::ArithmeticOverflow => {
                write!(f, "Transaction fee arithmetic overflowed")
            }
            TransactionError::InvalidSignature(msg) => {
                write!(f, "Signature validation error: {msg}")
            }
        }
    }
}

impl std::error::Error for TransactionError {}

/// Unique transaction identifier (BLAKE3 hash of canonical wire serialization).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct TransactionId(pub Hash);

impl TransactionId {
    pub const ZERO: Self = Self(Hash::ZERO);

    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }
}

/// Canonical transaction envelope circulating across P2P and consensus.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Transaction {
    pub chain_id: ChainId,
    pub nonce: Nonce,
    pub from: Address,
    pub to: Option<Address>,
    pub value: Amount,
    pub gas_limit: Gas,
    pub max_fee_per_gas: Amount,
    pub max_priority_fee_per_gas: Amount,
    pub valid_until_block: BlockHeight,
    pub data: Vec<u8>,
    pub memo: Option<String>,
    pub signature: Signature,
    pub tx_type: u8,
    pub access_list: Vec<AccessListItem>,
}

impl Transaction {
    pub fn is_contract_creation(&self) -> bool {
        self.to.is_none()
    }

    pub fn is_evm(&self) -> bool {
        matches!(self.signature, Signature::Secp256k1Eth { .. })
    }

    /// Calculate required intrinsic gas for this transaction based on creation status,
    /// calldata length and byte values, EIP-3860 initcode metering, and EIP-2930 access list entries.
    pub fn intrinsic_gas(&self) -> Gas {
        let mut gas: u64 = if self.is_contract_creation() {
            53_000 // 21,000 base + 32,000 create (EIP-2)
        } else {
            21_000
        };

        for &b in &self.data {
            if b == 0 {
                gas = gas.saturating_add(4);
            } else {
                gas = gas.saturating_add(16);
            }
        }

        if self.is_contract_creation() && !self.data.is_empty() {
            let words = (self.data.len() as u64).div_ceil(32);
            gas = gas.saturating_add(words.saturating_mul(2));
        }

        for item in &self.access_list {
            gas = gas.saturating_add(2_400);
            gas = gas.saturating_add((item.storage_keys.len() as u64).saturating_mul(1_900));
        }

        Gas(gas)
    }

    /// Calculate maximum possible upfront gas expense: gas_limit * max_fee_per_gas.
    pub fn max_gas_cost(&self) -> Result<Amount, TransactionError> {
        self.max_fee_per_gas
            .checked_mul(self.gas_limit.as_u64())
            .ok_or(TransactionError::ArithmeticOverflow)
    }

    /// Total upfront balance requirement: (gas_limit * max_fee_per_gas) + value.
    pub fn upfront_cost(&self) -> Result<Amount, TransactionError> {
        let max_gas = self.max_gas_cost()?;
        max_gas
            .checked_add(self.value)
            .ok_or(TransactionError::ArithmeticOverflow)
    }

    /// Compute dynamic EIP-1559 effective gas price based on block base fee:
    /// effective_price = base_fee + min(max_priority_fee_per_gas, max_fee_per_gas - base_fee)
    pub fn effective_gas_price(&self, base_fee: Amount) -> Amount {
        if self.max_fee_per_gas <= base_fee {
            return self.max_fee_per_gas;
        }
        let max_priority_allowed = self.max_fee_per_gas.saturating_sub(base_fee);
        let actual_priority = self.max_priority_fee_per_gas.min(max_priority_allowed);
        base_fee.saturating_add(actual_priority)
    }

    /// Stateless validation checks prior to mempool admission or execution.
    pub fn stateless_validate(
        &self,
        current_chain_id: ChainId,
        max_block_gas: Gas,
    ) -> Result<(), TransactionError> {
        if self.chain_id != current_chain_id {
            return Err(TransactionError::ChainIdMismatch {
                expected: current_chain_id,
                got: self.chain_id,
            });
        }

        if self.access_list.len() > MAX_ACCESS_LIST_ITEMS {
            return Err(TransactionError::AccessListTooLarge {
                max_items: MAX_ACCESS_LIST_ITEMS,
                got_items: self.access_list.len(),
            });
        }

        let min_gas = if self.is_evm() {
            self.intrinsic_gas().as_u64()
        } else {
            MIN_TRANSACTION_GAS_LIMIT
        };

        if self.gas_limit.as_u64() < min_gas {
            return Err(TransactionError::GasLimitTooLow {
                minimum: min_gas,
                got: self.gas_limit.as_u64(),
            });
        }

        if self.gas_limit.as_u64() > max_block_gas.as_u64() {
            return Err(TransactionError::GasLimitExceedsBlock {
                maximum: max_block_gas.as_u64(),
                got: self.gas_limit.as_u64(),
            });
        }

        if self.max_fee_per_gas < self.max_priority_fee_per_gas {
            return Err(TransactionError::FeeCapLessThanPriorityFee);
        }

        if self.is_contract_creation() && self.data.len() > MAX_INITCODE_SIZE {
            return Err(TransactionError::InitcodeTooLarge {
                max_bytes: MAX_INITCODE_SIZE,
                got_bytes: self.data.len(),
            });
        }

        if self.data.len() > MAX_TRANSACTION_PAYLOAD_BYTES {
            return Err(TransactionError::PayloadTooLarge {
                max_bytes: MAX_TRANSACTION_PAYLOAD_BYTES,
                got_bytes: self.data.len(),
            });
        }

        if let Some(ref memo) = self.memo {
            if memo.len() > MAX_MEMO_BYTES {
                return Err(TransactionError::MemoTooLarge {
                    max_bytes: MAX_MEMO_BYTES,
                    got_bytes: memo.len(),
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effective_gas_price_elasticity() {
        let tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce::ZERO,
            from: Address::ZERO,
            to: Some(Address::ZERO),
            value: Amount::ZERO,
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(100),
            max_priority_fee_per_gas: Amount::from_u128(10),
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Ed25519([0u8; 64]),
            tx_type: 0,
            access_list: vec![],
        };

        // Case 1: Base fee is 50. Total effective price = 50 + 10 = 60.
        let price1 = tx.effective_gas_price(Amount::from_u128(50));
        assert_eq!(price1, Amount::from_u128(60));

        // Case 2: Base fee spikes to 95. Headroom is 5 (< 10). Effective price = 95 + 5 = 100.
        let price2 = tx.effective_gas_price(Amount::from_u128(95));
        assert_eq!(price2, Amount::from_u128(100));

        // Case 3: Base fee exceeds max fee (e.g. 110). Capped at max_fee = 100.
        let price3 = tx.effective_gas_price(Amount::from_u128(110));
        assert_eq!(price3, Amount::from_u128(100));
    }

    #[test]
    fn test_stateless_validation() {
        let valid_tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce::ZERO,
            from: Address::ZERO,
            to: Some(Address::ZERO),
            value: Amount::ZERO,
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(10),
            max_priority_fee_per_gas: Amount::from_u128(2),
            valid_until_block: BlockHeight(100),
            data: vec![0u8; 100],
            memo: None,
            signature: Signature::Ed25519([0u8; 64]),
            tx_type: 0,
            access_list: vec![],
        };

        assert!(valid_tx
            .stateless_validate(ChainId::MAINNET, Gas(30_000_000))
            .is_ok());

        // Testnet chain ID mismatch
        assert!(valid_tx
            .stateless_validate(ChainId::TESTNET, Gas(30_000_000))
            .is_err());

        // Insufficient gas limit (< 21,000)
        let mut low_gas = valid_tx.clone();
        low_gas.gas_limit = Gas(20_999);
        assert!(low_gas
            .stateless_validate(ChainId::MAINNET, Gas(30_000_000))
            .is_err());

        // Fee cap lower than tip
        let mut bad_fee = valid_tx.clone();
        bad_fee.max_fee_per_gas = Amount::from_u128(1);
        bad_fee.max_priority_fee_per_gas = Amount::from_u128(2);
        assert!(bad_fee
            .stateless_validate(ChainId::MAINNET, Gas(30_000_000))
            .is_err());

        // Valid memo: English, Uzbek, emoji
        let mut tx_memo = valid_tx.clone();
        tx_memo.memo = Some("Invoice #1042".to_string());
        assert!(tx_memo
            .stateless_validate(ChainId::MAINNET, Gas(30_000_000))
            .is_ok());

        tx_memo.memo = Some("To‘lov".to_string());
        assert!(tx_memo
            .stateless_validate(ChainId::MAINNET, Gas(30_000_000))
            .is_ok());

        tx_memo.memo = Some("💰".to_string());
        assert!(tx_memo
            .stateless_validate(ChainId::MAINNET, Gas(30_000_000))
            .is_ok());

        // 256 bytes memo: PASS
        tx_memo.memo = Some("a".repeat(256));
        assert!(tx_memo
            .stateless_validate(ChainId::MAINNET, Gas(30_000_000))
            .is_ok());

        // 257 bytes memo: REJECT
        tx_memo.memo = Some("a".repeat(257));
        assert_eq!(
            tx_memo.stateless_validate(ChainId::MAINNET, Gas(30_000_000)),
            Err(TransactionError::MemoTooLarge {
                max_bytes: 256,
                got_bytes: 257
            })
        );
    }

    #[test]
    fn test_access_list_validation_and_intrinsic_gas() {
        let mut evm_tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce::ZERO,
            from: Address::ZERO,
            to: Some(Address::ZERO),
            value: Amount::ZERO,
            gas_limit: Gas(100_000),
            max_fee_per_gas: Amount::from_u128(10),
            max_priority_fee_per_gas: Amount::from_u128(2),
            valid_until_block: BlockHeight(100),
            data: vec![0, 1, 2, 3], // 1 zero (4), 3 nonzero (48) -> 52 gas
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [1; 32],
                s: [1; 32],
                y_parity: 0,
                chain_id: ChainId::MAINNET.0,
            },
            tx_type: 2,
            access_list: vec![AccessListItem {
                address: Address([0xaa; 20]),
                storage_keys: vec![Hash([0x01; 32]), Hash([0x02; 32])],
            }],
        };

        // Intrinsic gas = 21,000 + 52 + 2,400 + 2 * 1,900 = 27,252
        assert_eq!(evm_tx.intrinsic_gas(), Gas(27_252));

        evm_tx.gas_limit = Gas(27_251);
        assert_eq!(
            evm_tx.stateless_validate(ChainId::MAINNET, Gas(30_000_000)),
            Err(TransactionError::GasLimitTooLow {
                minimum: 27_252,
                got: 27_251,
            })
        );

        evm_tx.gas_limit = Gas(27_252);
        assert!(evm_tx
            .stateless_validate(ChainId::MAINNET, Gas(30_000_000))
            .is_ok());

        // Oversized access list > 256 items
        evm_tx.access_list = (0..257)
            .map(|_| AccessListItem {
                address: Address::ZERO,
                storage_keys: vec![],
            })
            .collect();
        assert_eq!(
            evm_tx.stateless_validate(ChainId::MAINNET, Gas(30_000_000)),
            Err(TransactionError::AccessListTooLarge {
                max_items: 256,
                got_items: 257,
            })
        );
    }
}
