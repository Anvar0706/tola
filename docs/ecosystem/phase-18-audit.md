# TOLA NETWORK — PHASE 18 ECOSYSTEM LAYER AUDIT REPORT

**Date:** October 7, 2026  
**Status:** AUDIT COMPLETED & VERIFIED  
**Scope:** Phase 18 (Ecosystem Layer: TOLA Connect, Decentralized @username Registry, Digital Collectibles / Gifts) & Strict Isolation of Phase 19 (Cross-Shard State Proofs)

---

## 1. Executive Summary

This formal audit evaluates the existing codebase of TOLA Network in preparation for the complete, production-ready implementation of **Phase 18 — Ecosystem Layer**.

Phase 18 bridges the native blockchain core (cryptographic wallet derivation, Secp256k1 recoverable signatures, consensus, JSON-RPC, and memo field) to consumer-facing applications, Telegram Mini Apps, web applications, and social messaging ecosystems without compromising non-custodial security invariants.

### Strict Scope Rule: Phase 19 Isolation
Phase 19 (**Cross-Shard State Proofs & Inter-Rollup Bridge Architecture**, governed by `ADR-0014`) was completed and verified in preceding milestones.
- **Phase 19 status:** **FROZEN**.
- **Isolation Rule:** No modifications, refactors, duplications, or re-implementations shall be made to `crates/tola-types/src/sharding.rs`, `crates/tola-consensus/src/sharding.rs`, `sdk/typescript/src/sharding.js`, `sdk/python/tola/sharding.py`, or Phase 19 test suites. Phase 18 interacts with existing layers solely via public interfaces.

---

## 2. Phase 18 Subsystem Status & Gap Analysis

### 2.1 Subsystem 18.1: TOLA Connect Protocol
* **Current State:**
  - Basic `TolaConnectSession` implemented in TypeScript (`sdk/typescript/src/connect/session.js`) utilizing WebCrypto AES-GCM-256 for symmetric payload encryption.
  - Basic `TelegramMiniAppBridge` in TypeScript (`bridge.js`) providing request routing and account access.
  - `ClearSigningPrompt` struct in Rust (`crates/tola-types/src/ecosystem.rs`) and Python (`sdk/python/tola/connect.py`).
* **Gaps Identified:**
  1. *Protocol Message Standard:* Missing formal message envelope specification and state machine covering all 11 lifecycle messages: `SESSION_INIT`, `SESSION_ACCEPT`, `DEVICE_REGISTER`, `DEVICE_REVOKE`, `SIGN_REQUEST`, `SIGN_APPROVAL`, `SIGN_REJECTION`, `TRANSACTION_REQUEST`, `TRANSACTION_APPROVAL`, `TRANSACTION_SUBMISSION`, `TRANSACTION_RESULT`.
  2. *Origin & Domain Separation:* Requests must be cryptographically bound to `origin`, `domain`, `app_id`, and `chain_id` to prevent cross-app replay attacks.
  3. *Device Management:* Multi-device support (register, list, revoke device session keys) not yet formalized or enforced.
  4. *Anti-Substitution Security:* Hardening against malicious Mini Apps attempting to display one value (e.g. 1 TOLA) in webview while dispatching an altered signing request (e.g. 100 TOLA) or tampering with recipient/memo.
  5. *Session Expiry & Nonce Monotonicity:* Explicit validation that expired sessions and replayed nonces are rejected.

### 2.2 Subsystem 18.2: Decentralized @username Registry
* **Current State:**
  - Production reference contract `contracts/TolaUsernameRegistry.sol` implementing two-phase commit-reveal, anti-squatting pricing tiers, and 90-day grace periods.
  - Rust primitive `Username` in `crates/tola-types/src/ecosystem.rs` enforcing `[a-z0-9_]{3,32}` and annual fee calculator.
  - Rust commitment hasher `compute_username_commitment` in `crates/tola-crypto/src/lib.rs`.
  - TypeScript client `normalizeUsername`, `validateUsername`, `calculateUsernameFee`, and `computeUsernameCommitment` in `sdk/typescript/src/username/registry.js`.
  - Python parity in `sdk/python/tola/username.py`.
* **Gaps Identified:**
  1. *Homoglyph Protection:* Explicit documentation and test vectors guaranteeing rejection of Cyrillic/Greek lookalikes, zero-width characters, uppercase letters, and confusable Unicode code points.
  2. *RPC Endpoints:* JSON-RPC dispatcher (`crates/tola-rpc/src/dispatcher.rs`) lacks native `tola_resolveUsername` and `tola_getUsernameRecord` query endpoints.
  3. *Client Resolution Integration:* TypeScript `UsernameResolver` and Python client need direct wiring to node RPC queries.

### 2.3 Subsystem 18.3: Digital Collectibles / Gifts (TLS-721)
* **Current State:**
  - Production reference contract `contracts/TolaDigitalGift.sol` supporting limited editions, serial numbering (`#X of Y`), in-chat custom messages, and ERC-2981 5% royalties.
  - Rust badge binding attestation primitives (`BadgeAttestationPayload`, `compute_badge_attestation_digest`, `verify_badge_attestation`).
  - TypeScript implementation in `sdk/typescript/src/gift/badge.js` with pure JS Secp256k1 recovery.
  - Python implementation in `sdk/python/tola/gift.py`.
* **Gaps Identified:**
  1. *TLS-721 Standard Specification:* Need formal architectural specification document (`docs/ecosystem/tls-721.md`) detailing the exact token interface, storage minimization (zero heavy media on-chain), and IPFS/Arweave off-chain URI anchoring.
  2. *Profile Badge Lifecycle:* Attestation expiration handling and messenger ID binding specification (`docs/ecosystem/digital-gifts.md`).
  3. *RPC Endpoints:* JSON-RPC queries for gift details and badge verification (`tola_getGiftDetails`, `tola_verifyBadgeAttestation`).

---

## 3. Cryptographic & Protocol Compatibility Invariants

The Ecosystem Layer directly builds upon the frozen TOLA protocol foundations:

| Invariant | Standard / Value | Status |
|---|---|---|
| **Account Keypair** | Secp256k1 ECDSA with canonical Low-S | VERIFIED |
| **Consensus Keypair** | Ed25519 (RFC 8032) | Strictly isolated from user accounts |
| **Address Derivation** | `Keccak-256(pubkey[1..65])[12..32] -> Bech32m` | Bit-for-bit verified across Rust, TS, Python |
| **Derivation Path** | `m/44'/9999'/0'/0/0` (Model A: Unified Identity) | FROZEN Standard v1.0 |
| **Mainnet Chain ID** | `9999` (`tola1...`) | VERIFIED |
| **Testnet Chain ID** | `102` (`ttola1...`) | VERIFIED |
| **Devnet Chain ID** | `1337` (`dtola1...`) | VERIFIED |
| **Transaction Signing** | `BLAKE3(DOMAIN || ChainID || Epoch || PayloadHash)` | VERIFIED |
| **Transaction Memo** | `Option<String>` max 256 bytes UTF-8 | Integrated into Clear-Signing |

---

## 4. Security Risks & Mitigations

1. **Malicious Mini App Phishing & Fake Prompts:**
   - *Risk:* Compromised Telegram Mini App presents a fake webview prompt to steal credentials or fool the user.
   - *Mitigation:* Non-custodial separation. The wallet runs in an isolated native process or extension; credentials never enter the Mini App DOM. Clear-signing prompts are rendered natively by the wallet.
2. **Transaction Payload Substitution:**
   - *Risk:* Mini App displays "Transfer 5 TOLA" in webview but transmits a payload for "500 TOLA" or changes the destination address.
   - *Mitigation:* The wallet inspects the actual unsigned transaction binary/fields, computes its digest independently, and renders the native Clear-Signing confirmation showing destination, amount, and memo directly from the parsed transaction.
3. **Cross-Origin Replay Attacks:**
   - *Risk:* A malicious app captures a signed session or request intended for `defi.tola.network` and replays it under `scam.tola.network`.
   - *Mitigation:* Protocol envelopes include explicit `domain`, `origin`, and unique monotonic nonces signed with domain separation tag `TOLA-CONNECT-SESSION-v1-`.
4. **Homoglyph & Confusable Username Phishing:**
   - *Risk:* Attacker registers `@аlice` using Cyrillic 'а' (U+0430) instead of Latin 'a' (U+0061).
   - *Mitigation:* Strict character whitelist: only ASCII `[a-z0-9_]{3,32}` lowercase is admitted. Any code point outside ASCII 0x61..0x7A, 0x30..0x39, and 0x5F is rejected with `InvalidCharacter`.

---

## 5. Implementation Roadmap

1. **Architecture & Protocol Specifications (Docs):**
   - Author `docs/ecosystem/phase-18-architecture.md`
   - Author `docs/ecosystem/tola-connect-protocol.md`
   - Author `docs/ecosystem/tola-connect-security.md`
   - Author `docs/ecosystem/tola-connect-threat-model.md`
   - Author `docs/ecosystem/username-registry.md`
   - Author `docs/ecosystem/tls-721.md`
   - Author `docs/ecosystem/digital-gifts.md`
2. **Subsystem 18.1 Enhancement (TOLA Connect):**
   - Formalize 11 protocol message types, monotonic session nonces, device management registry, and anti-substitution clear signing.
3. **Subsystem 18.2 Enhancement (@username Registry):**
   - Add `tola_resolveUsername` and `tola_getUsernameRecord` RPC endpoints; wire TypeScript and Python resolvers.
4. **Subsystem 18.3 Enhancement (Digital Gifts / TLS-721):**
   - Add `tola_getGiftDetails` and `tola_verifyBadgeAttestation` RPC endpoints; metadata schema validation.
5. **Cross-Language Testing & Verification:**
   - Complete end-to-end unit, integration, and security test matrix across Rust, TypeScript, and Python.
6. **Final Documentation & Log Certification:**
   - Author `docs/ecosystem/phase-18-completion-report.md`, update `PROCESS.md` and `LOGS.md`.
