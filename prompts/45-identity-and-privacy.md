# SUBSYSTEM: Cryptographic Identity & Privacy Architecture

## Objective
Design the privacy-preserving identity architecture, Anonymous Virtual Numbers (AVNs), zero-knowledge identity commitments, and social metadata protection for TOLA Network.

## Why It Exists
Blockchains are permanent public ledgers. Placing personal identifiers, telephone numbers, or social graphs directly on-chain exposes users to surveillance, extortion, and physical threats. Privacy must be architected into the system foundation.

## Dependencies
- `prompts/03-cryptography.md`
- `prompts/04-account-model.md`
- `prompts/42-messenger-integration.md`

## Non-Goals
- Placing hashed telephone numbers on-chain (hashes of phone numbers are trivially reversible via rainbow tables).
- Illicit transaction mixing services.

## Requirements
- Absolute Rule: Zero phone numbers, emails, or personal identifiers stored on-chain in plaintext or raw hash form.
- Anonymous Virtual Numbers (AVNs):
  - Cryptographic identifiers based on Pedersen commitments: $\text{AVN} = H(\text{SecretSalt} \parallel \text{UserPublicKey})$.
  - Verified without revealing the user's telecom provider or physical SIM.
- Zero-Knowledge Group Commitments (Semaphore-style):
  - Users can prove membership in a verified social tier (e.g. "Unique Human", "Verified Community Member") without revealing which specific account they own.
  - Cryptographic nullifiers prevent double-spending or multi-voting.
- Contact Graph Isolation: Messenger contacts and chat relationships remain strictly client-side/off-chain.

## Architecture
```
User Secret Salt + Public Key
               │
      Cryptographic Commitment (Poseidon Hash)
               │
      Registered on TOLA Network as AVN
               │
      ZK Proof of Valid Ownership ──► Messenger Verification (Zero PII Leaked)
```

## Interfaces
```rust
pub trait PrivacyEngine {
    fn generate_avn_commitment(secret: &[u8; 32], pubkey: &[u8; 32]) -> [u8; 32];
    fn verify_membership_proof(proof: &ZkProof, merkle_root: &[u8; 32], nullifier: &[u8; 32]) -> bool;
}
```

## Data Structures
```rust
pub struct AnonymousVirtualNumber {
    pub commitment: [u8; 32],
    pub registered_at: u64,
    pub expires_at: u64,
}

pub struct ZkProof {
    pub a: [u8; 32],
    pub b: [u8; 64],
    pub c: [u8; 32],
}
```

## Security Requirements
- Brute-force resistance: Commitments must utilize high-entropy secrets (256-bit cryptographic salts).
- Unlinkability: An attacker observing on-chain AVN transactions cannot determine the user's physical phone number or identity.

## Failure Modes
- User losing their secret salt (account recovery protocols via ZK multi-party recovery).
- Correlating transaction timing with social messaging actions (mitigated by randomized relay broadcast delays).

## Testing Requirements
- Mathematical proof verification for ZK circuits.
- Entropy validation and rainbow table resistance tests.

## Performance Requirements
- ZK proof verification on-chain $< 200,000$ gas.
- Off-chain proof generation on mobile devices $< 2.0\text{ seconds}$.

## Migration Requirements
- Modular ZK verification precompiles supporting Plonk, Groth16, and Halo2 proof formats.

## Acceptance Criteria
- Privacy model fully documented in `docs/privacy-model.md` and `docs/privacy/identity-and-privacy.md`.
- Cryptographic threat analysis committed.

## Open Questions
- What elliptic curve (BN254 vs BLS12-381) offers the best gas performance for our ZK identity verifiers?

## Implementation Tasks
1. Specify Poseidon hash commitment scheme.
2. Author ZK membership verification circuit specifications.
3. Design privacy threat analysis and mitigation guides.

## Completion Criteria
Privacy architecture ratified, cryptographic commitments specified, and threat models approved.
