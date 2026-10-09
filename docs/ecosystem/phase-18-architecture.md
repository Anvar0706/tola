# TOLA NETWORK — PHASE 18 ECOSYSTEM LAYER ARCHITECTURE

**Status:** APPROVED ARCHITECTURE SPECIFICATION  
**Scope:** Architecture of TOLA Connect, Decentralized @username Registry, Digital Collectibles / Gifts (TLS-721), and Client SDKs  
**Author:** TOLA Core Protocol Engineering

---

## 1. System Overview

The TOLA Ecosystem Layer connects the high-performance distributed blockchain core with everyday consumer applications, social messaging networks (notably Telegram Mini Apps), web portals, and non-custodial user wallets.

```
                    TOLA NETWORK CORE
                          │
               ┌──────────┼──────────┐
               │          │          │
           Wallet      RPC/SDK    Consensus (BFT / Jolteon)
               │          │          │
               └──────┬───┴──────────┘
                      │
                 TOLA CONNECT (E2EE Session Bus)
                      │
         ┌────────────┼────────────┐
         │            │            │
      Identity     Sessions     Signing
         │            │            │
         └────────────┼────────────┘
                      │
               TOLA ECOSYSTEM
                      │
           ┌──────────┴──────────┐
           │                     │
      @username              Digital Gifts (TLS-721)
           │                     │
       Resolution           Collectibles
           │                     │
       Payments               Badges
```

The Ecosystem Layer guarantees:
1. **Zero-Custody Boundary:** User private keys, seed phrases, and mnemonic entropy never leave the user's local secure enclave or device wallet.
2. **Identity Separation:** Messenger/platform identity (e.g. Telegram User ID `123456789`) is decoupled from blockchain cryptographic keys; bindings are established via cryptographically signed attestations.
3. **Transparent Clear-Signing:** Users never blindly approve raw bytecode; native wallets deconstruct every transaction into its human-readable parameters: Domain, Network, Action, Target, Amount, Gas, Nonce, and Memo.

---

## 2. Core Subsystems

### 2.1 Subsystem 18.1: TOLA Connect
TOLA Connect is a non-custodial session protocol providing an end-to-end encrypted (E2EE) communication channel between third-party applications (Telegram Mini Apps, browsers, mobile webviews) and native user wallets.
- **Session Transport:** Encrypted JSON payloads using WebCrypto AES-GCM-256 with ephemeral symmetric session keys.
- **Replay & Hijacking Defense:** Monotonic request sequence nonces, strict expiration timestamps, and domain/origin binding.
- **Device Management:** Users can authorize, inspect, and revoke specific device sessions at will.

### 2.2 Subsystem 18.2: Decentralized @username Registry
A human-readable namespace registry mapping `@username` identifiers to canonical 20-byte TOLA addresses (`tola1...`).
- **Character Whitelist:** Strictly lowercase ASCII alphanumeric and underscore: `[a-z0-9_]{3,32}`. Rejects all Cyrillic, Greek, look-alike confusables, symbols, and zero-width spaces.
- **Two-Phase Commit-Reveal:** Mitigates front-running and MEV bot interception during name registration.
  $$\text{Commitment} = \text{Keccak256}(\text{"TOLA\_USERNAME\_COMMIT\_V1:"} \parallel \text{username} \parallel \text{owner} \parallel \text{salt})$$
- **Tiered Anti-Squatting Pricing:** High annual fees for short handles (3 chars = 10,000 TOLA, 4 chars = 2,500 TOLA, 5 chars = 500 TOLA, 6+ chars = 50 TOLA) with 90-day grace periods.

### 2.3 Subsystem 18.3: Digital Collectibles / Gifts (TLS-721)
A social collectible and limited-edition digital gift standard implemented via smart contract `TolaDigitalGift.sol`.
- **Serial Numbering:** Limited editions with guaranteed immutable serial numbers (`#X of Y`).
- **In-Chat Gifting:** Atomic transfer coupled with optional encrypted greeting messages.
- **Creator Royalties:** Built-in ERC-2981 500 bps (5.0%) royalty standard.
- **Off-Chain Profile Badges:** Off-chain messenger badges bound to verified on-chain token ownership via ECDSA recoverable attestations without heavy media stored on-chain.

---

## 3. Interaction Flows

### 3.1 @username Payment with Clear-Signing

```
User (Messenger/App)      Mini App Webview         User Wallet              TOLA Node (RPC)
       │                         │                      │                          │
       │── Enter "@alice" 25 TOLA─▶                     │                          │
       │                         │── resolveUsername ───┼─────────────────────────▶│
       │                         │◀─ tola1alice... ─────┼──────────────────────────│
       │                         │                      │                          │
       │                         │── TRANSACTION_REQ ──▶│ (Validates Domain,       │
       │                         │                      │  Amount, Recipient,      │
       │                         │                      │  and Memo)               │
       │                         │                      │                          │
       │                         │                      │── Display Native Modal ─▶│ (User Sees:
       │                         │                      │   "Send 25 TOLA to Alice"│  @alice, 25 TOLA,
       │                         │                      │   "Invoice #1042")       │  Memo: #1042)
       │                         │                      │                          │
       │◀────────────────────────┼── User Approves ─────│                          │
       │                         │                      │── Sign & Submit ────────▶│
       │                         │◀─ TX_RESULT (Hash) ──│                          │
```

### 3.2 Digital Gift Profile Badge Verification

```
Collector Wallet               Messenger Client Backend                 Public Verifier
      │                                    │                                    │
      │── signBadgeAttestation(tokenId) ──▶│                                    │
      │   (Signs messengerId + tokenId     │                                    │
      │    + expiresAt)                    │                                    │
      │                                    │── verifyBadgeAttestation(...) ────▶│
      │                                    │   (Recovers owner address,         │
      │                                    │    verifies on-chain ownerOf())    │
      │                                    │◀── Verified: Display 🎁 Badge ─────│
```

---

## 4. Phase 19 Interface Boundary (Preserved & Frozen)

Phase 18 interfaces with Phase 19 (Cross-Shard State Proofs) strictly through public protocol abstractions:
- When a payment or gift transaction crosses shard boundaries, the underlying network routes the transaction using the deterministic high-order byte formula:
  $$\text{ShardID} = ((\text{Address}[0] \ll 8) \mid \text{Address}[1]) \pmod K$$
- The ecosystem layer remains oblivious to shard execution internals; client SDKs automatically partition transactions and query destination receipts without modifying shard verification logic.
