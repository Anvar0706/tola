# TOLA CONNECT PROTOCOL SPECIFICATION

**Protocol Version:** `v1.0.0`  
**Status:** CANONICAL SPECIFICATION  
**Scope:** Client-to-Wallet Non-Custodial Encrypted Session Transport

---

## 1. Overview

TOLA Connect is a non-custodial, end-to-end encrypted (E2EE) session protocol enabling decentralized applications (dApps), Telegram Mini Apps, web applications, and external messengers to communicate securely with native TOLA wallets.

### Core Tenets
1. **Zero-Trust Transport:** The communication relay or embedding webview has zero visibility into unencrypted session contents. All data is encrypted with AES-GCM-256.
2. **Non-Custodial Isolation:** Private keys never leave the wallet host environment.
3. **Domain & Origin Binding:** Every request is cryptographically bound to the application origin to prevent phishing and cross-origin hijacking.

---

## 2. Session Protocol Lifecycle & Message Types

The protocol defines **11 canonical message types**:

```
Application (Mini App)                         User Wallet
       │                                            │
       │── 1. SESSION_INIT ────────────────────────▶│ (Validates Domain & Version)
       │◀─ 2. SESSION_ACCEPT ───────────────────────│ (Generates Session ID & Key)
       │                                            │
       │── 3. DEVICE_REGISTER (Optional) ──────────▶│ (Registers Authorized Device)
       │◀─ [ACK] ───────────────────────────────────│
       │                                            │
       │── 4. SIGN_REQUEST ────────────────────────▶│ (Displays Clear-Signing Modal)
       │◀─ 5. SIGN_APPROVAL / 6. SIGN_REJECTION ────│
       │                                            │
       │── 7. TRANSACTION_REQUEST ─────────────────▶│ (Inspects Payload & Memo)
       │◀─ 8. TRANSACTION_APPROVAL ─────────────────│
       │                                            │
       │── 9. TRANSACTION_SUBMISSION ──────────────▶│ (Broadcasts to Node via RPC)
       │◀─ 10. TRANSACTION_RESULT ──────────────────│ (Returns Tx Hash & Status)
       │                                            │
       │── 11. DEVICE_REVOKE ──────────────────────▶│ (Revokes Device Session Key)
```

---

## 3. Message Schemas & Field Definitions

All TOLA Connect messages share an authenticated protocol envelope.

### 3.1 Authenticated Protocol Envelope

```json
{
  "version": "1.0",
  "sessionId": "a1b2c3d4e5f60718293a4b5c6d7e8f90",
  "type": "TRANSACTION_REQUEST",
  "requestId": "req_01h8v7k9p...",
  "nonce": 42,
  "timestamp": 1780000000,
  "expiresAt": 1780000300,
  "origin": "https://defi.tola.network",
  "domain": "defi.tola.network",
  "appId": "tola_swap_bot",
  "payload": { ... }
}
```

#### Field Specifications:
| Field | Type | Required | Description | Security Validation |
|---|---|---|---|---|
| `version` | string | Yes | Protocol version (`"1.0"`) | Must match supported version |
| `sessionId` | string (hex, 32 char) | Yes | Unique 128-bit session identifier | Must match active session |
| `type` | string (enum) | Yes | One of the 11 protocol message types | Must follow valid state transition |
| `requestId` | string | Yes | Unique UUID/ULID for request correlation | Must be non-empty, unique per request |
| `nonce` | integer | Yes | Monotonically increasing sequence number | Must be strictly $> \text{last\_seen\_nonce}$ |
| `timestamp` | integer (u64) | Yes | Unix timestamp in seconds | Must be within clock skew window ($\pm 60\text{s}$) |
| `expiresAt` | integer (u64) | Yes | Expiration timestamp in seconds | $\text{CurrentTime} \le \text{expiresAt}$ |
| `origin` | string | Yes | Fully qualified origin URL | Must match approved session origin |
| `domain` | string | Yes | Canonical domain name | Must match origin hostname |
| `appId` | string | Optional | Application identifier or bot handle | Normalized identifier |
| `payload` | object | Yes | Encrypted or plain payload data | Subtype-specific schema |

---

## 4. Message Payloads

### 4.1 SESSION_INIT & SESSION_ACCEPT
* `SESSION_INIT`: Sent by application to initiate connection.
  - `payload`: `{ "appName": "TolaSwap", "iconUrl": "...", "requestedChains": [9999] }`
* `SESSION_ACCEPT`: Sent by wallet upon user approval.
  - `payload`: `{ "walletAddress": "tola1...", "chainId": 9999, "supportedFeatures": ["clear-signing", "memo"] }`

### 4.2 DEVICE_REGISTER & DEVICE_REVOKE
* `DEVICE_REGISTER`: Authorizes a new client device or browser session.
  - `payload`: `{ "deviceId": "dev_macbook_pro", "deviceName": "Anvar MacBook Pro", "devicePublicKey": "0x..." }`
* `DEVICE_REVOKE`: Revokes an authorized device immediately.
  - `payload`: `{ "deviceId": "dev_macbook_pro" }`

### 4.3 TRANSACTION_REQUEST & TRANSACTION_APPROVAL
* `TRANSACTION_REQUEST`:
  ```json
  {
    "type": "TRANSACTION_REQUEST",
    "payload": {
      "to": "tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j",
      "amount": "25000000000000000000",
      "assetSymbol": "TOLA",
      "actionName": "Transfer to Alice",
      "memo": "Invoice #1042",
      "maxFee": 21000,
      "chainId": 9999
    }
  }
  ```
* `TRANSACTION_APPROVAL`:
  ```json
  {
    "type": "TRANSACTION_APPROVAL",
    "payload": {
      "approved": true,
      "txHash": "0x5f8b91a...",
      "signedPayload": "0x..."
    }
  }
  ```

---

## 5. End-to-End Encryption (E2EE) Specification

All inter-process messages between the application and wallet are encrypted using **AES-GCM-256**:
- **Key Derivation / Exchange:** 256-bit symmetric key generated via CSPRNG during handshake.
- **IV / Nonce:** 96-bit (12-byte) unique IV generated per encrypted frame via CSPRNG.
- **Wire Framing:**
  $$\text{WirePayload} = \text{Base64}(\text{IV}_{12\text{B}} \parallel \text{Ciphertext} \parallel \text{AuthTag}_{16\text{B}})$$
- **Decryption:** The receiving party extracts the 12-byte IV, verifies the 16-byte authentication tag, and decrypts the ciphertext. If authentication fails, the message is discarded with an error.
