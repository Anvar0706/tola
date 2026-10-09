# External Asset Security & Adversarial Threat Model

## 1. Security Philosophy & Principles

Tokenizing external assets introduces unique boundary threats because it bridges off-chain, centralized environments with decentralized cryptographic state machines.

The TOLA security architecture operates under zero-trust assumptions:
1. **The blockchain node never trusts off-chain claims without cryptographically verifiable proofs.**
2. **The blockchain node never issues HTTP/network requests to external platforms.**
3. **External platform compromises cannot compromise the integrity of the TOLA blockchain.**
4. **On-chain tokenization never implies automatic or forced mutations in external private databases.**

---

## 2. Threat Analysis & Defensive Mitigations

### Threat 1: Cross-Chain Replay Attacks
- **Attack Vector:** An attacker captures an attestation issued on TOLA Testnet (Chain ID 102) and broadcasts it to TOLA Mainnet (Chain ID 9999) to illicitly mint the asset on Mainnet.
- **Defensive Mechanism:**
  - `chainId` is strictly bound into the Keccak-256 pre-image:
    $$\text{Keccak-256}(\dots \parallel \text{chain\_id}_{8B} \parallel \dots)$$
  - When submitted to a node or smart contract on Mainnet, the recovered signer does not match the authorized verifier.
- **Verification Status:** Fully proven in `test_adversarial_attestation_security_and_replay_defenses` and cross-SDK unit tests.

---

### Threat 2: Cross-Wallet Front-Running & Hijacking
- **Attack Vector:** Alice requests tokenization of `@anvar`. An attacker Mallory observes the attestation in mempool or transit, replaces Alice's address with Mallory's address, and mints the NFT to themselves.
- **Defensive Mechanism:**
  - `destinationWallet` (20 bytes) is cryptographically signed into the pre-image.
  - If Mallory alters the destination wallet parameter, the recovered ECDSA address mismatches the authorized verifier, causing immediate rejection.
- **Verification Status:** Verified across Rust, TypeScript, and Python test suites.

---

### Threat 3: Attestation Expiration & Nonce Staling
- **Attack Vector:** An attacker retains an old attestation for months, waits for market valuation or ownership shifts on the external platform, and executes a stale mint.
- **Defensive Mechanism:**
  - `expiresAt` (Unix timestamp) is enforced at the consensus / RPC / contract level:
    $$\text{assert}(\text{block.timestamp} \le \text{expiresAt})$$
  - Expired attestations return `CryptoError::AttestationExpired` or smart contract revert `AttestationExpired`.
- **Verification Status:** Verified under past-timestamp edge tests.

---

### Threat 4: Signature Malleability (High-S Attacks)
- **Attack Vector:** Secp256k1 signatures $(r, s, v)$ have a dual valid form $(r, -s \pmod n, v \oplus 1)$. A malicious relayer flips $s$ to $-s$ to bypass duplicate attestation hashes or disrupt transaction receipts.
- **Defensive Mechanism:**
  - Strict Low-S canonical enforcement (BIP-146 / RFC 6979) in `verify_secp256k1_low_s`:
    $$s \le \frac{n}{2}$$
  - Any signature with $s > \frac{n}{2}$ is rejected immediately with `CryptoError::MalleableSignatureHighS`.
- **Verification Status:** Verified across all SDK and node crypto checks.

---

### Threat 5: Duplicate Minting of Identical External Assets
- **Attack Vector:** Two different users or services attempt to mint separate TOLA NFTs representing the exact same external handle or gift.
- **Defensive Mechanism:**
  - Deterministic 32-byte Canonical ID:
    $$\text{Canonical ID} = \text{Keccak-256}(\text{"TOLA\_EXTERNAL\_ASSET\_CANONICAL\_V1:"} \parallel \text{platform} \parallel \text{type} \parallel \text{id})$$
  - The node registry and smart contract store `canonicalIdToToken`. If `canonicalId` already exists, minting is rejected with `AssetAlreadyTokenized`.
- **Verification Status:** Verified in node integration and RPC dispatcher tests.

---

### Threat 6: SSRF & Malicious Metadata Injection
- **Attack Vector:** A malicious issuer provides metadata URIs containing private IP addresses, AWS metadata endpoints (`169.254.169.254`), or loopback URLs (`http://localhost`) targeting wallet or node infrastructure.
- **Defensive Mechanism:**
  - Multi-layer SSRF validation in SDKs and indexing clients via `validateExternalAssetMetadata`.
  - Rejection of private IPv4 ranges (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 127.0.0.0/8, 169.254.0.0/16) and localhost domains.
  - Enforcement of decentralized protocols (`ipfs://`, `ar://`) or secure HTTPS with public hostnames.
- **Verification Status:** Verified with dedicated test suites in TypeScript and Python SDKs.

---

### Threat 7: PII & Data Privacy Exposure
- **Attack Vector:** Storing user phone numbers, full names, or private messaging identifiers directly on the public immutable ledger.
- **Defensive Mechanism:**
  - Prohibition of raw PII on-chain.
  - External assets use opaque platform IDs (e.g., anonymous handle strings or numerical IDs).
  - Sensitive profile attributes are kept strictly off-chain within user-controlled metadata or encrypted storage.

---

### Threat 8: Misconception of Automatic Off-Chain Control
- **Attack Vector:** A user purchases an NFT representing an external handle expecting that the external platform is legally or mechanically bound to transfer the handle inside the proprietary application.
- **Defensive Mechanism & Legal Decoupling:**
  - Architectural standard explicitly declares: "TOLA TLS-721 ownership represents blockchain ownership of the representation on TOLA. It does NOT constitute automatic legal or administrative control of the external platform asset."
  - Third-party tokenization applications must display explicit clear-signing disclaimers during the mint and purchase flow.
