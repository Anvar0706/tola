# TOLA Network: Final Architecture — External Asset Tokenization Layer

## 1. Executive Summary & Authoritative Source of Truth

**TOLA is an independent, sovereign blockchain network designed to provide an on-chain ownership and tokenization layer for unique digital assets that originate on external platforms.**

### Core Architectural Principles:
1. **No Native Social/Identity Layer:**
   - TOLA does **NOT** create messenger identities or operating namespaces.
   - TOLA does **NOT** create messenger usernames.
   - TOLA does **NOT** create Gifts Numbers.
   - TOLA does **NOT** create external messenger gifts.
   - TOLA does **NOT** operate a social identity namespace.
   - TOLA does **NOT** operate a messenger ecosystem.

2. **Complete Independence from External Messengers:**
   - TOLA and external messengers (PaxtaGram, Telegram, X, Discord, etc.) are 100% independent.
   - The TOLA blockchain node runtime never calls, depends upon, or synchronizes with external platform databases, APIs, authentication backends, or servers.
   - TOLA consensus, state transitions, mempool validation, and block production are fully sovereign.

3. **External Asset Tokenization Pipeline:**
   ```
   ┌───────────────────────┐
   │   External Platform   │ (Owns original asset in proprietary DB:
   │ (PaxtaGram, TG, X...) │  e.g. @anvar, Number #12345, Gift #987)
   └──────────┬────────────┘
              │ 1. User requests tokenization
              ▼
   ┌───────────────────────┐
   │  Independent 3rd-Party│ (Verifies user ownership off-chain;
   │   Tokenization App    │  signs ECDSA attestation to user's TOLA wallet)
   └──────────┬────────────┘
              │ 2. Issues attestation
              ▼
   ┌───────────────────────┐
   │      TOLA Wallet      │ (Submits signed mint transaction)
   └──────────┬────────────┘
              │ 3. Broadcasts tx
              ▼
   ┌───────────────────────┐
   │    TOLA Blockchain    │ (Validates attestation, executes Block-STM,
   │   (L1 Core Engine)    │  records TLS-721 token with canonical_id)
   └──────────┬────────────┘
              │ 4. Secondary market transfer
              ▼
   ┌───────────────────────┐
   │       TOLA NFT        │ (On-chain representation transferred to Bob;
   │   (TLS-721 Token)     │  External platform database is 100% UNTOUCHED!)
   └───────────────────────┘
   ```

---

## 2. Decoupled Ownership Invariant

**CRITICAL INVARIANT:**
- TOLA NFT ownership represents **blockchain ownership of the representation on TOLA**.
- TOLA NFT ownership does **NOT** represent automatic, legal, or administrative ownership of the external platform's database.
- Transferring a TOLA NFT changes only the on-chain ledger record. The external platform database remains completely untouched unless and until an external platform specifically reads the TOLA ledger through independent integration.

---

## 3. Canonical External Asset Taxonomy

TOLA provides on-chain tokenization for 8 standardized external asset classes via `ExternalAssetType`:

| ID | Variant | Description | Canonical ID Format |
|---|---|---|---|
| `1` | `MessengerUsername` | External messenger username handle (e.g. `@anvar`) | `Keccak256("TOLA_EXTERNAL_ASSET_CANONICAL_V1:" \|\| src_len \|\| src \|\| 1 \|\| id_len \|\| id)` |
| `2` | `MessengerNumber` | External messenger phone/Gifts number (e.g. `#12345`) | `Keccak256("...:" \|\| src_len \|\| src \|\| 2 \|\| id_len \|\| id)` |
| `3` | `MessengerGift` | External messenger collectible gift (e.g. `#987`) | `Keccak256("...:" \|\| src_len \|\| src \|\| 3 \|\| id_len \|\| id)` |
| `4` | `MessengerBadge` | External messenger profile badge or tier | `Keccak256("...:" \|\| src_len \|\| src \|\| 4 \|\| id_len \|\| id)` |
| `5` | `SocialUsername` | Social network handle (e.g. X/Twitter handle) | `Keccak256("...:" \|\| src_len \|\| src \|\| 5 \|\| id_len \|\| id)` |
| `6` | `SocialNumber` | Social network identity number | `Keccak256("...:" \|\| src_len \|\| src \|\| 6 \|\| id_len \|\| id)` |
| `7` | `DigitalCollectible` | Generic external digital collectible | `Keccak256("...:" \|\| src_len \|\| src \|\| 7 \|\| id_len \|\| id)` |
| `8` | `OtherExternalAsset` | Other platform-defined external asset | `Keccak256("...:" \|\| src_len \|\| src \|\| 8 \|\| id_len \|\| id)` |

---

## 4. Concepts Strictly Excluded from TOLA

The following legacy concepts **do NOT exist** in TOLA:
- ❌ **TOLA Native Username**: TOLA does not provide on-chain `.tola` handles or identity registration.
- ❌ **TOLA Native Identity**: TOLA has no sovereign social namespace.
- ❌ **TOLA Native Gift / TGIFT**: TOLA has no native gift platform.
- ❌ **Native Gifts Number**: TOLA does not issue or track numbers natively.
- ❌ **PaxtaGram Consensus Coupling**: TOLA nodes have no dependency on PaxtaGram.
