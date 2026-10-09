use core::convert::Infallible;
use revm::{
    bytecode::Bytecode,
    inspector::Inspector,
    interpreter::{interpreter_types::Jumps, InstructionResult, Interpreter},
    primitives::{
        Address as RevmAddress, AddressMap, Bytes as RevmBytes, B256, KECCAK_EMPTY, U256,
    },
    state::{Account, AccountInfo},
    Database, DatabaseCommit,
};
use tola_core::{StateDB, StateDelta};
use tola_crypto::keccak256_hash;
use tola_types::{AccountState, Address, Amount, Hash, Nonce, FLAG_CONTRACT, FLAG_NONE};

pub const RESERVED_TLS721_ADDRESS: RevmAddress = RevmAddress::new([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x77, 0x21,
]);

#[derive(Default, Debug, Clone)]
pub struct TolaEvmInspector;

impl<CTX> Inspector<CTX> for TolaEvmInspector {
    fn step(&mut self, interp: &mut Interpreter, _context: &mut CTX) {
        // Opcode 0xFF is SELFDESTRUCT
        if interp.bytecode.opcode() == 0xFF {
            if let Ok(beneficiary_val) = interp.stack.peek(0) {
                // Convert U256 to Address (take lower 20 bytes)
                let beneficiary_bytes: [u8; 32] = beneficiary_val.to_be_bytes();
                let mut addr_bytes = [0u8; 20];
                addr_bytes.copy_from_slice(&beneficiary_bytes[12..32]);
                let beneficiary = RevmAddress::from(addr_bytes);

                if beneficiary == RESERVED_TLS721_ADDRESS {
                    interp.halt(InstructionResult::Revert);
                }
            }
        }
    }
}

pub struct RevmStateAdapter<'a> {
    pub state: &'a mut dyn StateDB,
    pub current_height: u64,
}

impl<'a> RevmStateAdapter<'a> {
    pub fn new(state: &'a mut dyn StateDB, current_height: u64) -> Self {
        Self {
            state,
            current_height,
        }
    }
}

impl<'a> Database for RevmStateAdapter<'a> {
    type Error = Infallible;

    fn basic(&mut self, address: RevmAddress) -> Result<Option<AccountInfo>, Self::Error> {
        let tola_addr = Address(*address.0);
        let acct_opt = self.state.get_account(&tola_addr).unwrap_or(None);

        let Some(acct) = acct_opt else {
            return Ok(None);
        };

        let balance = U256::from(acct.balance.as_u128());
        let nonce = acct.nonce.0;

        let (code_hash, code) =
            if acct.code_hash != Hash::ZERO && acct.code_hash.0 != KECCAK_EMPTY.0 {
                let code_bytes = self.state.get_code(&acct.code_hash).unwrap_or(None);
                let b = code_bytes.map(|c| Bytecode::new_raw(RevmBytes::from(c)));
                (B256::from(acct.code_hash.0), b)
            } else {
                (KECCAK_EMPTY, None)
            };

        Ok(Some(AccountInfo {
            balance,
            nonce,
            code_hash,
            code,
            ..Default::default()
        }))
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        let hash = Hash(code_hash.0);
        let code_bytes = self
            .state
            .get_code(&hash)
            .unwrap_or(None)
            .unwrap_or_default();
        Ok(Bytecode::new_raw(RevmBytes::from(code_bytes)))
    }

    fn storage(&mut self, address: RevmAddress, index: U256) -> Result<U256, Self::Error> {
        let tola_addr = Address(*address.0);
        let slot_key = Hash(index.to_be_bytes());
        let val_hash = self
            .state
            .get_storage(&tola_addr, &slot_key)
            .unwrap_or(Hash::ZERO);
        Ok(U256::from_be_bytes(val_hash.0))
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        if number >= self.current_height {
            return Ok(B256::ZERO);
        }
        if self.current_height > 256 && number < self.current_height - 256 {
            return Ok(B256::ZERO);
        }
        if let Ok(Some(hash)) = self
            .state
            .get_block_hash_by_height(tola_types::BlockHeight(number))
        {
            return Ok(B256::from(hash.0));
        }
        Ok(B256::ZERO)
    }
}

impl<'a> DatabaseCommit for RevmStateAdapter<'a> {
    fn commit(&mut self, changes: AddressMap<Account>) {
        let mut account_deltas = Vec::new();
        let mut storage_deltas = Vec::new();
        let mut code_deltas = Vec::new();

        for (revm_addr, account) in changes {
            // Do not commit dummy beneficiary address into persistent state
            if revm_addr == RevmAddress::ZERO {
                continue;
            }

            let tola_addr = Address(*revm_addr.0);

            if account.is_selfdestructed() {
                account_deltas.push((tola_addr, None));
                continue;
            }

            if account.is_touched() {
                // Bounds check EVM U256 balance to TOLA u128 balance
                let balance_u128 = u128::try_from(account.info.balance).unwrap_or(u128::MAX);
                let balance = Amount::from_u128(balance_u128);
                let nonce = Nonce(account.info.nonce);

                let mut code_hash = Hash(account.info.code_hash.0);
                let mut flags = FLAG_NONE;

                if let Some(code) = &account.info.code {
                    let code_bytes = code.original_bytes();
                    if !code_bytes.is_empty() {
                        let hash = keccak256_hash(&code_bytes);
                        code_hash = Hash(hash.0);
                        code_deltas.push((code_hash, code_bytes.to_vec()));
                        flags |= FLAG_CONTRACT;
                    }
                } else if code_hash != Hash::ZERO && code_hash.0 != KECCAK_EMPTY.0 {
                    flags |= FLAG_CONTRACT;
                }

                // Collect existing account state to preserve storage_root
                let existing = self
                    .state
                    .get_account(&tola_addr)
                    .unwrap_or(None)
                    .unwrap_or(AccountState::EMPTY);

                let new_state = AccountState {
                    nonce,
                    balance,
                    code_hash,
                    storage_root: existing.storage_root,
                    flags,
                };
                account_deltas.push((tola_addr, Some(new_state)));

                // Collect storage changes
                for (slot_key, slot_val) in account.storage {
                    if slot_val.is_changed() {
                        let key = Hash(slot_key.to_be_bytes());
                        let val = Hash(slot_val.present_value.to_be_bytes());
                        storage_deltas.push(((tola_addr, key), val));
                    }
                }
            }
        }

        let delta = StateDelta {
            accounts: account_deltas,
            storage: storage_deltas,
            code: code_deltas,
        };

        let _ = self.state.apply_delta(delta);
    }
}

pub struct RevmReadOnlyAdapter<'a> {
    pub state: &'a dyn StateDB,
    pub caller: Option<RevmAddress>,
    pub current_height: u64,
}

impl<'a> RevmReadOnlyAdapter<'a> {
    pub fn new(state: &'a dyn StateDB, current_height: u64) -> Self {
        Self {
            state,
            caller: None,
            current_height,
        }
    }

    pub fn new_with_caller(
        state: &'a dyn StateDB,
        caller: RevmAddress,
        current_height: u64,
    ) -> Self {
        Self {
            state,
            caller: Some(caller),
            current_height,
        }
    }
}

impl<'a> Database for RevmReadOnlyAdapter<'a> {
    type Error = Infallible;

    fn basic(&mut self, address: RevmAddress) -> Result<Option<AccountInfo>, Self::Error> {
        let tola_addr = Address(*address.0);
        let acct_opt = self
            .state
            .get_historical_account(tola_types::BlockHeight(self.current_height), &tola_addr)
            .unwrap_or(None);

        let mut info = match acct_opt {
            Some(acct) => {
                let balance = U256::from(acct.balance.as_u128());
                let nonce = acct.nonce.0;

                let (code_hash, code) =
                    if acct.code_hash != Hash::ZERO && acct.code_hash.0 != KECCAK_EMPTY.0 {
                        let code_bytes = self.state.get_code(&acct.code_hash).unwrap_or(None);
                        let b = code_bytes.map(|c| Bytecode::new_raw(RevmBytes::from(c)));
                        (B256::from(acct.code_hash.0), b)
                    } else {
                        (KECCAK_EMPTY, None)
                    };

                AccountInfo {
                    balance,
                    nonce,
                    code_hash,
                    code,
                    ..Default::default()
                }
            }
            None => {
                if Some(address) == self.caller {
                    AccountInfo::default()
                } else {
                    return Ok(None);
                }
            }
        };

        if Some(address) == self.caller && info.balance < U256::from(u128::MAX) {
            info.balance = U256::from(u128::MAX);
        }

        Ok(Some(info))
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        let hash = Hash(code_hash.0);
        let code_bytes = self
            .state
            .get_code(&hash)
            .unwrap_or(None)
            .unwrap_or_default();
        Ok(Bytecode::new_raw(RevmBytes::from(code_bytes)))
    }

    fn storage(&mut self, address: RevmAddress, index: U256) -> Result<U256, Self::Error> {
        let tola_addr = Address(*address.0);
        let slot_key = Hash(index.to_be_bytes());
        let val_hash = self
            .state
            .get_historical_storage(
                tola_types::BlockHeight(self.current_height),
                &tola_addr,
                &slot_key,
            )
            .unwrap_or(Hash::ZERO);
        Ok(U256::from_be_bytes(val_hash.0))
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        if number >= self.current_height {
            return Ok(B256::ZERO);
        }
        if self.current_height > 256 && number < self.current_height - 256 {
            return Ok(B256::ZERO);
        }
        if let Ok(Some(hash)) = self
            .state
            .get_block_hash_by_height(tola_types::BlockHeight(number))
        {
            return Ok(B256::from(hash.0));
        }
        Ok(B256::ZERO)
    }
}
