# Research 13: Privacy Architecture & Anonymous Number Validation

## 1. Executive Summary & Challenge of Phase 0 Assumptions
Phase 0 introduced the concept of an "Anonymous Virtual Number" (AVN). This research performs a formal cryptographic, threat modeling, and regulatory assessment of that concept.

**Core Findings:**
1. **The Telephony Fallacy:** Blockchains cannot natively create real, working cellular phone numbers without integrating centralized telecom carriers or SMS gateways. If a carrier gateway binds an on-chain key to a real telephone number, the carrier's logs immediately break anonymity. Furthermore, storing phone numbers or phone number hashes on-chain is catastrophically vulnerable to rainbow-table enumeration ($< 10^9$ valid numbers per country).
2. **Marketing Deception vs Cryptographic Reality:** Calling a standard cryptographic keypair or DID an "anonymous phone number" is misleading. What users and applications actually require is **Pseudonymous Identity with Selective Social Disclosure**.
3. **Viable Privacy Primitives:**
   - **Anonymous Virtual Identifiers (AVIs):** Cryptographic commitments over high-entropy random secrets ($256$ bits), independent of telecom networks.
   - **Zero-Knowledge Group Membership (Semaphore-style):** Proving that a user belongs to a verified tier (e.g. "Unique Human", "Verified Community Subscriber") using Groth16/Plonk proofs and cryptographic nullifiers without revealing their on-chain address or balance.
   - **Stealth Addresses (DKSAP):** Unlinkable one-time destination addresses for private peer-to-peer transfers.
4. **Recommendation:** **REDESIGN AND POSTPONE AVN**. Completely purge telecom terminology from the protocol. Formalize the **Cryptographic Identity Primitives (CIP)** framework, deferred to **Phase 19**.

---

## 2. Threat Analysis of Privacy Candidates

| Candidate Primitive | Anonymity Set | Sybil Resistance | Linkability Vulnerability | Implementation Complexity | Verdict |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Hashed Phone Numbers** | Tiny ($< 10^9$) | Telecom SIM | **Trivial Rainbow Table Reversal** | Low | **STRICTLY FORBIDDEN** |
| **Carrier Blind Signatures** | Limited | Carrier KYC | Carrier traffic correlation | Very High | **REJECTED (Centralized)** |
| **Anonymous Virtual IDs (AVI)** | Infinite ($2^{256}$) | Economic Deposit | None (Pure entropy) | Low | **APPROVED FOR PHASE 19** |
| **ZK Group Membership** | Set of $M$ members | Nullifiers | None (ZK Proof) | Moderate | **APPROVED FOR PHASE 19** |
| **Dual-Key Stealth Addresses**| Unlinkable | Natural Gas | Timing correlation if broadcasted directly | Moderate | **APPROVED FOR PHASE 19** |

---

## 3. Cryptographic Specification of Anonymous Virtual Identifiers (AVI)

Instead of a fake "phone number", an AVI is a verifiable cryptographic identity commitment:

$$\text{IdentitySecret} \leftarrow \text{CSPRNG}(256\text{ bits})$$
$$\text{NullifierKey} \leftarrow \text{BLAKE3}(\text{IdentitySecret} \parallel \text{"TOLA-NULLIFIER"})$$
$$\text{Commitment} = \text{Poseidon}(\text{IdentitySecret}, \text{NullifierKey})$$

- The commitment is registered in an on-chain Merkle tree.
- When authorizing an off-chain social action or voting, the user submits a Zero-Knowledge Proof:
  $$\pi = \text{ZK-Proof}\{\text{Commitment} \in \text{MerkleRoot} \land \text{Nullifier} = \text{Poseidon}(\text{NullifierKey}, \text{ScopeID})\}$$
- The protocol verifies the proof and stores the nullifier to prevent double-spending or multi-voting, while learning **zero information** about the user's on-chain address.

---

## 4. Architectural Decision Recommendation
**CREATE ADR-0011**: Formally reject phone-number-based on-chain primitives. Adopt the **Cryptographic Identity Primitives (CIP)** specification and schedule implementation for Phase 19.
