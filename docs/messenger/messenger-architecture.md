# TOLA Messenger & Telegram Mini Apps Integration Architecture

## 1. Architectural Doctrine: Strict Protocol Decoupling (ADR-0005)

TOLA Network strictly decouples the Layer-1 blockchain protocol from all messaging clients, external social networks, and bot platforms:

```
                  ┌─────────────────────────────────────────┐
                  │          TOLA Layer-1 Network           │
                  │   (Pipelined BFT, Block-STM, MDBX)      │
                  └────────────────────▲────────────────────┘
                                       │ Standard JSON-RPC / WebSocket
                  ┌────────────────────┴────────────────────┐
                  │             TOLA Client SDK             │
                  │        (TypeScript, Python, Rust)       │
                  └────────────────────▲────────────────────┘
                                       │ TOLA Connect E2EE Bridge
                  ┌────────────────────┴────────────────────┐
                  │         TOLA Connect Protocol           │
                  │      (App-Wallet Handshake & IPC)       │
                  └─────────┬─────────────────────┬─────────┘
                            │                     │
         ┌──────────────────▼──┐       ┌──────────▼──────────────┐
         │   Telegram Mini App │       │ Native Messenger Client │
         │   (Sandboxed View)  │       │   (Non-Custodial App)   │
         └─────────────────────┘       └─────────────────────────┘
```

### Invariants:
1. **Blockchain Sovereignty**: The blockchain runs autonomously. If any messenger, bot API, or server cluster shuts down or is blocked, Layer-1 block production and state finality remain 100% unaffected.
2. **Zero Chat Data On-Chain**: No chat messages, media payloads, voice notes, or contact address books are stored on-chain. Only economic settlements and tokenized external assets exist on the ledger.
3. **No Privileged Keys**: The messenger possesses zero master keys, admin backdoors, or special governance powers over the blockchain.

---

## 2. Non-Custodial Client Architecture

The mobile and web client embeds a non-custodial wallet ensuring sovereign private key security:

### Key Custody & Storage
- **Hardware Isolation**: On mobile devices (iOS/Android), private keys are generated inside the iOS Secure Enclave or Android Keystore backed by hardware security modules (HSM / StrongBox).
- **Zero Server Knowledge**: Private keys, seed phrases, and unencrypted transaction payloads never leave the local user device. Messenger servers only relay encrypted envelopes.
- **Biometric Authorization**: High-value transactions require mandatory biometric confirmation (FaceID, TouchID, Fingerprint) or a 6-digit hardware PIN.

---

## 3. TOLA Connect Protocol Specification

TOLA Connect provides an authenticated, end-to-end encrypted (E2EE) session between third-party Mini Apps and the user's non-custodial wallet.

### 3.1 Handshake Sequence
1. **Manifest Presentation**: The Mini App requests a connection, presenting its domain, icon, and app metadata.
2. **Ephemeral Key Exchange**: The wallet and Mini App establish a 256-bit symmetric session key via authenticated Diffie-Hellman or WebCrypto CSPRNG key generation.
3. **Encrypted Channel**: All subsequent communication travels over AES-GCM-256 authenticated encryption:
   $$\text{Ciphertext} = \text{AES-GCM-256}_{K_{\text{session}}}(\text{Payload}, \text{IV}_{12})$$
   $$\text{WireEnvelope} = [\text{IV}_{12} \parallel \text{Ciphertext} \parallel \text{AuthTag}_{16}]$$

### 3.2 Clear-Signing Security Modal
To eliminate blind-signing phishing attacks, any transaction requested by a Mini App must pass through a strict **Clear-Signing Modal** displaying:
- **WHO**: Originating domain, verified publisher status, target address ([`Bech32m`](../../crates/tola-types/src/address.rs)).
- **WHAT**: Action name and method intent (e.g., "Purchase Collectible", "Register Username").
- **HOW MUCH**: Exact token quantity formatted in whole TOLA and decimals, plus maximum gas fee ceiling.
- **WHICH ASSET**: Explicit native currency ticker (`TOLA`) or verified token contract address.
- **CHAIN ID**: Explicit network verification (`9999` for Mainnet, `102` for Testnet).

```
+-------------------------------------------------------+
|        TOLA Connect: Authorize Transaction            |
|                                                       |
|  App:         TOLA Social Market (shop.tola.network)  |
|  Action:      Send In-Chat Payment                    |
|  Recipient:   tola1zlz3s5t8gq0dqr847ke0e97eh07m05p9t  |
|  Amount:      25.0000 TOLA                            |
|  Max Fee:     21,000 gas (~0.000021 TOLA)             |
|  Network:     TOLA Mainnet (Chain ID: 9999)           |
|                                                       |
|        [ REJECT (Esc) ]         [ APPROVE (FaceID) ]   |
+-------------------------------------------------------+
```

---

## 4. Telegram Mini App Sandbox Policy

Mini Apps embedded within Telegram run inside sandboxed webviews with strict Content Security Policy (CSP) enforcement:

1. **Origin Verification**: The wallet inspects `window.location.origin` and checks against an on-chain/community domain reputation registry.
2. **Permission Scoping**:
   - `tola_requestAccounts`: Grants read access to public address only.
   - `tola_sendPayment`: Requires explicit clear-signing confirmation for every single outbound transaction.
   - Batch signing or blind approvals for arbitrary contract calls are strictly disabled by default.
3. **Idempotency & Replay Protection**:
   - Every payment request includes an incrementing client nonce and timestamp window (valid for 5 minutes). Duplicate payment requests within the window are rejected immediately.

---

## 5. In-Chat Instant Payments Architecture

In-chat social transfers enable users to send TOLA directly to contacts or group chats:

```
Sender (Alice)                     TOLA Node (RPC/WS)                Recipient (Bob)
     │                                     │                                │
     ├───── 1. Clear-Sign & Broadcast ────►│                                │
     │         (Alice -> Bob 25 TOLA)      ├────── 2. Mempool & Block ──────┤
     │                                     │          Execution Wave        │
     │◄──── 3. Immediate Tx Hash ──────────┤                                │
     │                                     ├────── 4. WebSocket Event ─────►│
     │◄──── 5. Finality QC (4.0s) ─────────┴──────    Payment Received ─────┤
```

- **Execution Cadence**: Sub-second mempool propagation, Jolteon 2-chain QC commit in 4.0 seconds.
- **Push Notification**: The recipient client receives a real-time WebSocket balance update event, updating the chat UI feed instantly.
