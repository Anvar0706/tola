# TOLA Network: Cryptographic Identity & Privacy Model

## 1. Foundational Privacy Principles

1. **Minimization of On-Chain Metadata**:
   On-chain state must store only that which is mathematically required to guarantee consensus, state transition validity, and double-spend prevention. Personal identifiers, communication metadata, and social relationships must remain off-chain.
2. **Pseudonymity by Default**:
   Addresses are cryptographic digests of public keys. No real-world KYC, email, or telephone identity is natively coupled to a base TOLA address.
3. **User-Controlled Selective Disclosure**:
   Users should be empowered to prove statements about their identity, balance, or authorization without revealing their underlying private identity or transaction history.

---

## 2. Anonymous Number & Decentralized Identity Architecture

### 2.1 The Anti-Pattern: Phone Numbers On-Chain
- Storing phone numbers on-chain—even when hashed with SHA-256—is an extreme privacy violation.
- Because the space of valid telephone numbers per country code is small ($< 10^9$ possibilities), rainbow tables can reverse any static hash in minutes.
- **TOLA Rule**: Under no circumstances shall raw phone numbers, salted phone numbers, or phone hashes be recorded in on-chain storage.

### 2.2 Anonymous Virtual Numbers (AVNs)
For ecosystem applications requiring virtual identifiers:
1. **Identifier Structure**:
   An Anonymous Virtual Number is a cryptographic commitment:
   $$\text{AVN} = H(\text{SecretSalt} \parallel \text{UserPublicKey})$$
2. **Issuance & Verification**:
   - Issued via a decentralized identity registry or privacy-preserving attestation.
   - Ownership is proven using zero-knowledge proofs (ZKP) or Schnorr signatures over the commitment secret.
   - The virtual number can be bought, transferred, or renewed as a digital asset without disclosing the user's physical SIM or telecom provider.

```
┌──────────────────┐       Blind Signature       ┌──────────────────┐
│  Telecom Gateway │ ──────────────────────────► │    User Client   │
│  (Attests SIM)   │                             │ (Holds Secret X) │
└──────────────────┘                             └────────┬─────────┘
                                                          │ ZK Proof of Valid Attestation
                                                          ▼
                                                 ┌──────────────────┐
                                                 │   TOLA Network   │
                                                 │ (Registers AVN)  │
                                                 └──────────────────┘
```

---

## 3. Username Privacy & Anti-Squatting Model

1. **Public vs Anonymous Usernames**:
   - **Public Username**: Transparently points to a designated TOLA address for public payments and creator profiles (`@alice -> tola1...`).
   - **Stealth Username**: Utilizes dual-key stealth address protocols (DKSAP). When a sender resolves `@bob_private`, the SDK computes an unlinkable one-time destination address:
     $$P_{\text{stealth}} = H(r \cdot K_{\text{view}}) \cdot G + K_{\text{spend}}$$
     Only Bob can detect and spend funds sent to this address, preventing external observers from tracking Bob's payment history.
2. **Anti-Squatting & Fair Allocation**:
   - Progressive auction mechanisms for high-value names.
   - Non-refundable bidding deposits to penalize Sybil squatting.
   - Predictable renewal fees to reclaim abandoned records.

---

## 4. Zero-Knowledge Primitives for Identity

TOLA Network specifies a phased roadmap for zero-knowledge privacy primitives:

### Phase 1: Cryptographic Commitments (Current Foundation)
- Poseidon / Blake3 hashing for state leaves.
- Ed25519 / Secp256k1 signature verification.
- Merkle inclusion proofs.

### Phase 2: Privacy-Preserving Group Signatures (Semaphore-style)
- Enables users to prove membership in a verified group (e.g. "Verified Messenger User", "Anti-Sybil Unique Human") without revealing which specific account they own:
  $$\text{Proof} = \pi_{\text{Groth16/Plonk}}(\text{IdentityCommitment} \in \text{MerkleRoot})$$
- Prevents double-voting, spam, or multi-claim exploitation using cryptographic **Nullifiers**:
  $$\text{Nullifier} = H(\text{IdentitySecret} \parallel \text{ScopeID})$$

### Phase 3: Confidential Asset Transfers (Optional Extension Layer)
- Shielded transactions utilizing Pedersen commitments and Bulletproofs / Halo2 to obscure transfer amounts and asset types when regulatory and compliance profiles permit.

---

## 5. Security & Privacy Threat Matrix

| Threat Vector | Description | TOLA Mitigation |
| :--- | :--- | :--- |
| **Cluster Analysis** | Graph analysis linking multiple dApp interactions to one address | HD wallet derivation (BIP-44/BIP-32); unique addresses generated per interaction. |
| **Metadata Leakage** | RPC nodes logging user IP addresses along with signed transactions | Encrypted P2P gossip propagation; optional Tor / Dandelion++ transaction broadcasting. |
| **Social Graph Profiling** | Linking chat activity to on-chain balances | Physical air-gap: Chat transport is completely decoupled from on-chain transactions. |
| **Phone Number Enumeration** | Brute-forcing hashed telephone numbers | Zero phone numbers on-chain; Anonymous Virtual Numbers built on cryptographic commitments. |
