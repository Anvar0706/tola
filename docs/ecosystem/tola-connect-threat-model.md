# TOLA CONNECT THREAT MODEL & ATTACK MITIGATIONS

**Status:** CANONICAL SPECIFICATION  
**Scope:** Formal Threat Model, Adversary Capabilities, and Cryptographic Countermeasures

---

## 1. Threat Landscape & Adversary Model

We model an active adversary $\mathcal{A}$ who may control:
- The Telegram bot or third-party web application server hosting the Mini App.
- The client-side JavaScript execution context of the Mini App webview.
- The network communication link between the webview and backend relays (Man-in-the-Middle).

$\mathcal{A}$ does NOT control:
- The user's native operating system or device secure enclave.
- The memory space of the native TOLA wallet application.
- The TOLA blockchain consensus network.

---

## 2. Attack Vectors & Countermeasures

### 2.1 Attack 1: Payload Substitution (Amount / Recipient / Memo Hijacking)
* **Attack Scenario:**
  The Mini App webview displays "Transfer 1 TOLA to Coffee Shop", but secretly transmits a transaction request to the wallet for "100 TOLA to Attacker's Address" with Memo "Gift".
* **Mitigation:**
  The wallet UI NEVER relies on strings or descriptions rendered by the Mini App. The native wallet parses the actual unsigned transaction payload directly, extracts recipient, amount, fee, and memo, and presents them in an un-spoofable native modal window. The user authorizes the actual cryptographic binary payload, not the webview's visual claim.

### 2.2 Attack 2: Cross-Origin Session Hijacking & Replay
* **Attack Scenario:**
  Malicious site `phish.tola.network` captures a signed approval or session token intended for `dex.tola.network` and attempts to reuse it to drain funds.
* **Mitigation:**
  1. Origin binding: Every session is explicitly tagged with `origin` and `domain`.
  2. The wallet verifies that the incoming request's transport origin matches the origin authorized in `SESSION_INIT`.
  3. Every message contains an incremental monotonic `nonce`. Replayed messages are rejected as duplicate nonces.

### 2.3 Attack 3: Fake Wallet Phishing Prompt
* **Attack Scenario:**
  A malicious Mini App renders a fake HTML modal mimicking the official TOLA Wallet UI, prompting the user: "Enter your 12-word seed phrase to connect".
* **Mitigation:**
  1. TOLA Connect never requests private keys or seed phrases from applications.
  2. The connection handshake only requests account address reading via E2EE sessions.
  3. Official documentation and community education emphasize that seed phrases are NEVER entered inside Telegram or browser webviews.

### 2.4 Attack 4: Chain ID Replay (Cross-Network Confusion)
* **Attack Scenario:**
  Attacker captures a transaction approval signed for Devnet (Chain ID 1337) or Testnet (Chain ID 102) and replays it on Mainnet (Chain ID 9999).
* **Mitigation:**
  TOLA transaction envelopes mandate `chain_id` inside the canonical BLAKE3 signing digest:
  $$\text{SigningDigest} = \text{BLAKE3}(\text{DOMAIN} \parallel \text{ChainID} \parallel \text{Epoch} \parallel \text{PayloadHash})$$
  Transactions signed for one Chain ID fail cryptographic verification on all other networks.

### 2.5 Attack 5: Compromised Relay Man-in-the-Middle (MITM)
* **Attack Scenario:**
  An adversary intercepts the transport bridge between the Mini App and wallet relay.
* **Mitigation:**
  All payload contents are end-to-end encrypted with WebCrypto AES-GCM-256 using an ephemeral symmetric key generated during the initial QR code / deep-link handshake. An intermediary relay only observes ciphertext and random IVs.
