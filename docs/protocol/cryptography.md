# TOLA Network: Cryptographic Primitives & Key Management Specification

## 1. Overview & Security Objectives
Cryptography provides the mathematical foundation of TOLA Network's security, consensus finality, and transactional authenticity. This document defines the production cryptographic schemes, curve parameters, domain separation rules, key derivation hierarchies, and side-channel defense standards implemented in `tola-crypto`.

---

## 2. Digital Signature Schemes

### 2.1 Ed25519 (RFC 8032) — Consensus & Peer Identity
- **Curve:** Twisted Edwards curve over the prime field $\mathbb{F}_{2^{255}-19}$:
  $$-x^2 + y^2 = 1 - \frac{121665}{121666} x^2 y^2$$
- **Primary Domain:**
  - Validator BFT consensus voting (Prevotes, Precommits, Timeout certificates).
  - P2P node identity (libp2p PeerId) and Noise Protocol (XX handshake).
- **Key Sizes:**
  - Secret Key: 32 bytes (256-bit seed expanded to 64-byte SHA-512 digest).
  - Public Key: 32 bytes (compressed point encoding).
  - Signature: 64 bytes $(R, S)$.
- **Security Properties:**
  - Pure deterministic signatures (RFC 8032 Ed25519ph / PureEd25519).
  - Batch verification support: verifies $K$ signatures simultaneously using random linear combinations, achieving $> 84,000\text{ ops/sec}$ per core.
  - Immunity to signature malleability.

### 2.2 Secp256k1 (RFC 6979) — User Transactions & Account Ownership
- **Curve:** Koblitz curve defined over $\mathbb{F}_p$ where $p = 2^{256} - 2^{32} - 977$:
  $$y^2 = x^3 + 7 \pmod p$$
- **Primary Domain:**
  - End-user transaction authorization.
  - Hardware wallet compatibility (Ledger, Trezor, Keystone).
  - Ethereum / MetaMask / EIP-712 compatibility.
- **Key Sizes:**
  - Private Key: 32 bytes (scalar $d \in [1, n-1]$).
  - Public Key: 33 bytes (compressed) or 65 bytes (uncompressed).
  - Signature: 65 bytes $(r, s, v)$ where $v \in \{0, 1\}$ is the recovery ID.
- **Strict Anti-Malleability Requirement:**
  Signatures are strictly validated for canonical low-$S$ representation:
  $$s \le \frac{n-1}{2}$$
  Any transaction presenting $s > \frac{n-1}{2}$ is rejected at the stateless validation boundary.

---

## 3. Cryptographic Hash Functions

### 3.1 BLAKE3 — High-Speed Internal Hashing
- **Architecture:** Tree-based hashing primitive derived from Bao and BLAKE2.
- **Output:** 32 bytes (256 bits).
- **Primary Domain:**
  - Block header digests and transaction IDs (`TransactionId`).
  - Jellyfish Merkle Tree (JMT) internal branch and leaf node commitments.
  - Domain-separated signing pre-images.
- **Performance:** Measured at $52.4\text{ ns}$ per 32-byte digest on AVX-512 ($7.3\times$ faster than Keccak-256).

### 3.2 Keccak-256 & SHA-256
- **Keccak-256:** Preserved strictly inside EVM contract execution and 20-byte address derivation:
  $$\text{Address} = \text{Keccak256}(\text{PubKey}_{\text{uncompressed}}[1..65])[12..32]$$
- **SHA-256:** Standard NIST FIPS 180-4 hashing used for BIP-39 checksums and light-client cross-chain proofs.

---

## 4. Canonical Domain Separation Framework

To eliminate signature replay attacks across chains, epochs, or subsystems, every signed pre-image MUST adhere to the canonical envelope:

$$\text{SigningDigest} = \text{BLAKE3}(\text{DomainTag} \parallel \text{ChainID} \parallel \text{Epoch} \parallel \text{PayloadHash})$$

### Registered Domain Separation Tags:
```rust
pub const DOMAIN_CONSENSUS_PREVOTE: &[u8; 24]  = b"TOLA-CONSENSUS-PREVOTE-1";
pub const DOMAIN_CONSENSUS_PRECOMMIT: &[u8; 24] = b"TOLA-CONSENSUS-PRECOMMIT";
pub const DOMAIN_TRANSACTION_PAYLOAD: &[u8; 24] = b"TOLA-TX-ENVELOPE-v1.0.0-";
pub const DOMAIN_CONNECT_SESSION: &[u8; 24]     = b"TOLA-CONNECT-SESSION-v1-";
pub const DOMAIN_IDENTITY_COMMITMENT: &[u8; 24] = b"TOLA-IDENTITY-COMMIT-v1-";
```

---

## 5. Hierarchical Key Derivation (BIP-39 & BIP-44)

1. **Mnemonic Generation:**
   - 12 or 24 words generated using high-entropy CSPRNG ($128$ or $256$ bits) mapped via standard BIP-39 English wordlist.
   - Seed derivation: PBKDF2 with HMAC-SHA512 (2048 iterations).
2. **Canonical HD Derivation Path:**
   $$m / 44' / 9999' / \text{account}' / \text{change} / \text{address\_index}$$
   - `44'`: BIP-44 standard purpose.
   - `9999'`: Registered TOLA Coin Type (Testnet uses `1'`).
   - `account'`: Hardened account index (default `0'`).
   - `change`: `0` for external receiving addresses; `1` for internal/stealth addresses.
   - `address_index`: Sequential integer ($0, 1, 2, \dots$).

---

## 6. Memory Protection & Zeroization

All private key and mnemonic seed buffers implement the `zeroize::Zeroize` and `zeroize::ZeroizeOnDrop` traits. Upon drop or process termination, memory regions containing secret key material are explicitly overwritten with zeroes using volatile writes to prevent cold-boot and swap memory leakage.
