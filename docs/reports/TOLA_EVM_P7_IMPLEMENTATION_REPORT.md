# TOLA EVM P7 — TARGETED FORENSIC REMEDIATION REPORT

## 1. Executive Summary & Remediation Authorization

Following the review of the initial P7 Implementation Report, targeted forensic remediation was performed to address four specific audit findings/blockers:
1. **ECRECOVER High-S Semantics**: Disentangled transaction-level signature validation (EIP-2 low-S rule) from EVM `ecrecover` (0x01) precompile execution semantics. Verified authoritative Ethereum Yellow Paper / EIP-2 specifications and proved empirically that the EVM precompile recovers signatures with high-S values and flipped parity $v$.
2. **Workspace Test Count Reconciliation**: Executed authoritative `cargo test --workspace`, establishing the exact count of **298 passed, 0 failed, 0 ignored** with a 1:1 matching target-by-target breakdown.
3. **Compiled Artifacts Inventory**: Verified filesystem `contracts/compiled/` contents, identifying exactly **21 JSON artifacts** (13 primary contracts, 8 OpenZeppelin helper libraries) mapped 1:1 to Solidity sources.
4. **Precompile Gas Accounting & Verification**: Implemented empirical bounded-gas testing (`callPrecompileWithBoundedGas`) across all Shanghai precompiles (0x01–0x09), confirming that each precompile fails boundedly on $(G - 1)$ gas and succeeds on exact $G$ gas.

**Zero Production Runtime Files Changed**: Zero lines of production code were altered.

---

## 2. Section A: ECRECOVER & Signature Semantics

### 2.1 Distinction Between Transaction Validation and Precompile Execution

| Dimension | Transaction Envelope Signature | EVM `ecrecover` Precompile (`0x01`) |
| :--- | :--- | :--- |
| **Governing Spec** | EIP-2 (Homestead) & TOLA Security Policy | Ethereum Yellow Paper (Appendix E) & EIP-2 |
| **Low-S Enforcement** | **STRICTLY ENFORCED** ($s \le n/2$) | **NOT ENFORCED** ($s < n$ allowed) |
| **High-S Behavior** | Rejected with `SignatureMalleability` error | Signature inverted and signer address recovered |
| **Rationale** | Prevents transaction hash malleability in consensus & mempool | Preserves mathematical ECDSA inversion capability |
| **Authoritative Text** | *"All transaction signatures whose s-value is greater than secp256k1n/2 are now considered invalid."* (EIP-2) | *"The ECDSA recover precompiled contract is not changed and will remain capable of recovering signatures with high s-values."* (EIP-2) |

### 2.2 Mathematical Specifications
For the secp256k1 curve:
- Curve order:
  $$n = \mathtt{0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141}$$
- Half-order bound:
  $$n/2 = \mathtt{0x7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF5D576E7357A4501DDFE92F46681B20A0}$$
- High-S counterpart:
  $$s_{\text{high}} = n - s_{\text{low}}$$
- Recovery identifier parity flip:
  $$v_{\text{high}} = \begin{cases} 28 & \text{if } v_{\text{low}} = 27 \\ 27 & \text{if } v_{\text{low}} = 28 \end{cases}$$

### 2.3 Empirical Verification Results
Tested in `test_p7_precompiles_complete_shanghai_matrix` and `test_p7_solidity_ecrecover_integration`:
1. **Low-S Input**: $(h, v_{\text{low}}, r, s_{\text{low}}) \implies \mathtt{0x5869dbf998263fbda12d9472500fce138e062b75}$ (deployer address) — **PASS**
2. **High-S Input with Flipped $v$**: $(h, v_{\text{high}}, r, s_{\text{high}}) \implies \mathtt{0x5869dbf998263fbda12d9472500fce138e062b75}$ (deployer address) — **PASS**
3. **High-S Input with Unflipped $v$**: $(h, v_{\text{low}}, r, s_{\text{high}}) \implies \mathtt{address} \ne \text{deployer}$ — **PASS**
4. **Invalid $v$ ($v=0$)**: Returns empty returndata (`0x`) — **PASS**
5. **Out of Range $s$ ($s \ge n$)**: Returns `0x0000000000000000000000000000000000000000` — **PASS**
6. **Consensus Transaction Validation**: Transaction with $s > n/2$ rejected with `InvariantViolation::SignatureMalleability` in `security.rs` — **PASS**

---

## 3. Section B: Authoritative Workspace Test Results

### 3.1 Execution Command
```bash
cargo test --workspace
```

### 3.2 Global Summary
- **Total Tests Passed**: **298**
- **Total Tests Failed**: **0**
- **Total Tests Ignored**: **0**
- **Doc-tests Run**: 0 across 13 crates

### 3.3 Authoritative Breakdown by Test Binary

| # | Test Target / Binary | Passed | Failed | Ignored |
| :---: | :--- | :---: | :---: | :---: |
| 1 | `tola-cli` (unit `src/main.rs`) | 13 | 0 | 0 |
| 2 | `tola-cli` (integration `tests/wallet_integration_test.rs`) | 13 | 0 | 0 |
| 3 | `tola-codec` (unit `src/lib.rs`) | 12 | 0 | 0 |
| 4 | `tola-consensus` (unit `src/lib.rs`) | 20 | 0 | 0 |
| 5 | `tola-core` (unit `src/lib.rs`) | 0 | 0 | 0 |
| 6 | `tola-crypto` (unit `src/lib.rs`) | 22 | 0 | 0 |
| 7 | `tola-execution` (unit `src/lib.rs`) | 11 | 0 | 0 |
| 8 | `tola-mempool` (unit `src/lib.rs`) | 6 | 0 | 0 |
| 9 | `tola-node` (unit `src/lib.rs`) | 21 | 0 | 0 |
| 10 | `tola-node` (unit `src/main.rs`) | 0 | 0 | 0 |
| 11 | `tola-node` (`tests/autonomous_testnet_audit_test.rs`) | 11 | 0 | 0 |
| 12 | `tola-node` (`tests/external_asset_architecture_test.rs`) | 5 | 0 | 0 |
| 13 | `tola-node` (`tests/fuzz_and_chaos_test.rs`) | 4 | 0 | 0 |
| 14 | `tola-node` (`tests/mainnet_tokenomics_governance_test.rs`) | 5 | 0 | 0 |
| 15 | `tola-node` (`tests/multi_process_testnet.rs`) | 4 | 0 | 0 |
| 16 | `tola-node` (`tests/p1_evm_persistence_test.rs`) | 4 | 0 | 0 |
| 17 | `tola-node` (`tests/p2_evm_logs_filter_test.rs`) | 5 | 0 | 0 |
| 18 | `tola-node` (`tests/p3_rpc_compatibility_test.rs`) | 1 | 0 | 0 |
| 19 | `tola-node` (`tests/p4_websocket_subscription_test.rs`) | 9 | 0 | 0 |
| 20 | `tola-node` (`tests/p5_rpc_batch_and_tooling_test.rs`) | 11 | 0 | 0 |
| 21 | `tola-node` (`tests/p6_smart_contract_test.rs`) | 18 | 0 | 0 |
| 22 | `tola-node` (`tests/p7_solidity_and_precompiles_test.rs`) | **12** | 0 | 0 |
| 23 | `tola-node` (`tests/security_audit_test.rs`) | 7 | 0 | 0 |
| 24 | `tola-node` (`tests/testnet_stress_test.rs`) | 4 | 0 | 0 |
| 25 | `tola-p2p` (unit `src/lib.rs`) | 12 | 0 | 0 |
| 26 | `tola-rpc` (unit `src/lib.rs`) | 26 | 0 | 0 |
| 27 | `tola-sdk` (unit `src/lib.rs`) | 10 | 0 | 0 |
| 28 | `tola-state` (unit `src/lib.rs`) | 4 | 0 | 0 |
| 29 | `tola-storage` (unit `src/lib.rs`) | 10 | 0 | 0 |
| 30 | `tola-types` (unit `src/lib.rs`) | 18 | 0 | 0 |
| — | **Total** | **298** | **0** | **0** |

$$\sum = 13 + 13 + 12 + 20 + 0 + 22 + 11 + 6 + 21 + 0 + 11 + 5 + 4 + 5 + 4 + 4 + 5 + 1 + 9 + 11 + 18 + 12 + 7 + 4 + 12 + 26 + 10 + 4 + 10 + 18 = 298$$

---

## 4. Section C: Compiled Artifacts Inventory

### 4.1 Filesystem Content Verification
Inspection of `contracts/compiled/` confirms exactly **21 `.json` artifact files** generated by `solc 0.8.24` (`--evm-version shanghai`):

### 4.2 Source-to-Artifact Mapping

| # | Artifact File | Byte Size | Source File | Contract / Role |
| :---: | :--- | :---: | :--- | :--- |
| 1 | `MinimalState.json` | 6,104 | `contracts/src/MinimalState.sol` | State variable getter/setter with events |
| 2 | `ERC20Mock.json` | 17,952 | `contracts/src/ERC20Mock.sol` | OpenZeppelin v5.0.2 ERC-20 implementation |
| 3 | `ERC721Mock.json` | 32,113 | `contracts/src/ERC721Mock.sol` | OpenZeppelin v5.0.2 ERC-721 implementation |
| 4 | `ERC721ReceiverMock.json` | 6,662 | `contracts/src/ERC721Mock.sol` | ERC-721 callback receiver mock |
| 5 | `ERC1155Mock.json` | 30,086 | `contracts/src/ERC1155Mock.sol` | OpenZeppelin v5.0.2 ERC-1155 implementation |
| 6 | `ERC1155ReceiverMock.json` | 8,516 | `contracts/src/ERC1155Mock.sol` | ERC-1155 callback receiver mock |
| 7 | `PrecompileCaller.json` | 12,218 | `contracts/src/PrecompileCaller.sol` | Precompile caller & bounded gas harness |
| 8 | `CallRouter.json` | 5,408 | `contracts/src/CallRouter.sol` | Inter-contract CALL/STATICCALL router |
| 9 | `TargetContract.json` | 5,019 | `contracts/src/CallRouter.sol` | Callee target with state mutator & revert |
| 10 | `CustomErrorTest.json` | 3,063 | `contracts/src/CustomErrorTest.sol` | Solidity 0.8+ custom error `Unauthorized` |
| 11 | `EIP1967ProxyWrapper.json` | 3,715 | `contracts/src/EIP1967Proxy.sol` | OpenZeppelin v5.0.2 ERC1967Proxy deployer |
| 12 | `UUPSImplementationMock.json` | 12,479 | `contracts/src/UUPSImplementationMock.sol` | OpenZeppelin UUPS upgradeable V1 |
| 13 | `UUPSImplementationV2Mock.json` | 11,387 | `contracts/src/UUPSImplementationMock.sol` | OpenZeppelin UUPS upgradeable V2 |
| 14 | `Address.json` | 1,059 | `@openzeppelin/contracts/utils/Address.sol` | OpenZeppelin utility library |
| 15 | `Arrays.json` | 538 | `@openzeppelin/contracts/utils/Arrays.sol` | OpenZeppelin utility library |
| 16 | `ERC1967Proxy.json` | 3,748 | `@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol` | OpenZeppelin proxy contract |
| 17 | `ERC1967Utils.json` | 2,235 | `@openzeppelin/contracts/proxy/ERC1967/ERC1967Utils.sol` | OpenZeppelin proxy storage slots |
| 18 | `Math.json` | 634 | `@openzeppelin/contracts/utils/math/Math.sol` | OpenZeppelin utility library |
| 19 | `SignedMath.json` | 551 | `@openzeppelin/contracts/utils/math/SignedMath.sol` | OpenZeppelin utility library |
| 20 | `StorageSlot.json` | 548 | `@openzeppelin/contracts/utils/StorageSlot.sol` | OpenZeppelin storage slot library |
| 21 | `Strings.json` | 876 | `@openzeppelin/contracts/utils/Strings.sol` | OpenZeppelin string formatting |

**Total .json Artifacts**: 21  
**Total Source Files (.sol)**: 9  
**Duplicate or Missing Artifacts**: 0  

---

## 5. Section D: Precompile Gas Forensic Verification

Precompile gas consumption was forensically verified using `callPrecompileWithBoundedGas`:
$$(success, returndata) = \mathtt{target.staticcall\{gas: gasLimit\}(input)}$$
For each precompile, two tests were executed:
- **Under-budget**: $gasLimit = G - 1 \implies success = \mathtt{false}$ (reverts boundedly with Out Of Gas)
- **Exact budget**: $gasLimit = G \implies success = \mathtt{true}$ (executes and returns valid returndata)

### 5.1 Gas Accounting Matrix (0x01–0x09)

| Address | Precompile | Test Input | Specification Gas Model | Budget $G$ | Under $G-1$ | Exact $G$ | Status |
| :--- | :--- | :--- | :--- | :---: | :---: | :---: | :---: |
| `0x01` | **ECRecover** | 128 bytes $(h, v, r, s)$ | Fixed 3,000 gas (Yellow Paper) | 3,000 | `false` | `true` | **PASS** |
| `0x02` | **SHA-256** (A) | 32 bytes (1 word) | $60 + 12 \times \lceil \text{len}/32 \rceil = 72$ | 72 | `false` | `true` | **PASS** |
| `0x02` | **SHA-256** (B) | 46 bytes (2 words) | $60 + 12 \times \lceil \text{len}/32 \rceil = 84$ | 84 | `false` | `true` | **PASS** |
| `0x03` | **RIPEMD-160** (A) | 20 bytes (1 word) | $600 + 120 \times \lceil \text{len}/32 \rceil = 720$ | 720 | `false` | `true` | **PASS** |
| `0x03` | **RIPEMD-160** (B) | 37 bytes (2 words) | $600 + 120 \times \lceil \text{len}/32 \rceil = 840$ | 840 | `false` | `true` | **PASS** |
| `0x04` | **Identity** (A) | 32 bytes (1 word) | $15 + 3 \times \lceil \text{len}/32 \rceil = 18$ | 18 | `false` | `true` | **PASS** |
| `0x04` | **Identity** (B) | 64 bytes (2 words) | $15 + 3 \times \lceil \text{len}/32 \rceil = 21$ | 21 | `false` | `true` | **PASS** |
| `0x05` | **ModExp** | 99 bytes (1, 1, 1, B=3, E=5, M=7) | EIP-2565 $\max(200, \lfloor \text{complexity} / 3 \rfloor)$ | 200 | `false` | `true` | **PASS** |
| `0x06` | **ecAdd** | 128 bytes (two points) | Fixed 150 gas (EIP-1108) | 150 | `false` | `true` | **PASS** |
| `0x07` | **ecMul** | 96 bytes (point + scalar) | Fixed 6,000 gas (EIP-1108) | 6,000 | `false` | `true` | **PASS** |
| `0x08` | **ecPairing** (A) | 0 bytes ($k=0$, empty) | $34,000 \times k + 45,000 = 45,000$ (EIP-1108) | 45,000 | `false` | `true` | **PASS** |
| `0x08` | **ecPairing** (B) | 192 bytes ($k=1$, 1 pair) | $34,000 \times 1 + 45,000 = 79,000$ (EIP-1108) | 79,000 | `false` | `true` | **PASS** |
| `0x09` | **Blake2f** (A) | 213 bytes ($r=12$ rounds) | Exactly $r = 12$ gas (EIP-152) | 12 | `false` | `true` | **PASS** |
| `0x09` | **Blake2f** (B) | 213 bytes ($r=24$ rounds) | Exactly $r = 24$ gas (EIP-152) | 24 | `false` | `true` | **PASS** |

---

## 6. Section E: Production Code Invariance Audit

Audited against `git diff`:
- `crates/tola-execution/src`: **0 files changed** (0 lines added, 0 lines removed)
- `crates/tola-storage/src`: **0 files changed** (0 lines added, 0 lines removed)
- `crates/tola-rpc/src`: **0 files changed** (0 lines added, 0 lines removed)
- `crates/tola-consensus/src`: **0 files changed** (0 lines added, 0 lines removed)
- `crates/tola-codec/src`: **0 files changed** (0 lines added, 0 lines removed)
- `crates/tola-crypto/src`: **0 files changed** (0 lines added, 0 lines removed)
- `crates/tola-types/src`: **0 files changed** (0 lines added, 0 lines removed)
- `crates/tola-node/src`: **0 files changed** (0 lines added, 0 lines removed)

**Production runtime files changed**: **0**

The only modifications in the repository are:
1. `crates/tola-node/Cargo.toml`: Addition of `[dev-dependencies]` (`k256`, `sha2`, `ripemd`, `num-bigint`) for test oracles.
2. `crates/tola-node/tests/p7_solidity_and_precompiles_test.rs`: Addition of integration tests.
3. `contracts/`: Committed real Solidity sources and deterministic compiled JSON artifacts.

---

## 7. Section F: P0–P6 Regression Status

Full workspace regression verification confirms zero regressions across all prior EVM and protocol phases:

| Phase | Integration Test Target | Status |
| :--- | :--- | :--- |
| **P1** | `tests/p1_evm_persistence_test.rs` (4 tests) | **PASS** |
| **P2** | `tests/p2_evm_logs_filter_test.rs` (5 tests) | **PASS** |
| **P3** | `tests/p3_rpc_compatibility_test.rs` (1 test) | **PASS** |
| **P4** | `tests/p4_websocket_subscription_test.rs` (9 tests) | **PASS** |
| **P5** | `tests/p5_rpc_batch_and_tooling_test.rs` (11 tests) | **PASS** |
| **P6** | `tests/p6_smart_contract_test.rs` (18 tests) | **PASS** |
| **P7** | `tests/p7_solidity_and_precompiles_test.rs` (12 tests) | **PASS** |
| **All Other** | Protocol, Consensus, P2P, Storage, Cryptography, Security (238 tests) | **PASS** |

- `cargo clippy --all-targets --all-features -- -D warnings`: **Clean (0 warnings)**
- `cargo fmt --check`: **Clean**

---

## 8. Section G: Final Status

All identified blockers and forensic audit findings have been resolved with mathematical and empirical evidence.

IMPLEMENTATION COMPLETE — READY FOR FORENSIC AUDIT
