use revm::{
    context::{BlockEnv, CfgEnv, Context, TxEnv},
    context_interface::transaction::{AccessList, AccessListItem},
    handler::{ExecuteCommitEvm, MainBuilder, MainContext},
    inspector::InspectEvm,
    primitives::{hardfork::SpecId, Address as RevmAddress, Bytes as RevmBytes, TxKind, U256},
};
use tola_core::StateDB;
use tola_types::{Address, Amount, BlockHeight, Gas, Hash, Log, Timestamp, Transaction};

use crate::evm_adapter::{RevmStateAdapter, TolaEvmInspector};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvmExecutionStatus {
    Success,
    Revert,
    Halt(String),
}

pub struct EvmExecutionOutcome {
    pub success: bool,
    pub gas_used: Gas,
    pub logs: Vec<Log>,
    pub return_data: Vec<u8>,
    pub created_address: Option<Address>,
    pub status: EvmExecutionStatus,
}

pub struct EvmExecutor;

impl EvmExecutor {
    pub fn execute_transaction(
        tx: &Transaction,
        height: BlockHeight,
        timestamp: Timestamp,
        base_fee: Amount,
        proposer: Address,
        block_gas_limit: Gas,
        state: &mut dyn StateDB,
    ) -> Result<EvmExecutionOutcome, String> {
        let mut block_env = BlockEnv::default();
        block_env.number = U256::from(height.0);
        block_env.timestamp = U256::from(timestamp.0);
        block_env.beneficiary = RevmAddress::from(proposer.0);
        block_env.basefee = base_fee.as_u128() as u64;
        block_env.gas_limit = block_gas_limit.as_u64();

        let mut tx_env = TxEnv::default();
        tx_env.caller = RevmAddress::from(tx.from.0);
        tx_env.kind = match tx.to {
            Some(to) => TxKind::Call(RevmAddress::from(to.0)),
            None => TxKind::Create,
        };
        tx_env.value = U256::from(tx.value.as_u128());
        tx_env.data = RevmBytes::from(tx.data.clone());
        tx_env.gas_limit = tx.gas_limit.as_u64();
        let effective_price = tx.effective_gas_price(base_fee);
        tx_env.gas_price = effective_price.as_u128();
        tx_env.gas_priority_fee = Some(tx.max_priority_fee_per_gas.as_u128());
        tx_env.chain_id = Some(tx.chain_id.0);
        tx_env.nonce = tx.nonce.0;
        tx_env.tx_type = tx.tx_type;
        let access_list: Vec<AccessListItem> = tx
            .access_list
            .iter()
            .map(|item| AccessListItem {
                address: RevmAddress::from(item.address.0),
                storage_keys: item
                    .storage_keys
                    .iter()
                    .map(|k| revm::primitives::B256::from(k.0))
                    .collect(),
            })
            .collect();
        tx_env.access_list = AccessList(access_list);

        let mut cfg_env = CfgEnv::default();
        cfg_env.spec = SpecId::SHANGHAI;
        cfg_env.chain_id = tx.chain_id.0;

        let db = RevmStateAdapter::new(state, height.0);
        let ctx = Context::mainnet()
            .with_db(db)
            .with_block(block_env)
            .with_cfg(cfg_env);

        let inspector = TolaEvmInspector::default();
        let mut evm = ctx.build_mainnet_with_inspector(inspector);

        let exec_result_and_state = evm
            .inspect_tx(tx_env)
            .map_err(|e| format!("EVM execution failed: {e:?}"))?;

        let exec_result = exec_result_and_state.result;
        let mut changes = exec_result_and_state.state;

        // Discard/reverse revm's automatic priority fee credit to beneficiary (proposer)
        // per Approved Decisions D01/D04: 0% directly to proposer, 75% to Validator Pool, 25% to Treasury.
        let revm_proposer = RevmAddress::from(proposer.0);
        let gas_used_val = exec_result.tx_gas_used();
        let priority_fee_per_gas = effective_price.as_u128().saturating_sub(base_fee.as_u128());
        let beneficiary_reward = U256::from(gas_used_val as u128 * priority_fee_per_gas);

        if beneficiary_reward > U256::ZERO {
            if let Some(beneficiary_acct) = changes.get_mut(&revm_proposer) {
                beneficiary_acct.info.balance = beneficiary_acct
                    .info
                    .balance
                    .saturating_sub(beneficiary_reward);
            }
        }

        evm.commit(changes);

        let success = exec_result.is_success();
        let gas_used = Gas(exec_result.tx_gas_used());
        let return_data = exec_result.output().map(|b| b.to_vec()).unwrap_or_default();
        let created_address = exec_result.created_address().map(|a| Address(*a.0));
        let logs = exec_result
            .logs()
            .iter()
            .map(|l| Log {
                address: Address(*l.address.0),
                topics: l.topics().iter().map(|t| Hash(t.0)).collect(),
                data: l.data.data.to_vec(),
            })
            .collect();

        let status = match &exec_result {
            revm::context_interface::result::ExecutionResult::Success { .. } => {
                EvmExecutionStatus::Success
            }
            revm::context_interface::result::ExecutionResult::Revert { .. } => {
                EvmExecutionStatus::Revert
            }
            revm::context_interface::result::ExecutionResult::Halt { reason, .. } => {
                EvmExecutionStatus::Halt(format!("{reason:?}"))
            }
        };

        Ok(EvmExecutionOutcome {
            success,
            gas_used,
            logs,
            return_data,
            created_address,
            status,
        })
    }

    pub fn simulate_call(
        tx: &Transaction,
        height: BlockHeight,
        timestamp: Timestamp,
        base_fee: Amount,
        proposer: Address,
        block_gas_limit: Gas,
        state: &dyn StateDB,
    ) -> Result<EvmExecutionOutcome, String> {
        use crate::evm_adapter::RevmReadOnlyAdapter;
        use revm::inspector::InspectEvm;

        let mut block_env = BlockEnv::default();
        block_env.number = U256::from(height.0);
        block_env.timestamp = U256::from(timestamp.0);
        block_env.beneficiary = RevmAddress::from(proposer.0);
        block_env.basefee = base_fee.as_u128() as u64;
        block_env.gas_limit = block_gas_limit.as_u64();

        let mut tx_env = TxEnv::default();
        tx_env.caller = RevmAddress::from(tx.from.0);
        tx_env.kind = match tx.to {
            Some(to) => TxKind::Call(RevmAddress::from(to.0)),
            None => TxKind::Create,
        };
        tx_env.value = U256::from(tx.value.as_u128());
        tx_env.data = RevmBytes::from(tx.data.clone());
        if tx.gas_limit.as_u64() > block_gas_limit.as_u64() {
            return Err("gas limit exceeds block gas limit".to_string());
        }
        tx_env.gas_limit = tx.gas_limit.as_u64();
        let effective_price = if tx.max_fee_per_gas.as_u128() > 0 {
            tx.effective_gas_price(base_fee).as_u128()
        } else {
            base_fee.as_u128()
        };
        tx_env.gas_price = effective_price;
        tx_env.gas_priority_fee = Some(tx.max_priority_fee_per_gas.as_u128());
        tx_env.chain_id = Some(tx.chain_id.0);
        tx_env.nonce = tx.nonce.0;
        tx_env.tx_type = tx.tx_type;
        let access_list: Vec<AccessListItem> = tx
            .access_list
            .iter()
            .map(|item| AccessListItem {
                address: RevmAddress::from(item.address.0),
                storage_keys: item
                    .storage_keys
                    .iter()
                    .map(|k| revm::primitives::B256::from(k.0))
                    .collect(),
            })
            .collect();
        tx_env.access_list = AccessList(access_list);

        let mut cfg_env = CfgEnv::default();
        cfg_env.spec = SpecId::SHANGHAI;
        cfg_env.chain_id = tx.chain_id.0;
        cfg_env.disable_nonce_check = true;

        let db = RevmReadOnlyAdapter::new_with_caller(state, tx_env.caller, height.0);
        let ctx = Context::mainnet()
            .with_db(db)
            .with_block(block_env)
            .with_cfg(cfg_env);

        let inspector = TolaEvmInspector::default();
        let mut evm = ctx.build_mainnet_with_inspector(inspector);

        let exec_result_and_state = evm
            .inspect_tx(tx_env)
            .map_err(|e| format!("EVM simulation failed: {e:?}"))?;

        let exec_result = exec_result_and_state.result;
        let success = exec_result.is_success();
        let gas_used = Gas(exec_result.tx_gas_used());
        let return_data = exec_result.output().map(|b| b.to_vec()).unwrap_or_default();
        let created_address = exec_result.created_address().map(|a| Address(*a.0));
        let logs = exec_result
            .logs()
            .iter()
            .map(|l| Log {
                address: Address(*l.address.0),
                topics: l.topics().iter().map(|t| Hash(t.0)).collect(),
                data: l.data.data.to_vec(),
            })
            .collect();

        let status = match &exec_result {
            revm::context_interface::result::ExecutionResult::Success { .. } => {
                EvmExecutionStatus::Success
            }
            revm::context_interface::result::ExecutionResult::Revert { .. } => {
                EvmExecutionStatus::Revert
            }
            revm::context_interface::result::ExecutionResult::Halt { reason, .. } => {
                EvmExecutionStatus::Halt(format!("{reason:?}"))
            }
        };

        Ok(EvmExecutionOutcome {
            success,
            gas_used,
            logs,
            return_data,
            created_address,
            status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tola_state::MemoryStateJournal;
    use tola_types::{AccountState, Amount, ChainId, Nonce, Signature, FLAG_CONTRACT};

    #[test]
    fn test_evm_contract_deployment_and_call() {
        let mut state = MemoryStateJournal::new();
        let sender = Address([0x10; 20]);
        let proposer = Address([0x99; 20]);

        // Fund sender
        state.set_account(
            sender,
            AccountState::new_with_balance(Amount::from_u128(10_000_000_000)),
        );

        // Init code: returns 10 bytes of runtime code that returns 42 (0x2a)
        // 600a 80 600b 6000 39 6000 f3 602a 6000 52 6020 6000 f3
        let init_code = hex::decode("600a80600b6000396000f3602a60005260206000f3").unwrap();

        let deploy_tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce::ZERO,
            from: sender,
            to: None, // Contract creation
            value: Amount::ZERO,
            gas_limit: Gas(200_000),
            max_fee_per_gas: Amount::from_u128(20),
            max_priority_fee_per_gas: Amount::from_u128(2),
            valid_until_block: BlockHeight(100),
            data: init_code,
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0u8; 32],
                s: [0u8; 32],
                y_parity: 0,
                chain_id: ChainId::MAINNET.0,
            },
            tx_type: 2,
            access_list: vec![],
        };

        let deploy_res = EvmExecutor::execute_transaction(
            &deploy_tx,
            BlockHeight(1),
            Timestamp(1000),
            Amount::from_u128(10),
            proposer,
            Gas(30_000_000),
            &mut state,
        )
        .expect("Deployment must succeed");

        assert!(deploy_res.success);
        let contract_addr = deploy_res
            .created_address
            .expect("Must have created address");

        // Verify contract exists in StateDB
        let contract_acct = state.get_account(&contract_addr).unwrap().unwrap();
        assert_eq!(contract_acct.flags & FLAG_CONTRACT, FLAG_CONTRACT);
        assert_ne!(contract_acct.code_hash, Hash::ZERO);

        let code = state.get_code(&contract_acct.code_hash).unwrap().unwrap();
        assert_eq!(code, hex::decode("602a60005260206000f3").unwrap());

        // Now call the contract
        let call_tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(1),
            from: sender,
            to: Some(contract_addr),
            value: Amount::ZERO,
            gas_limit: Gas(100_000),
            max_fee_per_gas: Amount::from_u128(20),
            max_priority_fee_per_gas: Amount::from_u128(2),
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0u8; 32],
                s: [0u8; 32],
                y_parity: 0,
                chain_id: ChainId::MAINNET.0,
            },
            tx_type: 2,
            access_list: vec![],
        };

        let call_res = EvmExecutor::execute_transaction(
            &call_tx,
            BlockHeight(1),
            Timestamp(1001),
            Amount::from_u128(10),
            proposer,
            Gas(30_000_000),
            &mut state,
        )
        .expect("Call must succeed");

        assert!(call_res.success);
        assert_eq!(call_res.return_data.len(), 32);
        assert_eq!(call_res.return_data[31], 0x2a); // 42

        // Simulate call
        let sim_res = EvmExecutor::simulate_call(
            &call_tx,
            BlockHeight(1),
            Timestamp(1001),
            Amount::from_u128(10),
            proposer,
            Gas(30_000_000),
            &state,
        )
        .expect("Simulate call must succeed");

        assert!(sim_res.success);
        assert_eq!(sim_res.return_data[31], 0x2a);
    }

    #[test]
    fn test_selfdestruct_reserved_address_blocked() {
        let mut state = MemoryStateJournal::new();
        let sender = Address([0x20; 20]);
        let proposer = Address([0x99; 20]);

        state.set_account(
            sender,
            AccountState::new_with_balance(Amount::from_u128(10_000_000_000)),
        );

        // Runtime code: 61 77 21 ff -> PUSH2 0x7721, SELFDESTRUCT
        // Init code: 600480600b6000396000f3 617721ff
        let init_code = hex::decode("600480600b6000396000f3617721ff").unwrap();

        let deploy_tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce::ZERO,
            from: sender,
            to: None,
            value: Amount::ZERO,
            gas_limit: Gas(200_000),
            max_fee_per_gas: Amount::from_u128(20),
            max_priority_fee_per_gas: Amount::from_u128(2),
            valid_until_block: BlockHeight(100),
            data: init_code,
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0u8; 32],
                s: [0u8; 32],
                y_parity: 0,
                chain_id: ChainId::MAINNET.0,
            },
            tx_type: 2,
            access_list: vec![],
        };

        let deploy_res = EvmExecutor::execute_transaction(
            &deploy_tx,
            BlockHeight(1),
            Timestamp(1000),
            Amount::from_u128(10),
            proposer,
            Gas(30_000_000),
            &mut state,
        )
        .expect("Deployment must succeed");

        assert!(deploy_res.success);
        let contract_addr = deploy_res.created_address.unwrap();

        // Now call the contract: it tries to SELFDESTRUCT to 0x...7721
        let destroy_tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(1),
            from: sender,
            to: Some(contract_addr),
            value: Amount::ZERO,
            gas_limit: Gas(100_000),
            max_fee_per_gas: Amount::from_u128(20),
            max_priority_fee_per_gas: Amount::from_u128(2),
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0u8; 32],
                s: [0u8; 32],
                y_parity: 0,
                chain_id: ChainId::MAINNET.0,
            },
            tx_type: 2,
            access_list: vec![],
        };

        let destroy_res = EvmExecutor::execute_transaction(
            &destroy_tx,
            BlockHeight(1),
            Timestamp(1002),
            Amount::from_u128(10),
            proposer,
            Gas(30_000_000),
            &mut state,
        )
        .expect("Execution completes");

        // The transaction MUST revert because TolaEvmInspector halts on 0x...7721!
        assert!(
            !destroy_res.success,
            "SELFDESTRUCT to 0x...7721 must be reverted!"
        );

        // Contract must still exist!
        let contract_acct = state.get_account(&contract_addr).unwrap();
        assert!(contract_acct.is_some(), "Contract must NOT be destroyed!");
    }
}
