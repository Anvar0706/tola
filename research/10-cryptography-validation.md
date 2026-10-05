# Research 10: Cryptographic Primitives & Key Management Validation

## 1. Executive Summary & Challenge of Phase 0 Assumptions
Phase 0 proposed Ed25519 for consensus and Secp256k1 for transactions. This research validates the cryptographic schemes, signature encoding formats, domain separation tags, key derivation paths, and threshold signature requirements.

**Core Findings:**
1. **Dual Signature Architecture is Justified:**
   - **Consensus & P2P (Ed25519 / BLS12-381):** Ed25519 provides unmatched verification speed ($> 12,000$ ops/sec/core), zero malleability, and constant-time execution, protecting validators against timing side-channels. For BFT committee voting, BLS12-381 threshold signatures allow compressing 100+ validator signatures into a single 96-byte aggregate signature.
   - **User Transactions (Secp256k1):** Essential for seamless day-one hardware wallet compatibility (Ledger, Trezor), MetaMask, Web3 libraries, and mobile Secure Enclaves.
2. **Hash Function Strategy:**
   - **BLAKE3:** Primary internal hash function for block digests, JMT state tree nodes, transaction hashing, and P2P wire verification. BLAKE3 is $> 4\times$ faster than SHA-256 and $> 8\times$ faster than Keccak-256 on AVX-512/Neon architectures.
   - **Keccak-256:** Preserved strictly inside the EVM smart contract execution environment to maintain compatibility with Solidity contracts.
3. **Domain Separation:** Every signature pre-image must incorporate strict cryptographic domain separation tags to eliminate cross-protocol and cross-network replay vulnerabilities.

---

## 2. Cryptographic Scheme Specification Matrix

| Component | Primitive | Output Size | Security Level | Purpose & Domain Tag |
| :--- | :--- | :--- | :--- | :--- |
| **Validator Consensus Vote** | Ed25519 / BLS12-381 | 64 / 96 Bytes | 128-bit | Vote signing: `TOLA-BFT-VOTE-v1` |
| **P2P Node Identity** | Ed25519 | 32 Bytes PK | 128-bit | libp2p PeerId & Noise handshake |
| **User Transaction** | Secp256k1 (RFC 6979) | 65 Bytes (with recovery $V$)| 128-bit | Tx authorization: `TOLA-TX-v1` |
| **Passkey / WebAuthn** | Secp256r1 (P-256) | 64 Bytes | 128-bit | Native mobile auth (via ERC-4337) |
| **Internal Digest / Tree** | BLAKE3 | 32 Bytes | 128-bit | Block hashes, JMT nodes, Merkle roots |
| **EVM Bytecode / Logs** | Keccak-256 | 32 Bytes | 128-bit | EVM opcode compatibility |
| **Key Derivation** | HKDF-SHA256 / PBKDF2 | 32 Bytes | 128-bit | BIP-39 mnemonic to seed |

---

## 3. Strict Domain Separation Framework

To prevent signature replay attacks across chains, epochs, or subsystems, every signed message must adhere to the canonical envelope:

$$\text{PreImage} = \text{BLAKE3}(\text{DomainTag} \parallel \text{ChainID} \parallel \text{Epoch} \parallel \text{PayloadHash})$$

### Standard Domain Tags:
- `TOLA-CONSENSUS-PREVOTE-v1`
- `TOLA-CONSENSUS-PRECOMMIT-v1`
- `TOLA-TRANSACTION-ENVELOPE-v1`
- `TOLA-CONNECT-SESSION-v1`
- `TOLA-IDENTITY-ATTESTATION-v1`

---

## 4. Key Derivation & Hierarchy (BIP-44)

TOLA accounts utilize canonical hierarchical deterministic (HD) wallet derivation:

$$\text{Path} = m / 44' / 9999' / \text{account}' / \text{change} / \text{address\_index}$$

- `44'`: BIP-44 standard purpose.
- `9999'`: Registered TOLA Coin Type (Testnet uses `1'`).
- `account'`: Hardened account index (default `0'`).
- `change`: `0` for external addresses, `1` for internal/stealth addresses.
- `address_index`: Monotonically increasing address sequence ($0, 1, 2, \dots$).

---

## 5. Architectural Decision Recommendation
**ACCEPT ADR-0002 & ADR-0006**: Dual signature architecture (Ed25519/BLS for consensus, Secp256k1 for transactions) and BLAKE3 hashing are mathematically validated.
