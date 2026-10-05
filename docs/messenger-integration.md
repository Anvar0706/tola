# TOLA Messenger Integration Architecture

## 1. Architectural Independence Principle

The core architectural doctrine of TOLA Network is **Strict Protocol Decoupling**:

```
                       ┌───────────────────────────────┐
                       │     TOLA Layer-1 Protocol     │
                       │    (Consensus, State, VM)     │
                       └───────────────▲───────────────┘
                                       │ Standard RPC / JSON-RPC
                       ┌───────────────┴───────────────┐
                       │        TOLA Core SDK          │
                       │    (TypeScript, Rust, Go)     │
                       └───────────────▲───────────────┘
                                       │ E2EE Protocol
                       ┌───────────────┴───────────────┐
                       │         TOLA Connect          │
                       │   (App-Wallet Handshake)      │
                       └───────┬───────────────┬───────┘
                               │               │
            ┌──────────────────▼──┐         ┌──▼──────────────────┐
            │   TOLA Messenger    │         │ Standalone Web/App  │
            │   (Ecosystem Client)│         │       dApps         │
            └─────────────────────┘         └─────────────────────┘
```

1. **Protocol Sovereignty**: TOLA Network is a self-sustaining Layer-1 blockchain. If the TOLA Messenger ceases operations, is banned, or changes its technology stack, the blockchain continues to produce blocks, finalize transactions, and validate state without interruption.
2. **Client Equality**: The TOLA Messenger enjoys no hidden administrative privileges, backdoors, or non-standard cryptographic keys on the blockchain. It interacts with the network strictly through public RPCs and standard transactions like any third-party wallet or client.
3. **Zero Chat Data On-Chain**: Message bodies, images, audio, typing indicators, delivery statuses, and private contact graphs are **STRICTLY OFF-CHAIN**. Only economic settlements, verifiable identity commitments, username registries, and collectible gift ownership reside on-chain.

---

## 2. Messenger Wallet Integration Architecture

### 2.1 Non-Custodial Architecture
The wallet embedded within the TOLA Messenger client must remain 100% non-custodial:
- **Client-Side Key Generation**: Private keys (Ed25519 or Secp256k1) are generated directly in the user's secure hardware storage (iOS Secure Enclave, Android Keystore, or OS-level encrypted vaults).
- **Zero Server Knowledge**: The messenger backend servers never receive, transmit, or cache user private keys, seeds, or unencrypted transaction pre-images.
- **Biometric & PIN Authorization**: All outbound transactions require explicit user biometric (FaceID, Fingerprint) or hardware PIN confirmation before signing.

### 2.2 Account Abstraction & Identity Mapping
To ensure privacy and avoid data leakage:
- **No Phone Numbers On-Chain**: Phone numbers or email addresses are NEVER written to the blockchain, nor are their raw hashes (which are trivially vulnerable to rainbow-table enumeration).
- **Cryptographic Binding**: A messenger user profile can bind to a TOLA address by signing an ephemeral authentication challenge:
  $$\text{BindingProof} = \text{Sign}_{SK_{\text{TOLA}}}(\text{MessengerUID} \parallel \text{Timestamp} \parallel \text{Nonce})$$
- The messenger server verifies this signature to display on-chain badges or assets without taking control of the account.

---

## 3. TOLA Connect: dApp & Mini-App Connection Protocol

### 3.1 Overview
TOLA Connect enables seamless, secure interaction between Mini Apps (running inside web views within the messenger) and the user's non-custodial wallet:

```
Mini App (Webview)               TOLA Connect Bridge                User Wallet (Client)
        │                                 │                                    │
        ├─── 1. Handshake Request ───────►│                                    │
        │    (Manifest + Ephemeral PK)    ├───── 2. Route Encrypted Session ──►│
        │                                 │                                    ├── 3. Decrypt & Verify
        │                                 │                                    │   Display Consent
        │                                 │◄──── 4. Approve (Session Key) ─────┤
        │◄── 5. Connection Established ───┤                                    │
        │                                 │                                    │
        ├─── 6. Request Transaction ─────►│                                    │
        │    (Recipient, Value, Payload)  ├───── 7. Route Request ────────────►│
        │                                 │                                    ├── 8. User Confirmation
        │                                 │                                    │   (WHO, WHAT, AMOUNT)
        │                                 │                                    ├── 9. Sign Transaction
        │                                 │                                    ├── 10. Broadcast to L1
        │◄── 12. Return Tx Hash ──────────┼───── 11. Emit Success Event ───────┤
```

### 3.2 End-to-End Encryption (E2EE)
- Sessions are secured via Diffie-Hellman key exchange (X25519) combined with ChaCha20-Poly1305 authenticated encryption.
- Bridge relays (whether centralized HTTP relays, local WebSocket relays, or WebRTC channels) carry only encrypted ciphertexts.

### 3.3 Clear-Signing Security Modal
To combat malicious Mini Apps and phishing attacks, the wallet modal must implement **Strict Clear-Signing**:
- **WHO**: Domain, verified publisher status, app icon, contract address.
- **WHAT**: Method name, human-readable function intent (e.g., "Transfer Gift #104" instead of raw hex bytecode).
- **HOW MUCH**: Exact token/asset amounts, maximum gas fee allowance.
- **WHICH ASSET**: Explicit asset symbol, verified contract badge.
- **PERMISSIONS**: If contract approval/allowance is requested, highlight the risk of unlimited approvals.

---

## 4. Digital Gifts & Social Collectibles Model

### 4.1 Separation of Ownership and Profile Association
```
     On-Chain Ownership                             Off-Chain Messenger Profile
┌───────────────────────────┐                      ┌───────────────────────────┐
│     TOLA Gift Contract    │                      │     User Profile Page     │
│  - Token ID: #4092        │                      │  - Display Gift: #4092    │
│  - Owner: 0x8A4...b1C     │◄── Verified Proof ───┤  - Social Badges          │
│  - Transferable: true     │                      │  - Showcase Gallery       │
└───────────────────────────┘                      └───────────────────────────┘
```
1. **Verifiable Ownership**: The token owner is strictly the TOLA on-chain address.
2. **Display Delegation**: The owner may authorize a messenger profile to display the gift in their profile showcase by issuing an off-chain signed attestation.
3. **Transfer Independence**: If the owner sells or transfers the gift on a marketplace, the on-chain transfer event is indexed, and the messenger profile automatically removes the display badge upon state verification.

---

## 5. Threat Model for Messenger Integration

1. **Malicious Mini App Exploits**:
   - *Mitigation*: Sandboxed iframes, strict Content Security Policy (CSP), domain verification, automated contract reputation scanning.
2. **Session Hijacking**:
   - *Mitigation*: Ephemeral session keys with strict time-to-live (TTL) and per-transaction re-authentication for high-value actions.
3. **Phishing and Address Spoofing**:
   - *Mitigation*: Checksummed addresses (Bech32m), verified username resolution, visual identicons.
4. **Messenger Server Compromise**:
   - *Mitigation*: Non-custodial keys ensure that a complete breach of the messenger server infrastructure yields zero private keys or unauthorized transactions.
