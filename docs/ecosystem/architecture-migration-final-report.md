# TOLA Network: Protocol Architecture Migration Final Report
## Legacy Ecosystem Model Removal & External Asset Tokenization Architecture

**Document ID:** TOLA-AMR-2026-01  
**Classification:** Canonical Protocol Architecture Report  
**Date:** October 7, 2026  
**Status:** APPROVED & CERTIFIED  
**Final Verdict:** **ARCHITECTURE MIGRATION COMPLETE**

---

## 1. Executive Summary

This report documents the completion of the protocol-level architectural correction for TOLA Network: the complete removal of the legacy direct messenger coupling model and the implementation of the sovereign **External Asset Tokenization Architecture**.

### The Core Problem Remediated:
The legacy ecosystem architecture contained an incorrect interpretation wherein the TOLA blockchain was assumed to act as the authoritative source of truth for external messenger assets (such as PaxtaGram handles, phone numbers, and in-app gifts). Such coupling introduced unacceptable centralization risks, availability dependencies, and state synchronization failures.

### The Authoritative Architecture Established:
1. **Absolute Platform Independence:** TOLA and PaxtaGram (and all other external messengers/social networks) are 100% independent platforms. The TOLA blockchain node never connects to, depends upon, or queries external messenger APIs, backends, databases, or authentication services.
2. **Three Sovereign Asset Classes:**
   - **Class 1 (TOLA Native Assets):** Created on TOLA (e.g. TOLA Native Usernames `[a-z0-9_]{3,32}`, Native `TGIFT` collectibles, Profile Badges). TOLA is the absolute sovereign source of truth.
   - **Class 2 (External Platform Assets):** Created and owned on external platforms (e.g. PaxtaGram `@anvar`, `+8880001`, external gifts). The external platform is the sole source of truth.
   - **Class 3 (TLS-721 Tokenized External Assets):** TOLA TLS-721 NFTs representing external assets, minted via independent third-party tokenization services (e.g. Fragment model) using authorized cryptographic attestations.
3. **Decoupled Ownership Model:** Blockchain ownership represents ownership of the representation on TOLA. Transferring a TOLA NFT updates on-chain records, but never mechanically mutates external platform databases.

---

## 2. Inventory of Legacy Architecture

The comprehensive pre-implementation inventory was documented in [`docs/ecosystem/legacy-architecture-inventory.md`](../../docs/ecosystem/legacy-architecture-inventory.md):
- **Core Types (`tola-types`):** Direct coupling of messenger profile identifiers and badges without generic asset abstraction.
- **Cryptographic Primitives (`tola-crypto`):** Static badge attestation bindings without chain-id binding or generic asset type domain separation.
- **Smart Contracts (`contracts/`):** Absence of a dedicated TLS-721 tokenization contract with canonical ID duplicate mint protection.
- **JSON-RPC Engine (`tola-rpc`):** Absence of external asset lookup and attestation verification endpoints.
- **Client SDKs (`@tola/sdk` & `tola`):** Telegram-specific mini-app bridge abstractions lacking generic mini-app interoperability.
- **Documentation:** Ambiguity regarding whether TOLA nodes synchronize state with external messenger backends.

---

## 3. What Was Removed

1. **Direct Blockchain ↔ Messenger State Dependency:** Any architectural assumption that TOLA consensus, state roots, or validators communicate with or depend upon external messenger backends.
2. **Node-Level External Network Calls:** Ensured that no TOLA node runtime component makes outbound HTTP/RPC calls to any external messenger API.
3. **Implicit Dual-Custody Assumptions:** Removed any expectation that purchasing a blockchain NFT automatically forces a database record modification in a proprietary off-chain application.

---

## 4. What Was Deprecated

1. **Single-Platform Mini App Bridge:** Generalized `TelegramMiniAppBridge` into `MiniAppBridge`, preserving `TelegramMiniAppBridge` as a backward-compatible wrapper.
2. **Specific Profile Badge Stubs:** Generalized specific profile badge fields into the extensible `ExternalAssetType` standard.

---

## 5. What Was Preserved

1. **TOLA Native Username Registry (`[a-z0-9_]{3,32}`):** Preserved the complete on-chain tiered anti-squatting fee schedule (10,000 / 2,500 / 500 / 50 TOLA) and two-phase commit-reveal MEV protection in [`contracts/TolaUsernameRegistry.sol`](../../contracts/TolaUsernameRegistry.sol).
2. **TOLA Native Collectibles (`TGIFT`):** Preserved on-chain limited-edition digital collectibles with ERC-2981 royalties in [`contracts/TolaDigitalGift.sol`](../../contracts/TolaDigitalGift.sol).
3. **TOLA Wallet Derivation Standard v1.0:** Canonical BIP-44 path `m/44'/9999'/0'/0/0` (Model A) remains 100% FROZEN and intact.
4. **Phase 19 (Cross-Shard State Proofs & Inter-Rollup State Bridge / ADR-0014):** 100% FROZEN and untouched.

---

## 6. What Was Added

1. **`ExternalAssetType` Enum:** Standardized 6 canonical asset types (`MessengerUsername`, `MessengerNumber`, `MessengerGift`, `MessengerBadge`, `SocialUsername`, `OtherExternalAsset`).
2. **`ExternalAsset` Struct:** Canonical representation tracking `token_id`, `canonical_id`, `source_platform`, `asset_type`, `external_asset_id`, `external_reference`, `owner`, `verifier_address`, `minted_at`, and `metadata_uri`.
3. **`ExternalAssetAttestation` Struct:** Secure attestation format binding `chainId`, `nonce`, `destinationWallet`, `expiresAt`, `sourcePlatform`, `assetType`, and `externalAssetId`.
4. **Cryptographic Primitives:**
   - `compute_external_asset_canonical_id`
   - `compute_external_asset_attestation_digest`
   - `verify_external_asset_attestation`
5. **Smart Contract:** [`contracts/TolaExternalAssetToken.sol`](../../contracts/TolaExternalAssetToken.sol).
6. **JSON-RPC Endpoints:** `tola_getExternalAsset`, `tola_getExternalAssetOwner`, `tola_getExternalAssetMetadata`, and `tola_verifyExternalAssetAttestation`.
7. **SDK Modules:** TypeScript `@tola/sdk` external module and Python `tola.external`.
8. **CLI Subcommands:** `tola-cli external-asset verify-attestation`, `show`, `canonical-id`.
9. **Integration Tests:** [`crates/tola-node/tests/external_asset_architecture_test.rs`](../../crates/tola-node/tests/external_asset_architecture_test.rs).

---

## 7. Complete External Asset Tokenization Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                   EXTERNAL ASSET TOKENIZATION ARCHITECTURE                  │
└─────────────────────────────────────────────────────────────────────────────┘

  1. External Platform                2. Third-Party Tokenization Service
  ┌───────────────────────┐           ┌──────────────────────────────────────┐
  │   External Platform   │ ◄───────► │ Independent Tokenization Service     │
  │      (PaxtaGram)      │  OAuth    │ (Fragment-like Entity)               │
  │ • User owns handle    │  Verify   │ • Validates user off-chain           │
  │ • DB: @anvar          │           │ • Signs ExternalAssetAttestation     │
  └───────────────────────┘           └──────────────────┬───────────────────┘
                                                         │
                                                         │ 3. Issues Attestation
                                                         ▼
  4. TOLA Node & Blockchain           3. TOLA Non-Custodial Wallet
  ┌───────────────────────┐           ┌──────────────────────────────────────┐
  │    TOLA Blockchain    │ ◄──────── │            Alice's Wallet            │
  │ • Verifies Attestation│ Broadcast │ • Holds private key                  │
  │ • Prevents Duplicates │ Mint TX   │ • Receives TLS-721 token             │
  │ • TLS-721 Contract    │           │ • Can transfer/sell on TOLA DEX      │
  └───────────────────────┘           └──────────────────────────────────────┘
```

---

## 8. Full Classification of the Three Asset Classes

| Feature | Class 1: TOLA Native Asset | Class 2: External Asset | Class 3: Tokenized External Asset |
| :--- | :--- | :--- | :--- |
| **Origin** | Created on TOLA | Created on External App | Minted on TOLA via Attestation |
| **Source of Truth** | TOLA Blockchain | External Platform DB | TOLA (NFT) / External App (DB) |
| **Examples** | TOLA Native `@alice`, `TGIFT` | PaxtaGram `@anvar`, `+8880001` | TLS-721 NFT for PaxtaGram `@anvar` |
| **Governing Rules** | Protocol consensus & code | Terms of Service of external app | Smart Contract rules on TOLA |
| **Decoupling** | N/A (Fully Sovereign) | N/A (Off-chain) | Fully Decoupled (NFT != DB record) |

---

## 9. Cryptographic Proof Architecture

The attestation binds all relevant state to prevent any parameter substitution:

$$\text{Digest} = \text{Keccak-256}\Big(\text{"TOLA\_EXTERNAL\_ASSET\_ATTESTATION\_V1:"} \parallel \text{chain\_id}_{8} \parallel \text{nonce}_{8} \parallel \text{dest}_{20} \parallel \text{expiry}_{8} \parallel \text{plat\_len}_{4} \parallel \text{platform} \parallel \text{type}_{1} \parallel \text{id\_len}_{4} \parallel \text{id}\Big)$$

$$\text{Canonical ID} = \text{Keccak-256}\Big(\text{"TOLA\_EXTERNAL\_ASSET\_CANONICAL\_V1:"} \parallel \text{plat\_len}_{4} \parallel \text{platform} \parallel \text{type}_{1} \parallel \text{id\_len}_{4} \parallel \text{id}\Big)$$

Every signature requires canonical Secp256k1 recovery with strict Low-S validation ($s \le n/2$).

---

## 10. Verifier Model & Security

1. **Whitelisted Verifiers:** Only addresses registered in `isAuthorizedVerifier[verifier]` on the TLS-721 contract (or approved in node registry) may sign valid mint attestations.
2. **Key Rotation & Revocation:** Verifier keys can be revoked or rotated by the tokenization authority without affecting existing minted tokens.
3. **Signature Verification:** The recovered address from the 65-byte signature must exactly match the authorized verifier.

---

## 11. Replay Defense Model

The architecture incorporates four distinct layers of replay defense:
1. **Cross-Chain Isolation:** `chainId` (e.g. 9999 vs 102) is cryptographically pre-hashed. A signature generated for Testnet cannot be replayed on Mainnet.
2. **Cross-Wallet Hijacking Defense:** `destinationWallet` is cryptographically pre-hashed. An attacker observing an attestation cannot claim it to their own wallet.
3. **Stale/Expired Defense:** `expiresAt` timestamp is enforced against current time (`now <= expiresAt`).
4. **Duplicate Mint Prevention:** `canonicalIdToToken` mapping ensures that the identical external asset cannot be minted twice.

---

## 12. Third-Party Simulation Results

The end-to-end multi-party simulation was executed in [`crates/tola-node/tests/external_asset_architecture_test.rs`](../../crates/tola-node/tests/external_asset_architecture_test.rs) (`test_third_party_external_asset_tokenization_simulation`):
1. Alice authenticated on Mock External Platform (PaxtaGram).
2. Independent Tokenization Service issued an authorized attestation to Alice's TOLA wallet.
3. Node verified attestation via `tola_verifyExternalAssetAttestation` (`valid: true`).
4. Token was registered on TOLA with canonical ID `0xe51c65...`.
5. Alice transferred the TOLA NFT to Bob on TOLA.
6. **Result:** On-chain ownership updated to Bob. External platform database record remained Alice!
7. **Conclusion:** Ownership decoupling proven successfully.

---

## 13. Independence Test Results

The platform independence test was executed in `test_platform_independence_and_sovereignty`:
- Started node dispatcher with pure sovereign TOLA state.
- Executed native balance queries, native username registration (`@anvar_sovereign`), native gift badge attestations, and native transfers.
- **Result:** 100% of operations completed sovereignly without connecting to or querying any external platform or service.

---

## 14. Smart Contract Verification Results

The contract [`contracts/TolaExternalAssetToken.sol`](../../contracts/TolaExternalAssetToken.sol) was verified for:
- Correct canonical ID computation matching Rust, TypeScript, and Python.
- Duplicate mint rejection (`AssetAlreadyTokenized`).
- Attestation replay rejection (`AttestationAlreadyUsed`).
- Signature recovery with Low-S enforcement.
- Whitelisted verifier enforcement (`UnauthorizedVerifier`).

---

## 15. JSON-RPC Verification Results

The four dedicated JSON-RPC endpoints were tested in `crates/tola-rpc/src/dispatcher.rs` and `crates/tola-node/tests/external_asset_architecture_test.rs`:
- `tola_getExternalAsset`: Returns full record by token ID or canonical ID.
- `tola_getExternalAssetOwner`: Returns 20-byte owner address.
- `tola_getExternalAssetMetadata`: Returns metadata URI and asset details.
- `tola_verifyExternalAssetAttestation`: Validates attestation cryptographically and checks for existing mints.

---

## 16. SDK Verification Results

- **TypeScript SDK (`@tola/sdk`):**
  - Canonical ID bit-for-bit parity with Rust.
  - Attestation signing and verification.
  - Multi-layer SSRF metadata validation (`validateExternalAssetMetadata`).
  - Generalized `MiniAppBridge` with backward-compatible `TelegramMiniAppBridge`.
  - **Status:** 52/52 tests passing.
- **Python SDK (`tola`):**
  - Canonical ID bit-for-bit parity with Rust and TypeScript.
  - Attestation signing via `TolaWallet.sign_digest` and verification.
  - Metadata URL SSRF filtering.
  - **Status:** 49/49 tests passing.
- **Rust SDK (`tola-sdk`):**
  - Native integration with `tola-types` and `tola-crypto`.
  - **Status:** 10/10 tests passing.

---

## 17. CLI Verification Results

The `tola-cli external-asset` command was tested:
```bash
tola-cli external-asset canonical-id --platform paxtagram --type messenger_username --id anvar
# Output: Canonical ID: 0xe51c65e1f87b606cb656a4a931e3f4319af4880be892bea1ac5335026265f9f0
```
- `external-asset verify-attestation`: Verifies structured attestation JSON against authorized verifier.
- `external-asset show`: Queries on-chain record via RPC by token ID or canonical ID.
- `external-asset canonical-id`: Computes deterministic canonical ID.

---

## 18. Backward Compatibility Assessment

- **Zero Breaking Changes to Working Subsystems:**
  - Existing profile badge attestations (`compute_badge_attestation_digest`, `verify_badge_attestation`) preserved alongside the new external asset methods.
  - `TelegramMiniAppBridge` preserved as an alias extending `MiniAppBridge`.
  - `TolaUsernameRegistry.sol` and `TolaDigitalGift.sol` fully preserved without breaking edits.
  - Wallet Derivation Standard v1.0 preserved.

---

## 19. Security Threat Model & Audit Findings

The threat model documented in [`docs/ecosystem/external-asset-security.md`](../../docs/ecosystem/external-asset-security.md) audited and confirmed mitigations for:
1. Cross-chain replay attacks (EIP-155 style `chainId` binding).
2. Cross-wallet hijacking (`destinationWallet` binding).
3. Nonce exhaustion and attestation staling (`expiresAt` enforcement).
4. Signature malleability (Strict Low-S canonical check).
5. Duplicate minting (Canonical ID mapping).
6. SSRF and metadata URI injection (Private IP & metadata filtering).
7. PII privacy protection (Zero raw personal data on-chain).
8. Legal / Ownership decoupling (Clear-signing disclaimers).

---

## 20. Complete Test Results

| Test Suite | Tests Passed | Tests Failed | Skipped | Status |
| :--- | :---: | :---: | :---: | :---: |
| **Rust Workspace (`cargo test --workspace`)** | 194 | 0 | 0 | **PASS** |
| **TypeScript SDK (`node test/sdk.test.mjs`)** | 52 | 0 | 0 | **PASS** |
| **Python SDK (`unittest discover`)** | 49 | 0 | 0 | **PASS** |
| **Total Automated Tests** | **295** | **0** | **0** | **PASS (100%)** |
| **Cargo Clippy (`-D warnings`)** | 0 warnings | 0 errors | — | **PASS** |
| **Cargo Format (`cargo fmt --check`)** | 0 diffs | 0 errors | — | **PASS** |

---

## 21. Confirmation of Phase 19 Isolation

Phase 19 (Cross-Shard State Proofs, Dynamic Sharding & Inter-Rollup State Bridge / ADR-0014) is **100% FROZEN AND UNTOUCHED**:
- Zero modifications to `crates/tola-consensus/src/sharding.rs`.
- Zero modifications to `crates/tola-types/src/sharding.rs`.
- Zero modifications to `decisions/ADR-0014-cross-shard-state-proofs-and-bridge.md`.
- All cross-shard tests pass with 100% parity across Rust, TypeScript, and Python.

---

## 22. Final Architectural Verdict

All requirements of the protocol architectural correction have been fully satisfied, verified, tested, and documented.

```
===============================================================================
                     ARCHITECTURE MIGRATION COMPLETE
===============================================================================
```
