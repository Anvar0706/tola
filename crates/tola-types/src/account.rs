//! Account state representation and protocol invariants.
//!
//! Complies with ADR-0010 and State Transition Invariants v0.1:
//! - Balance Non-Negativity Invariant
//! - Nonce Monotonicity Invariant
//! - Zero-allocation state leaf layout

use crate::primitives::{Amount, Gas, Hash, Nonce};
use core::fmt;

pub const FLAG_NONE: u32 = 0;
pub const FLAG_CONTRACT: u32 = 1 << 0;
pub const FLAG_FROZEN: u32 = 1 << 1;
pub const FLAG_VALIDATOR: u32 = 1 << 2;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum AccountError {
    InsufficientBalance {
        requested: Amount,
        available: Amount,
    },
    BalanceOverflow,
    NonMonotonicNonce {
        current: Nonce,
        provided: Nonce,
    },
    AccountFrozen,
}

impl fmt::Display for AccountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AccountError::InsufficientBalance {
                requested,
                available,
            } => {
                write!(
                    f,
                    "Insufficient balance: requested {requested}, available {available}"
                )
            }
            AccountError::BalanceOverflow => write!(f, "Balance arithmetic overflowed"),
            AccountError::NonMonotonicNonce { current, provided } => {
                write!(f, "Nonce violation: current {current}, provided {provided}")
            }
            AccountError::AccountFrozen => write!(f, "Account is frozen by governance"),
        }
    }
}

impl std::error::Error for AccountError {}

/// Canonical on-chain state for an account or smart contract.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AccountState {
    pub nonce: Nonce,
    pub balance: Amount,
    pub code_hash: Hash,
    pub storage_root: Hash,
    pub flags: u32,
}

impl AccountState {
    pub const EMPTY: Self = Self {
        nonce: Nonce::ZERO,
        balance: Amount::ZERO,
        code_hash: Hash::ZERO,
        storage_root: Hash::ZERO,
        flags: FLAG_NONE,
    };

    pub fn new_empty() -> Self {
        Self::EMPTY
    }

    pub fn new_with_balance(balance: Amount) -> Self {
        Self {
            nonce: Nonce::ZERO,
            balance,
            code_hash: Hash::ZERO,
            storage_root: Hash::ZERO,
            flags: FLAG_NONE,
        }
    }

    pub fn new_contract(balance: Amount, code_hash: Hash, storage_root: Hash) -> Self {
        Self {
            nonce: Nonce::ZERO,
            balance,
            code_hash,
            storage_root,
            flags: FLAG_CONTRACT,
        }
    }

    /// Check if account can afford upfront transaction gas fee and transfer value.
    pub fn can_afford(&self, gas_limit: Gas, max_fee_per_gas: Amount, value: Amount) -> bool {
        if self.is_frozen() {
            return false;
        }
        let total_gas_cost = match max_fee_per_gas.checked_mul(gas_limit.as_u64()) {
            Some(c) => c,
            None => return false,
        };
        let total_required = match total_gas_cost.checked_add(value) {
            Some(t) => t,
            None => return false,
        };
        self.balance >= total_required
    }

    /// Deduct an amount from the balance, enforcing the Non-Negativity Invariant.
    pub fn debit(&mut self, amount: Amount) -> Result<(), AccountError> {
        if self.is_frozen() {
            return Err(AccountError::AccountFrozen);
        }
        match self.balance.checked_sub(amount) {
            Some(new_bal) => {
                self.balance = new_bal;
                Ok(())
            }
            None => Err(AccountError::InsufficientBalance {
                requested: amount,
                available: self.balance,
            }),
        }
    }

    /// Credit an amount to the balance, enforcing overflow prevention.
    pub fn credit(&mut self, amount: Amount) -> Result<(), AccountError> {
        if self.is_frozen() {
            return Err(AccountError::AccountFrozen);
        }
        match self.balance.checked_add(amount) {
            Some(new_bal) => {
                self.balance = new_bal;
                Ok(())
            }
            None => Err(AccountError::BalanceOverflow),
        }
    }

    /// Monotonically increment nonce by 1.
    pub fn increment_nonce(&mut self) -> Result<Nonce, AccountError> {
        let next = self.nonce.next();
        self.nonce = next;
        Ok(next)
    }

    /// Strictly advance nonce, rejecting non-monotonic updates.
    pub fn advance_nonce_to(&mut self, target: Nonce) -> Result<(), AccountError> {
        if target.as_u64() <= self.nonce.as_u64() {
            return Err(AccountError::NonMonotonicNonce {
                current: self.nonce,
                provided: target,
            });
        }
        self.nonce = target;
        Ok(())
    }

    pub fn is_contract(&self) -> bool {
        (self.flags & FLAG_CONTRACT) != 0
    }

    pub fn is_frozen(&self) -> bool {
        (self.flags & FLAG_FROZEN) != 0
    }

    pub fn is_validator(&self) -> bool {
        (self.flags & FLAG_VALIDATOR) != 0
    }

    pub fn set_flag(&mut self, flag: u32) {
        self.flags |= flag;
    }

    pub fn clear_flag(&mut self, flag: u32) {
        self.flags &= !flag;
    }
}

impl Default for AccountState {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balance_debit_underflow_prevention() {
        let mut account = AccountState::new_with_balance(Amount::from_u128(100));
        assert!(account.debit(Amount::from_u128(50)).is_ok());
        assert_eq!(account.balance, Amount::from_u128(50));

        let err = account.debit(Amount::from_u128(51)).unwrap_err();
        match err {
            AccountError::InsufficientBalance {
                requested,
                available,
            } => {
                assert_eq!(requested, Amount::from_u128(51));
                assert_eq!(available, Amount::from_u128(50));
            }
            _ => panic!("Expected InsufficientBalance error"),
        }
        assert_eq!(account.balance, Amount::from_u128(50)); // State preserved
    }

    #[test]
    fn test_nonce_monotonicity() {
        let mut account = AccountState::new_empty();
        assert_eq!(account.nonce, Nonce::ZERO);

        let n1 = account.increment_nonce().unwrap();
        assert_eq!(n1, Nonce(1));
        assert_eq!(account.nonce, Nonce(1));

        assert!(account.advance_nonce_to(Nonce(5)).is_ok());
        assert_eq!(account.nonce, Nonce(5));

        // Attempt non-monotonic regress or repeat
        let err = account.advance_nonce_to(Nonce(5)).unwrap_err();
        assert!(matches!(err, AccountError::NonMonotonicNonce { .. }));

        let err2 = account.advance_nonce_to(Nonce(4)).unwrap_err();
        assert!(matches!(err2, AccountError::NonMonotonicNonce { .. }));
    }

    #[test]
    fn test_account_flags() {
        let mut account = AccountState::new_empty();
        assert!(!account.is_contract());
        assert!(!account.is_frozen());

        account.set_flag(FLAG_CONTRACT);
        assert!(account.is_contract());

        account.set_flag(FLAG_FROZEN);
        assert!(account.is_frozen());
        assert!(account.debit(Amount(10)).is_err()); // Frozen account blocked

        account.clear_flag(FLAG_FROZEN);
        assert!(!account.is_frozen());
    }

    #[test]
    fn test_can_afford_gas_and_value() {
        let account = AccountState::new_with_balance(Amount::from_u128(100_000));
        let gas_limit = Gas(21_000);
        let max_fee = Amount::from_u128(2); // gas cost = 42,000
        let value = Amount::from_u128(50_000); // total = 92,000

        assert!(account.can_afford(gas_limit, max_fee, value));

        let excessive_value = Amount::from_u128(60_000); // total = 102,000
        assert!(!account.can_afford(gas_limit, max_fee, excessive_value));
    }
}
