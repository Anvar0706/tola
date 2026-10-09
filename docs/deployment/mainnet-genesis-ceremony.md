# TOLA Network: Mainnet Genesis Key Ceremony Protocol & Operator Runbook

**Document Identifier:** `TOLA-CEREMONY-RUNBOOK-v1.0`  
**Target Network:** TOLA Mainnet (Chain ID `9999`)  
**Ceremony Status:** **PENDING AUTHENTIC CEREMONY KEYS**  
**Classification:** Public Security Specification & Procedure  

---

## 1. PURPOSE & PRINCIPLES

This specification establishes the official, cryptographic, and multi-party protocol for conducting the **TOLA Mainnet Genesis Key Ceremony**. 

The initial genesis configuration (`genesis-mainnet.json`) contains synthetic placeholder public keys (`0xaaaa...aaaa`, `0xbbbb...bbbb`, `0xcccc...cccc`, `0xdddd...dddd`). In accordance with production security standards:
- **No private keys are generated or stored in this repository.**
- **No mock keys or arbitrary development keys shall be used for Mainnet Genesis.**
- **All production consensus signing keys must be generated independently offline by real institutional validator operators.**

---

## 2. TEN-STAGE CEREMONY SPECIFICATION

Each validator operator must adhere to the following 10-stage procedure:

### Stage 1: Key Generation Algorithm
- **Cryptographic Curve:** Pure Ed25519 (Edwards-curve Digital Signature Algorithm over Curve25519) in compliance with RFC 8032.
- **Key Parameters:** 32-byte private seed $\to$ 32-byte public key ($A$).
- **Derivation / Tooling:** Generated via Hardware Security Module (HSM - PKCS#11 / FIPS 140-2 Level 3), air-gapped secure hardware signer (Ledger/YubiKey), or dedicated offline air-gapped machine running `tola-cli validator keygen --airgap`.

### Stage 2: Secure / Air-Gapped Offline Environment
- Key generation must occur on an **air-gapped machine** with all networking interfaces (Wi-Fi, Bluetooth, Ethernet, Cellular) physically disabled or removed.
- Operating system must be a trusted, freshly verified live environment (e.g., Tails OS or minimal Debian Linux verified via SHA-256 and PGP).

### Stage 3: Randomness & Entropy Requirements
- Entropy must be gathered from hardware TRNG (True Random Number Generator) combined with operating system CSPRNG (`/dev/urandom` / `getrandom(2)`).
- Minimum entropy requirement: 256 bits of cryptographically strong pseudo-random entropy.
- Operators must not use deterministic seeds derived from passwords or predictable phrases.

### Stage 4: Private Key Storage Procedure
- The Ed25519 private key seed must **NEVER** be committed to disk in plaintext, emailed, uploaded, or exposed to any network.
- Recommended production storage:
  - Stored inside an institutional HSM (e.g., AWS CloudHSM, YubiHSM 2, or HashiCorp Vault with transit engine).
  - Encrypted backup of the 32-byte seed stored in a tamper-evident physical safe using Shamir's Secret Sharing (3-of-5 threshold) or encrypted with multi-party PGP.

### Stage 5: Public Key Extraction
- Extract only the 32-byte Ed25519 public key in hexadecimal format (`0x<64 hex characters>`).
- Compute the BLAKE3-256 and SHA-256 fingerprints of the public key.

### Stage 6: Public Key Verification (Proof of Possession)
- The operator must prove possession of the private key without exposing it by signing a canonical **Ceremony Proof of Possession (PoP)** payload:
  ```text
  TOLA_MAINNET_GENESIS_CEREMONY_POP_V1
  ChainId: 9999
  OperatorId: <Operator Address Hex>
  ConsensusPubkey: <Public Key Hex>
  Timestamp: <UTC ISO-8601 Timestamp>
  ```
- The signature is verified by the ceremony coordinator using the public key.

### Stage 7: Operator Identity Verification
- Each validator entity must complete decentralized multi-party identity verification:
  - Video conference identity attestation among all 4 validator signers.
  - Cross-signing with existing recognized organizational GPG keys or verified TLS certificates.

### Stage 8: Genesis Registration Procedure
- The ceremony coordinator collects the 4 verified public keys and signatures of possession.
- The coordinator replaces ONLY the placeholder public keys in `genesis-mainnet.json` while strictly preserving:
  - `chain_id`: `9999`
  - Total Genesis Supply: `1,000,000,000` TOLA ($10^{27}$ base units)
  - Allocation distributions (Foundation 30%, Ecosystem 25%, Validator Pool 20%, Public 15%, Core Contributors 10%)
  - Validator voting power: `50,000,000` TOLA each

### Stage 9: Genesis Hash & State Root Verification
- The ceremony coordinator and all operators independently run:
  ```bash
  tola-cli genesis show --file genesis-mainnet.json
  ```
- Each operator computes and verifies:
  1. The SHA-256 and BLAKE3 checksum of `genesis-mainnet.json`.
  2. The deterministic Jellyfish Merkle Tree (JMT) Genesis State Root.
  3. Every operator must independently obtain the identical 32-byte state root.

### Stage 10: Independent Multi-Party Confirmation
- All 4 operators publish their signed commitment to the canonical Genesis State Root and Genesis Block Hash.
- Once 4-of-4 confirmations are collected, the genesis artifact is declared finalized and immutable.

---

## 3. CEREMONY OPERATOR REGISTRATION TEMPLATE

*(This artifact template is completed and signed by each operator during the live ceremony. No private keys are ever recorded here.)*

```
================================================================================
                    TOLA MAINNET GENESIS VALIDATOR RECORD
================================================================================

Validator Operator #1:
--------------------------------------------------------------------------------
Operator ID:            0x________________________________________ (20 bytes)
Organization Name:      ________________________________________
Operator Contact:       ________________________________________
Consensus Public Key:   0x________________________________________________________________ (32 bytes)
Key Fingerprint (SHA256): ________________________________________________________________
Proof-of-Possession Sig: 0x________________________________________________________________
Independent Witness:    ________________________________________
Status:                 [ ] PENDING    [ ] VERIFIED

Validator Operator #2:
--------------------------------------------------------------------------------
Operator ID:            0x________________________________________ (20 bytes)
Organization Name:      ________________________________________
Operator Contact:       ________________________________________
Consensus Public Key:   0x________________________________________________________________ (32 bytes)
Key Fingerprint (SHA256): ________________________________________________________________
Proof-of-Possession Sig: 0x________________________________________________________________
Independent Witness:    ________________________________________
Status:                 [ ] PENDING    [ ] VERIFIED

Validator Operator #3:
--------------------------------------------------------------------------------
Operator ID:            0x________________________________________ (20 bytes)
Organization Name:      ________________________________________
Operator Contact:       ________________________________________
Consensus Public Key:   0x________________________________________________________________ (32 bytes)
Key Fingerprint (SHA256): ________________________________________________________________
Proof-of-Possession Sig: 0x________________________________________________________________
Independent Witness:    ________________________________________
Status:                 [ ] PENDING    [ ] VERIFIED

Validator Operator #4:
--------------------------------------------------------------------------------
Operator ID:            0x________________________________________ (20 bytes)
Organization Name:      ________________________________________
Operator Contact:       ________________________________________
Consensus Public Key:   0x________________________________________________________________ (32 bytes)
Key Fingerprint (SHA256): ________________________________________________________________
Proof-of-Possession Sig: 0x________________________________________________________________
Independent Witness:    ________________________________________
Status:                 [ ] PENDING    [ ] VERIFIED
================================================================================
```

---

## 4. GENESIS INGESTION & UPDATE SCRIPT PROCEDURE

Once the 4 verified public keys are collected, they are registered using the following procedure:

1. Update `genesis-mainnet.json` with the authentic 32-byte consensus public keys.
2. Verify total allocation conservation:
   $$\sum_{i=1}^{n} \text{balance}_i \equiv 1,000,000,000 \cdot 10^{18} \text{ base units}$$
3. Re-run deterministic state root calculation:
   ```bash
   tola-cli genesis show --file genesis-mainnet.json
   ```
4. Verify all tests pass with the authentic genesis file:
   ```bash
   cargo test --workspace
   ```
5. Tag git commit as canonical genesis release.

---
**Protocol Governance:** TOLA Genesis Steering Committee  
**Security Policy:** Strict Hardware Isolation & Zero-Knowledge Private Key Generation
