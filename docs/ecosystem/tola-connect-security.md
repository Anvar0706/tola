# TOLA CONNECT SECURITY ARCHITECTURE

**Status:** CANONICAL SPECIFICATION  
**Scope:** Security Guarantees, Clear-Signing Invariants, Cryptographic Boundary, and Replay Protection

---

## 1. Zero-Custody Architectural Invariants

TOLA Connect enforces strict physical and cryptographic isolation between host applications and user keys:

1. **Local Enclave Exclusivity:** Private keys, seed phrases, and mnemonic entropy reside exclusively inside the user's native wallet process or hardware enclave.
2. **Untrusted Host Context:** The embedding webview (e.g. Telegram Mini App runtime, Safari/Chrome iframe) is treated as an **untrusted foreign environment**.
3. **No Signature Delegation:** Wallets NEVER expose raw signing primitives (`secp256k1_sign_arbitrary`) to applications without clear human authorization.

---

## 2. Clear-Signing Invariants

Clear-signing transforms machine bytecode into unambiguous, human-verifiable transaction summaries prior to signing.

### 2.1 The 9 Mandatory Modal Parameters
Before any signature is generated, the native wallet UI must present a tamper-proof confirmation displaying:

1. **Domain:** Authenticated origin of the requesting application (e.g. `app.tola.network`).
2. **Action Name:** Purpose of the transaction (e.g. `Mint Collectible`, `Pay Invoice`).
3. **Network & Chain ID:** Verification of target chain (`Mainnet (9999)`, `Testnet (102)`, `Devnet (1337)`).
4. **Sender (`From`):** User's active Bech32m address (`tola1...`).
5. **Recipient (`To`):** Explicit destination address in Bech32m format.
6. **Amount:** Exact value in native TOLA tokens formatted with decimals.
7. **Maximum Fee:** Gas limit and estimated base fee.
8. **Memo:** Canonical UTF-8 memo string (up to 256 bytes) or `None`.
9. **Nonce:** Monotonic account sequence number.

### 2.2 Blind-Signing Prohibition
Any attempt to request a transaction signature without displaying the full payload parameters is strictly rejected by the wallet client library.

---

## 3. Cryptographic Domain Separation

All protocol digests and commitments are strictly separated using unique domain tags:
- **Session Auth:** `TOLA-CONNECT-SESSION-v1-` (24 bytes)
- **Transaction Envelope:** `TOLA-TX-ENVELOPE-v1.0.0-` (24 bytes)
- **Username Commitment:** `TOLA_USERNAME_COMMIT_V1:`
- **Profile Badge Attestation:** `TOLA_BADGE_ATTESTATION_V1:`

Cross-protocol reuse of hashes is mathematically impossible across domain tags.

---

## 4. Replay & Session Hijacking Defenses

1. **Monotonic Session Nonces:** Each request within a session must increment `nonce`. Duplicate or descending nonces trigger immediate session suspension.
2. **Time-Bounded Expiry:** Every request carries an `expiresAt` timestamp (default: 300 seconds). Wallets reject requests where $\text{CurrentTime} > \text{expiresAt}$.
3. **Session Revocation:** Users can revoke active sessions or individual devices at any time. Revoked sessions immediately reject all subsequent requests.
