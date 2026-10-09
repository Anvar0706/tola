# TOLA NETWORK — LEGACY ECOSYSTEM ARCHITECTURE INVENTORY

**Document ID:** ARCH-MIG-INV-001  
**Date:** October 7, 2026  
**Status:** FORMAL INVENTORY COMPLETE (PRE-MIGRATION)  
**Author:** Lead Protocol Architect, Blockchain Engineer & Security Engineer  

---

## 1. Executive Context & Objective

This document provides a comprehensive component-by-component inventory of the existing TOLA Network ecosystem implementation. It catalogs all modules, types, protocols, RPC endpoints, SDK methods, smart contracts, database entries, and documentation files affected by the legacy ecosystem interpretation.

### Core Architectural Principle
- **TOLA is an independent Layer-1 blockchain.**
- **PaxtaGram (and any other messenger) is an independent external platform.**
- **There is NO direct TOLA ↔ PaxtaGram integration.**
- TOLA never queries, depends upon, or acts as the authoritative registry for external messenger identities (usernames, phone numbers, unique account IDs, or external gifts).
- External assets can only exist on TOLA as **Tokenized External Assets** (TLS-721 NFTs representing verified external assets, minted via third-party tokenization applications with cryptographic attestations).
- **TOLA Native Assets** (such as TOLA Native Usernames and TOLA Native Gifts) remain sovereign, native protocol primitives with completely separate namespaces.

---

## 2. Component Classification Taxonomy

Every inspected component is classified under one of five strict migration categories:

- **KEEP:** Genuine TOLA-native primitive or protocol-agnostic mechanism. Retained as-is, with documentation clarified to emphasize sovereign TOLA-native status.
- **MIGRATE:** Existing mechanism that conflated native and external semantics. Migrated to decouple external platform references and adhere to the generic external asset tokenization model.
- **RENAME:** Component whose identifier implied a direct messenger coupling (e.g., `TelegramMiniAppBridge`). Renamed to a generic, platform-neutral identifier (`MiniAppBridge`) while providing a backward-compatible alias.
- **DEPRECATE:** Legacy endpoint, type, or method preserved temporarily for backward compatibility with explicit deprecation notices.
- **REMOVE:** Erroneous or tightly coupled logic that directly violated protocol independence (e.g., node querying external messenger APIs, direct messenger consensus logic, or conflating TOLA usernames with external messenger usernames).

---

## 3. Comprehensive Component Inventory

### 3.1 Rust Workspace Core Crates

| Item # | File / Path | Module / Symbol | Current Interpretation | Classification | Target Architecture Action |
|---|---|---|---|---|---|
| **RUST-01** | `crates/tola-types/src/ecosystem.rs` | `Username`, `UsernameError` | Validated handle `[a-z0-9_]{3,32}` with tiered pricing. | **KEEP** | Retained as **TOLA Native Username**. Explicitly documented as sovereign TOLA identity, independent of external messenger handles. |
| **RUST-02** | `crates/tola-types/src/ecosystem.rs` | `BadgeAttestationPayload` | Payload with `messenger_id: String` linking token ID to messenger profile display. | **MIGRATE / DEPRECATE** | Deprecate messenger-specific payload in favor of generic `ExternalAssetAttestation`. Maintain struct for backward compatibility. |
| **RUST-03** | `crates/tola-types/src/ecosystem.rs` | `ConnectMessageType` | 11 canonical TOLA Connect messages (`SessionInit`, `SignRequest`, etc.). | **KEEP** | Retain as protocol-agnostic E2EE session message types. |
| **RUST-04** | `crates/tola-types/src/ecosystem.rs` | `DeviceRecord` | Multi-device authorization and revocation tracking. | **KEEP** | Protocol-agnostic wallet session device management. Retain as-is. |
| **RUST-05** | `crates/tola-types/src/ecosystem.rs` | `ClearSigningPrompt` | Wallet UI modal fields (`domain`, `amount`, `memo`, `chainId`). | **KEEP** | Universal clear-signing verification. Retain as-is. |
| **RUST-06** | `crates/tola-types/src/ecosystem.rs` | *(New Primitives)* | None existed for generic external assets. | **MIGRATE (ADD)** | Introduce `ExternalAssetType`, `ExternalAssetSource`, `ExternalAsset`, and `ExternalAssetAttestation`. |
| **RUST-07** | `crates/tola-crypto/src/lib.rs` | `compute_username_commitment` | Keccak-256 commit hash for two-phase name registration. | **KEEP** | Sovereign TOLA Native Username commit-reveal mechanism. Retain. |
| **RUST-08** | `crates/tola-crypto/src/lib.rs` | `compute_badge_attestation_digest` | Digest binding `(chain_id, messenger_id, token_id, owner, expires_at)`. | **MIGRATE / DEPRECATE** | Retain for legacy profile badges, but introduce `compute_external_asset_attestation_digest` and `compute_external_asset_canonical_id`. |
| **RUST-09** | `crates/tola-crypto/src/lib.rs` | `verify_badge_attestation` | Recovers Secp256k1 signer against owner. | **KEEP** | Retain for legacy badge verification. Add `verify_external_asset_attestation`. |

---

### 3.2 RPC Engine (`crates/tola-rpc`)

| Item # | File / Path | Module / Endpoint | Current Interpretation | Classification | Target Architecture Action |
|---|---|---|---|---|---|
| **RPC-01** | `crates/tola-rpc/src/dispatcher.rs` | `tola_resolveUsername` | Resolves TOLA username to on-chain address. | **KEEP** | Retained as **TOLA Native Username** resolver. Returns null if expired. |
| **RPC-02** | `crates/tola-rpc/src/dispatcher.rs` | `tola_getUsernameRecord` | Alias for `tola_resolveUsername`. | **KEEP** | Retained for TOLA Native Username records. |
| **RPC-03** | `crates/tola-rpc/src/dispatcher.rs` | `tola_getGiftDetails` | Queries TLS-721 digital gift record by `tokenId`. | **KEEP** | Retained for TOLA Native Social Gifts (`TGIFT`). |
| **RPC-04** | `crates/tola-rpc/src/dispatcher.rs` | `tola_verifyBadgeAttestation` | Verifies messenger profile badge attestation. | **KEEP / DEPRECATE** | Retained with backward compatibility. Enforces on-chain ownership. |
| **RPC-05** | `crates/tola-rpc/src/dispatcher.rs` | *(New RPC Endpoints)* | No external asset query endpoints. | **MIGRATE (ADD)** | Add `tola_getExternalAsset`, `tola_getExternalAssetOwner`, `tola_getExternalAssetMetadata`, and `tola_verifyExternalAssetAttestation`. |
| **RPC-06** | `crates/tola-rpc/src/dispatcher.rs` | `username_registry` | In-memory store for native usernames. | **KEEP** | Retain for TOLA Native Usernames. |
| **RPC-07** | `crates/tola-rpc/src/dispatcher.rs` | `gift_registry` | In-memory store for native gifts. | **KEEP** | Retain for TOLA Native Gifts. |
| **RPC-08** | `crates/tola-rpc/src/dispatcher.rs` | `external_asset_registry` | None. | **MIGRATE (ADD)** | Add registry for tokenized external assets indexed by token ID and canonical hash. |

---

### 3.3 Smart Contracts (`contracts/`)

| Item # | File / Path | Contract / Interface | Current Interpretation | Classification | Target Architecture Action |
|---|---|---|---|---|---|
| **SOL-01** | `contracts/TolaUsernameRegistry.sol` | `TolaUsernameRegistry` | Two-phase commit-reveal registry with tiered pricing and grace period. | **KEEP** | Sovereign **TOLA Native Username Registry**. Update docstrings to state it governs TOLA-native handles only. |
| **SOL-02** | `contracts/TolaDigitalGift.sol` | `TolaDigitalGift` | Limited-edition social collectibles (`TGIFT`) with in-chat message and royalties. | **KEEP** | Sovereign **TOLA Native Digital Gift** contract. Retain as-is. |
| **SOL-03** | `contracts/TolaExternalAssetToken.sol` | *(New Contract)* | None existed for tokenizing external platform assets. | **MIGRATE (ADD)** | Implement canonical **TolaExternalAssetToken** (TLS-721) supporting authorized verifier attestations, duplicate mint prevention, and non-custodial ownership. |

---

### 3.4 Client SDKs (`sdk/typescript` & `sdk/python`)

| Item # | File / Path | Component / Function | Current Interpretation | Classification | Target Architecture Action |
|---|---|---|---|---|---|
| **TS-01** | `sdk/typescript/src/username/registry.js` | `normalizeUsername`, `validateUsername`, `calculateUsernameFee`, `computeUsernameCommitment` | Client validation for `@username`. | **KEEP** | Sovereign TOLA Native Username helpers. Retain. |
| **TS-02** | `sdk/typescript/src/connect/bridge.js` | `TelegramMiniAppBridge` | Name implies specific Telegram coupling, though it implements generic clear-signing. | **RENAME / MIGRATE** | Rename to `MiniAppBridge` / `TolaAppBridge`. Provide `TelegramMiniAppBridge` as backward-compatible subclass alias. |
| **TS-03** | `sdk/typescript/src/gift/badge.js` | `computeBadgeAttestationDigest`, `signBadgeAttestation`, `verifyBadgeAttestation` | Profile badge binding for messenger display. | **KEEP / DEPRECATE** | Retain for legacy profile badges. |
| **TS-04** | `sdk/typescript/src/external/asset.js` | *(New Module)* | None. | **MIGRATE (ADD)** | Add generic external asset tokenization helpers (`computeExternalAssetAttestationDigest`, `signExternalAssetAttestation`, `verifyExternalAssetAttestation`, `computeExternalAssetCanonicalId`). |
| **PY-01** | `sdk/python/tola/username.py` | `Username`, `calculate_username_fee`, `compute_username_commitment` | Python native username helpers. | **KEEP** | Sovereign TOLA Native Username helpers. Retain. |
| **PY-02** | `sdk/python/tola/connect.py` | `TolaConnectSession`, `DeviceRegistry`, `ClearSigningPrompt` | Protocol-agnostic E2EE and clear-signing. | **KEEP** | Retain as-is. |
| **PY-03** | `sdk/python/tola/gift.py` | `compute_badge_attestation_digest`, `sign_badge_attestation`, `verify_badge_attestation` | Profile badge binding. | **KEEP / DEPRECATE** | Retain for legacy badge display. |
| **PY-04** | `sdk/python/tola/external.py` | *(New Module)* | None. | **MIGRATE (ADD)** | Add generic external asset tokenization helpers in Python with 100% parity to TypeScript and Rust. |

---

### 3.5 Operational CLI (`crates/tola-cli`)

| Item # | File / Path | Command / Function | Current Interpretation | Classification | Target Architecture Action |
|---|---|---|---|---|---|
| **CLI-01** | `crates/tola-cli/src/main.rs` | `username` (`validate`, `price`, `commit`) | CLI commands for native usernames. | **KEEP** | Retained for TOLA Native Usernames. |
| **CLI-02** | `crates/tola-cli/src/main.rs` | `gift` (`sign-badge`, `verify-badge`) | CLI commands for legacy profile badge attestations. | **KEEP** | Retained for legacy badges. |
| **CLI-03** | `crates/tola-cli/src/main.rs` | `connect` (`verify-prompt`) | Validates clear-signing prompt JSON. | **KEEP** | Retained for wallet clear-signing. |
| **CLI-04** | `crates/tola-cli/src/main.rs` | `external-asset` *(New Subcommand)* | None. | **MIGRATE (ADD)** | Add CLI commands for verifying external asset attestations and inspecting tokenized external asset states. |

---

### 3.6 Documentation Suite (`docs/`)

| Item # | File / Path | Current Content | Classification | Target Architecture Action |
|---|---|---|---|---|---|
| **DOC-01** | `docs/messenger-integration.md` | Discusses messenger integration; contained vague language on messenger identity binding. | **MIGRATE** | Update to clearly explain: TOLA and messengers are 100% independent; zero direct consensus or API integration; third-party tokenization boundary. |
| **DOC-02** | `docs/messenger/messenger-architecture.md` | Discusses decoupled messenger client. | **MIGRATE** | Emphasize ADR-0005 sovereign decoupling; TOLA blockchain has zero dependency on messenger servers. |
| **DOC-03** | `docs/usernames/username-system.md` | Described `@username` registry. | **MIGRATE** | Clarify this is **TOLA Native Username Registry**, completely distinct from external platform handles. |
| **DOC-04** | `docs/gifts/digital-gifts.md` | Described digital gifts and profile badge showcase. | **MIGRATE** | Clarify distinction between TOLA Native Gifts and Tokenized External Messenger Gifts. |
| **DOC-05** | `docs/ecosystem/tls-721.md` | TLS-721 specification. | **MIGRATE** | Add section specifying Tokenized External Asset standard under TLS-721. |
| **DOC-06** | `docs/ecosystem/native-vs-external-assets.md` | *(New Document)* | **MIGRATE (ADD)** | Formal document detailing the 3 separate concepts: TOLA Native Asset, External Asset, Tokenized External Asset. |
| **DOC-07** | `docs/ecosystem/external-asset-tokenization.md` | *(New Document)* | **MIGRATE (ADD)** | Formal architectural specification of the external asset tokenization pipeline. |
| **DOC-08** | `docs/ecosystem/external-asset-security.md` | *(New Document)* | **MIGRATE (ADD)** | Security and threat model for external asset tokenization (anti-replay, anti-forgery, privacy, duplicate mint defense). |

---

## 4. Summary of Planned Actions

- **Kept (Sovereign TOLA Primitives):** 11 components (including TOLA Native Username registry, TOLA Native Gifts, TOLA Connect E2EE, DeviceRegistry, and ClearSigningPrompt).
- **Migrated / Renamed:** 4 components (including `TelegramMiniAppBridge` -> `MiniAppBridge` with backward-compatible alias).
- **Added (Generic External Asset Model):** 8 components across Rust core, RPC, smart contracts, TypeScript SDK, Python SDK, and CLI.
- **Documentation Overhauled:** 5 existing documents revised + 5 new authoritative architectural documents created.
- **Phase 19 Isolation:** 100% frozen. Zero touch to Cross-Shard State Proofs or Inter-Rollup State Bridge.
