# TOLA Network: Final Architecture Cleanup Plan
## Complete Removal of Native Identity / Native Gift Semantics & Alignment with External Asset Model

**Document ID:** TOLA-FCP-2026-01  
**Date:** October 7, 2026  
**Status:** APPROVED FOR EXECUTION  

---

## 1. Executive Directive

Following protocol architecture review, TOLA Network does NOT operate a native messenger identity layer, native username registry, native gifts number registry, or native gift product. 

**The Authoritative Architecture:**
- **TOLA is a blockchain**, providing an on-chain ownership and tokenization layer for unique digital assets that originate on external platforms.
- **External Platforms** (e.g. PaxtaGram, Telegram, X, Discord) are the sole creators and sources of truth for their own usernames, numbers, gifts, and accounts.
- **Independent Third-Party Tokenization Applications** (e.g. Fragment-like services) verify external ownership off-chain and facilitate minting TOLA NFTs.
- **TOLA NFTs** represent external assets on-chain. Blockchain ownership of the representation does not automatically mutate external platform databases.

---

## 2. Inventory & Classification of Components to Clean Up

### A. Active Runtime Components to Remove / Deprecate
1. **`crates/tola-types/src/ecosystem.rs`**:
   - `Username`, `UsernameRecord`, `UsernameError`: **REMOVE** (implements native username registry).
   - `BadgeAttestationPayload`: **REMOVE** (implements native badge attestation).
   - Expand `ExternalAssetType` to include:
     - `MESSENGER_USERNAME`
     - `MESSENGER_NUMBER`
     - `MESSENGER_GIFT`
     - `SOCIAL_USERNAME`
     - `SOCIAL_NUMBER`
     - `DIGITAL_COLLECTIBLE`
     - `OTHER_EXTERNAL_ASSET`
2. **`crates/tola-crypto/src/lib.rs`**:
   - `compute_badge_attestation_digest`, `verify_badge_attestation`: **REMOVE / DEPRECATE** (native badge attestation logic).
   - Ensure generic `compute_external_asset_canonical_id`, `compute_external_asset_attestation_digest`, and `verify_external_asset_attestation` serve as the sole attestation primitives.
3. **`crates/tola-rpc/src/dispatcher.rs`**:
   - `username_registry`, `username_by_owner`, `gift_registry`: **REMOVE**.
   - `tola_resolveUsername`, `tola_getUsernameRecord`: **REMOVE**.
   - `tola_getGiftDetails`, `tola_verifyBadgeAttestation`: **REMOVE**.
   - Preserve exclusively the 4 external asset RPC methods:
     - `tola_getExternalAsset`
     - `tola_getExternalAssetOwner`
     - `tola_getExternalAssetMetadata`
     - `tola_verifyExternalAssetAttestation`
4. **`crates/tola-cli/src/main.rs`**:
   - `username` subcommand (`cmd_username`): **REMOVE**.
   - `gift` subcommand (`cmd_gift`): **REMOVE**.
   - Preserve `external-asset` (`verify-attestation`, `canonical-id`, `show`).
5. **`sdk/typescript`**:
   - `src/username/` directory and exports: **REMOVE**.
   - `src/gift/` directory and exports: **REMOVE**.
   - Retain `src/external/` as the primary asset module.
6. **`sdk/python`**:
   - `tola/username.py` and exports: **REMOVE**.
   - `tola/gift.py` and exports: **REMOVE**.
   - Retain `tola/external.py` as the primary asset module.
7. **`contracts/`**:
   - `TolaUsernameRegistry.sol`: **DEPRECATE / MARK INACTIVE**.
   - `TolaDigitalGift.sol`: **DEPRECATE / MARK INACTIVE**.
   - `TolaExternalAssetToken.sol`: **ACTIVE CANONICAL TLS-721 TOKENIZATION CONTRACT**.

---

## 3. Reusable NFT & Blockchain Infrastructure to Retain

The underlying cryptographic and tokenization infrastructure is retained and strengthened:
- **TLS-721 NFT Interface & Machinery**: Standard non-fungible token mechanics (`ownerOf`, `transfer`, `approve`, `transferFrom`, `balanceOf`).
- **Canonical ID Derivation**: Deterministic 32-byte identity hash preventing duplicate minting of identical external assets.
- **Attestation Engine**: Cryptographic pre-image binding `chainId`, `nonce`, `destinationWallet`, `expiresAt`, `sourcePlatform`, `assetType`, and `externalAssetId`.
- **TOLA Connect Protocol**: Generic E2EE wallet-app connection and clear-signing protocol.

---

## 4. Execution Steps

1. **Step 1: Core Types & Cryptography Cleanup (`tola-types` & `tola-crypto`)**:
   - Remove native username and badge types.
   - Update `ExternalAssetType` enum with all specified types (`SocialNumber`, `DigitalCollectible`).
2. **Step 2: RPC Engine Cleanup (`tola-rpc`)**:
   - Remove `username_registry`, `gift_registry`, and corresponding native RPC handlers.
   - Ensure external asset RPC endpoints operate with full fidelity.
3. **Step 3: CLI Cleanup (`tola-cli`)**:
   - Remove `username` and `gift` subcommands.
   - Update help documentation.
4. **Step 4: Smart Contract Annotation (`contracts/`)**:
   - Mark `TolaUsernameRegistry.sol` and `TolaDigitalGift.sol` as deprecated legacy archives.
   - Designate `TolaExternalAssetToken.sol` as the active canonical contract.
5. **Step 5: TypeScript & Python SDKs Cleanup**:
   - Remove `username` and `gift` modules.
   - Update package index exports.
   - Align test suites to test external asset lifecycle and test for absence of native identity.
6. **Step 6: Integration & Architecture Tests (`crates/tola-node/tests/`)**:
   - Update `testnet_stress_test.rs` to remove native username/gift calls.
   - Update `external_asset_architecture_test.rs`:
     - Explicit test that native username/gift does NOT exist.
     - Full simulation of PaxtaGram `@anvar`, Gifts Number `#12345`, and Gift `#987` via third-party tokenization.
     - Decoupling proof: TOLA NFT transfer updates only blockchain ownership; mock platform DB is untouched.
7. **Step 7: Documentation Cleanup**:
   - Update all docs to remove active native identity/gift claims and enforce the external asset model.
8. **Step 8: Verification & Certification**:
   - Run full workspace tests (`cargo test --workspace`).
   - Run TypeScript (`node test`) and Python (`unittest`).
   - Run Clippy and Fmt.
   - Generate `docs/ecosystem/final-architecture-cleanup-report.md`.
