# ADR-0015: Real Solidity Compilation Standard, EVM Target Policy & Precompile Verification Baseline

## Status
ACCEPTED (TOLA EVM Phase 7)

## Context
Phases 0 through 6 (P0–P6) established TOLA's EVM execution engine (REVM 43.0.3, Shanghai revision), unified StateDB persistence, transaction envelopes (Legacy and Type-2), JSON-RPC compatibility, and basic smart contract execution using hand-authored EVM bytecodes.

To establish production developer readiness and ecosystem compatibility, TOLA must prove that:
1. Contracts compiled by the official Solidity compiler (`solc`) execute deterministically on TOLA's consensus state machine.
2. Canonical industry smart contract libraries (OpenZeppelin Contracts v5.0.2) deploy, mutate storage, emit logs, handle custom error reverts, and maintain upgradeable proxy storage isolation without friction.
3. All Ethereum Shanghai precompiles (addresses `0x01` through `0x09`) evaluate with bit-for-bit mathematical accuracy against independent cryptographic oracles and official Ethereum test vectors.
4. TOLA node consensus runtime maintains strict isolation from external developer tooling (Node.js, npm, Foundry, Hardhat).

## Decision

1. **Compiler Baseline:**
   - **Solidity Version:** Pinned to **`solc 0.8.24`**.
   - **EVM Specification:** Explicitly pinned to **`--evm-version shanghai`** (or `evmVersion: "shanghai"` in standard JSON input). Because `solc 0.8.24` defaults to Cancun (which emits `MCOPY` and `TSTORE`/`TLOAD`), all TOLA smart contract compilation MUST specify Shanghai explicitly.
   - **Optimizer:** Enabled with 200 runs (`optimizer: { enabled: true, runs: 200 }`).
   - **Metadata Invariance:** Configured with `metadata: { bytecodeHash: "none" }` to prevent non-deterministic CBOR metadata differences across platforms.

2. **Committed Deterministic Artifacts:**
   - Pre-compiled JSON artifacts containing exact ABI, initcode (`bytecode`), and runtime bytecode (`deployedBytecode`) are committed under `contracts/compiled/`.
   - The Rust integration test harness (`cargo test`) consumes these committed artifacts directly. Normal node compilation and CI test runs require **zero external network downloads**, zero npm installs, and zero external process spawning.
   - A dedicated verification script (`contracts/compile.js`) provides an audit trail allowing developers to recompile the Solidity sources from scratch and verify artifact byte-for-byte equivalence.

3. **OpenZeppelin Contracts Baseline:**
   - Pinned to **`@openzeppelin/contracts v5.0.2`**.
   - Tests execute real OpenZeppelin v5 implementations of:
     - `ERC20` (exercising modern `draft-IERC6093` custom error reverts: `ERC20InsufficientBalance`);
     - `ERC721` with `ERC721URIStorage` and `IERC721Receiver` safe transfer callbacks;
     - `ERC1155` multi-token with single and batch receiver callbacks;
     - `ERC1967Proxy` with `UUPSUpgradeable` implementation and canonical implementation storage slot verification (`0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc`).

4. **Shanghai Precompile Baseline (0x01–0x09):**
   - Addresses `0x01` through `0x09` are recognized as authoritative precompiles executed statelessly by REVM:
     - `0x01` `ecrecover` (Elliptic curve public key recovery, accepting valid signatures with $v \in \{27, 28\}$ and $s < n$)
     - `0x02` `SHA-256` (FIPS 180-2 256-bit hash)
     - `0x03` `RIPEMD-160` (160-bit hash left-padded to 32 bytes)
     - `0x04` `identity` (Verbatim input copy)
     - `0x05` `modexp` (Arbitrary precision modular exponentiation per EIP-198 / EIP-2565)
     - `0x06` `ecAdd` (alt_bn128 curve addition per EIP-196 / EIP-1108)
     - `0x07` `ecMul` (alt_bn128 curve scalar multiplication per EIP-196 / EIP-1108)
     - `0x08` `ecPairing` (alt_bn128 bilinear pairing check per EIP-197 / EIP-1108)
     - `0x09` `blake2f` (Blake2b compression function per EIP-152)
   - Every precompile is verified against **independent cryptographic oracles** (e.g. `k256`, `sha2`, `ripemd`, `num-bigint`, RFC 7693, and official Ethereum Foundation General State Test vectors). Circular testing (asserting REVM output against REVM) is strictly prohibited.

5. **Runtime Boundary & Isolation:**
   - The TOLA node runtime (`tola-node`, `tola-execution`, `tola-storage`, `tola-consensus`) depends strictly on REVM 43.0.3 and native Rust primitives.
   - Neither `solc`, npm, Node.js, Foundry, nor Hardhat may be added as production dependencies.
   - Development dependencies (`k256`, `sha2`, `ripemd`, `num-bigint`) are strictly isolated to `[dev-dependencies]` for integration testing.

6. **Gas & Fee Invariance:**
   - Precompile execution gas and contract execution gas reported by REVM must match `receipt.gas_used` exactly.
   - Fee deduction from caller balances must equal $\text{gas\_used} \times \text{effective\_gas\_price}$ with exact integer balance conservation.

7. **Persistence Invariance:**
   - Real Solidity contracts and OpenZeppelin token state commit into TOLA's unified `PersistentStateDB` (`Table::Accounts`, `Table::Storage`, `Table::Code`).
   - State survives cold node restart with bit-for-bit parity.

## Consequences
- Developers can deploy production Solidity contracts compiled with modern Solidity toolchains (Foundry, Hardhat, Remix, solc 0.8.24) targeting Shanghai.
- OpenZeppelin v5 custom errors and proxy patterns operate identically to Ethereum mainnet.
- Precompile support enables decentralized finance protocols (Uniswap Permit / ERC-2612) and zero-knowledge proof verifiers (Groth16 on alt_bn128).
- TOLA's consensus core remains minimal, secure, and independent of external language toolchains.
