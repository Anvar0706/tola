# TOLA Network: Cryptography Specification v0.1

## 1. Cryptographic Algorithms & Curves

### 1.1 Digital Signatures
1. **Ed25519 (RFC 8032):**
   - Curve: Curve25519 in Edwards form ($x^2 + y^2 = 1 - \frac{121665}{121666} x^2 y^2$).
   - Used for: Validator consensus voting, BFT Quorum Certificates, P2P peer identity.
   - Public Key: 32 bytes. Signature: 64 bytes.
   - Batch verification enabled via SIMD instructions.
2. **Secp256k1 (RFC 6979):**
   - Curve: $y^2 = x^3 + 7 \pmod p$.
   - Used for: End-user transaction authorization, hardware wallet compatibility.
   - Signature: 65 bytes $(r, s, v)$ with strict low-$S$ validation ($s \le \frac{n-1}{2}$).

### 1.2 Hash Functions
1. **BLAKE3:**
   - Output: 32 bytes (256 bits).
   - Used for: Block digests, JMT state tree nodes, transaction hashing, P2P message IDs.
2. **Keccak-256:**
   - Output: 32 bytes (256 bits).
   - Used for: EVM smart contract opcodes and ABI encoding.

---

## 2. Domain Separation Framework

All signed cryptographic payloads MUST prefix their pre-image with an explicit domain separation tag:

$$\text{SigningDigest} = \text{BLAKE3}(\text{DomainTag} \parallel \text{ChainID} \parallel \text{PayloadBytes})$$

```rust
pub const DOMAIN_CONSENSUS_PREVOTE: &[u8; 24]  = b"TOLA-CONSENSUS-PREVOTE-1";
pub const DOMAIN_CONSENSUS_PRECOMMIT: &[u8; 24] = b"TOLA-CONSENSUS-PRECOMMIT";
pub const DOMAIN_TRANSACTION_PAYLOAD: &[u8; 24] = b"TOLA-TX-ENVELOPE-v1.0.0-";
```

---

## 3. Key Derivation & Memory Security

- **Derivation:** BIP-39 mnemonic phrases derived via PBKDF2 with 2048 rounds.
- **HD Path:** $m / 44' / 9999' / 0' / 0 / i$.
- **Zeroization:** All private key buffers in node memory implement `Zeroize` on drop to prevent heap dump residue.
