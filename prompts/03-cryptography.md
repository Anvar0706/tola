# SUBSYSTEM: Cryptographic Primitives & Schemes

## Objective
Specify and implement the foundational cryptographic schemes for TOLA Network: digital signatures, cryptographic hashes, key derivation, and verifiable random functions.

## Why It Exists
Cryptography provides the mathematical bedrock of blockchain security. Flawed signature schemes or weak hash algorithms lead to total loss of funds, forged blocks, and network collapse.

## Dependencies
- `prompts/00-project-foundation.md`
- `prompts/02-architecture.md`

## Non-Goals
- Implementing novel, unproven cryptographic curves or custom hash algorithms.
- Post-quantum lattice schemes in Phase 1 (reserved for future cryptographic agility upgrade).

## Requirements
- Signature Schemes:
  - **Ed25519**: For ultra-fast, high-security validator consensus voting and P2P peer identity.
  - **Secp256k1**: For user account transactions to ensure full EVM and hardware wallet compatibility.
- Hash Functions:
  - **BLAKE3**: For ultra-fast internal hashing, block digests, and storage trees.
  - **Keccak-256**: For EVM compatibility and contract execution hashing.
  - **SHA-256**: For standard interoperability and Merkle proofs.
- Key Derivation: BIP-39 (Mnemonic) and BIP-44 (Hierarchical Deterministic derivation paths: `m/44'/9999'/0'/0/0`).

## Architecture
Cryptographic Service Provider module exposing strict, constant-time algorithms with zero memory residue (`zeroize` on drop for private keys).

## Interfaces
```rust
pub trait SignatureScheme {
    type PublicKey;
    type PrivateKey;
    type Signature;
    fn sign(sk: &Self::PrivateKey, msg: &[u8]) -> Self::Signature;
    fn verify(pk: &Self::PublicKey, msg: &[u8], sig: &Self::Signature) -> bool;
}
```

## Data Structures
```rust
pub struct KeyPair {
    pub public_key: [u8; 32],
    pub private_key: Zeroizing<[u8; 32]>,
}

pub struct Signature64(pub [u8; 64]);
pub struct Hash256(pub [u8; 32]);
```

## Security Requirements
- All private key buffers must implement `Zeroize` on drop to eliminate memory dump risks.
- Constant-time verification to prevent side-channel timing attacks.
- Protection against signature malleability (strict low-$S$ validation for Secp256k1).

## Failure Modes
- Key leakage through core dumps or swap memory.
- Nonce reuse in signature generation (use RFC 6979 deterministic signatures).
- Weak entropy generation during node initialization.

## Testing Requirements
- 100% test coverage using official RFC 8032 test vectors for Ed25519 and RFC 6979 for Secp256k1.
- Differential fuzzing against established reference libraries (`ed25519-dalek`, `k256`).

## Performance Requirements
- Ed25519 batch signature verification $> 10,000$ ops/sec per CPU core.
- BLAKE3 hashing throughput $> 1\text{ GB/sec}$.

## Migration Requirements
- Cryptographic agility: Transaction envelopes include signature algorithm version byte.

## Acceptance Criteria
- Cryptography specification documented in `docs/protocol/cryptography.md`.
- Cryptographic test vectors passing with zero warnings.

## Open Questions
- Should BLS12-381 aggregate signatures be adopted in Phase 1 or deferred to Phase 9 (staking scale)?

## Implementation Tasks
1. Specify exact curve parameters and serialization formats.
2. Define key derivation paths for TOLA accounts.
3. Document side-channel mitigation standards.

## Completion Criteria
Cryptographic specification verified against standard test vectors and approved in ADR.
