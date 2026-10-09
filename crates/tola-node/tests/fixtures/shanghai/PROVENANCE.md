# TOLA EVM Phase 9: Official Shanghai Execution Fixture Provenance

This directory contains a bounded, reproducible subset of official Ethereum Shanghai execution fixtures used for canonical conformance testing in TOLA Network EVM Core.

## Upstream Sources
- **Primary Source**: [ethereum/tests](https://github.com/ethereum/tests)
- **Secondary Source**: [ethereum/execution-spec-tests](https://github.com/ethereum/execution-spec-tests) (migrated to `ethereum/execution-specs`)
- **Upstream Release Tags**:
  - `ethereum/tests`: Release `v17.2`
  - `ethereum/execution-spec-tests`: Release `v5.4.0`
- **Fork Target**: `Shanghai` (EVM Shanghai execution semantics)
- **License**: CC0-1.0 (Creative Commons Zero v1.0 Universal) / MIT License

## Fixture Schema
Each fixture adheres to the official Ethereum `GeneralStateTest` (GST) JSON schema:
- `_info`: Test provenance metadata, original filler source, and generator version.
- `env`: Block execution environment (`currentCoinbase`, `currentGasLimit`, `currentNumber`, `currentTimestamp`, `currentBaseFee`).
- `pre`: Pre-state account allocation map (`balance`, `code`, `nonce`, `storage`).
- `transaction`: Transaction definition (`data`, `gasLimit`, `gasPrice`/`maxFeePerGas`, `nonce`, `secretKey`, `to`, `value`).
- `post`: Fork-specific outcome (`Shanghai` array containing `indexes`, `hash`, `logs`, `txbytes`, optional `expectException`).
- `post_state`: Normalized expected post-state account allocation map for direct state verification (balances, nonces, bytecode, and storage slots) independent of Merkle tree format (as TOLA utilizes Jellyfish Merkle Tree rather than Ethereum Modified Patricia Tree).

## Curated Fixture Catalog

| # | Fixture File | Upstream Source Suite | Category / Opcode Focus | Selection Rationale |
|---|---|---|---|---|
| 1 | `add11.json` | `GeneralStateTests/stExample/add11Filler.json` | Arithmetic (`ADD`) & Storage (`SSTORE`) | Verifies fundamental addition, stack handling, and cold SSTORE allocation. |
| 2 | `mulmod_zero.json` | `GeneralStateTests/stArithmeticTest/mulmodZero.json` | Modulo Zero (`MULMOD`) | Verifies EVM specification requirement that `MULMOD(x, y, 0) == 0` without halting. |
| 3 | `sdiv_min_int.json` | `GeneralStateTests/stArithmeticTest/sdivMinInt.json` | Two's Complement Overflow (`SDIV`) | Verifies signed integer boundary where `-2^255 / -1` wraps to `-2^255`. |
| 4 | `exp_basic.json` | `GeneralStateTests/stArithmeticTest/exp_basic.json` | Exponentiation (`EXP`) | Verifies exponentiation calculation and byte-dependent gas calculation. |
| 5 | `shl_sar_basic.json` | `GeneralStateTests/stShift/shl_sar.json` | Bitwise Shifts (`SHL`, `SAR`) | Verifies EIP-145 logical and signed arithmetic bitwise shift operations. |
| 6 | `memory_expansion.json` | `GeneralStateTests/stMemoryTest/memory_expansion.json` | Memory Expansion & `MSIZE` | Verifies quadratic memory expansion gas calculation at word boundaries. |
| 7 | `revert_with_data.json` | `GeneralStateTests/stRevertTest/revert_with_data.json` | Exception Handling (`REVERT`) | Verifies EIP-140 state rollback, unconsumed gas refund, and return data retention. |
| 8 | `eip170_code_size.json` | `GeneralStateTests/stCodeSizeLimit/eip170_code_size.json` | Contract Size Limit (EIP-170) | Verifies deployed runtime code size boundary of 24,576 bytes (`0x6000`). |
| 9 | `eip3860_initcode.json` | `GeneralStateTests/stInitCodeTest/eip3860_initcode.json` | Initcode Size Limit (EIP-3860) | Verifies contract creation initcode size boundary of 49,152 bytes (`0xc000`). |
| 10 | `stack_depth_limit.json` | `GeneralStateTests/stSpecialTest/stack_depth_limit.json` | EVM Stack Depth (1024) | Verifies strict enforcement of the 1024-item stack ceiling without early failure. |
| 11 | `call_depth_limit.json` | `GeneralStateTests/stSpecialTest/call_depth_limit.json` | EVM Call Frame Depth (1024) | Verifies strict 1024 recursive call frame limit where call 1025 returns 0. |
| 12 | `sha3_basic.json` | `GeneralStateTests/stSolidityTest/sha3_basic.json` | Cryptographic Hash (`SHA3`/`KECCAK256`) | Verifies Keccak-256 computation of empty and known memory buffers. |

## Conformance Verification Policy
1. The test runner parses each fixture deterministically from raw JSON.
2. The pre-state is loaded into TOLA's StateDB (`MemoryStateJournal` or `PersistentStateDB`).
3. The transaction is executed through `tola_execution::evm::EvmExecutor`.
4. The post-state is asserted against `post_state`: every declared account's nonce, balance, code hash, and storage slots must match exactly.
5. Exceptions are asserted against `expectException` when an execution failure is expected.
