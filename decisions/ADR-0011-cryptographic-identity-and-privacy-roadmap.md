# ADR-0011: Cryptographic Identity Primitives (CIP) & Phased Privacy Roadmap

## Title
Purging of Telephony/AVN Concepts and Adoption of Cryptographic Identity Primitives (CIP)

## Status
ACCEPTED

## Context
Phase 0 introduced "Anonymous Virtual Numbers" (AVNs). Research paper `research/13-privacy-validation.md` proved that storing telephone numbers or hashes on-chain is vulnerable to rainbow-table enumeration, and blockchains cannot provide anonymous telecom routing without centralized telecom intermediaries.

## Problem
Establishing a genuine, mathematically sound privacy and identity framework that does not deceive users with unrealistic telecom claims.

## Options
1. *Hashed Phone Numbers on-chain*: Fatal privacy flaw; trivially reversible.
2. *Telecom Gateway Integration*: Centralized point of failure and surveillance choke point.
3. *Cryptographic Identity Primitives (CIP)*: High-entropy commitments, Zero-Knowledge group memberships (Semaphore-style), and Stealth Addresses (DKSAP), scheduled for Phase 19.

## Decision
1. **Purge telecom/phone-number terminology** from protocol-level specifications.
2. Adopt **Cryptographic Identity Primitives (CIP)**:
   - Pseudonymous accounts (Ed25519 / Secp256k1).
   - Anonymous Virtual Identifiers (AVIs) based on Poseidon hash commitments.
   - Zero-Knowledge group membership proofs with nullifiers.
3. **Postpone implementation to Phase 19**, keeping the Genesis L1 core simple, secure, and focused on consensus and execution.

## Why
Protects user privacy mathematically rather than relying on marketing illusions; eliminates premature cryptography from Genesis.

## Consequences
Developers and ecosystem apps use standard cryptographic keypairs and verifiable credentials rather than simulated phone numbers.

## Security Impact
Completely eliminates rainbow-table enumeration attacks.

## Scalability Impact
Prevents premature ZK circuit verification overhead on initial validator nodes.

## Migration Impact
Genesis decision.
