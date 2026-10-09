# TOLA Network: Community Security Council 5-of-7 Multi-Sig Charter & Runbook

**Document Identifier:** `TOLA-GOV-SECURITY-COUNCIL-v1.0`  
**Governance Scope:** Mainnet 30-Day Emergency Mitigation & Advisory Pause  
**Threshold Specification:** 5-of-7 M-of-N Cryptographic Quorum  
**Signing Algorithm:** Pure Ed25519 (RFC 8032) with BLAKE3 Domain Separation  

---

## 1. PURPOSE & MANDATE

The **TOLA Community Security Council** is an emergency multi-sig governance committee designed to protect network security, state integrity, and user assets during the initial 30 days of Mainnet launch. 

### Mandate Limits & Guarantees
- **Strictly Emergency Only:** The council has NO ability to alter account balances, mint new tokens, modify tokenomics, or bypass normal decentralized governance for protocol upgrades.
- **Scope of Power:**
  1. Authorize an emergency advisory pause signal (`TOLA_SECURITY_COUNCIL_EMERGENCY_PAUSE_V1`).
  2. Coordinate security patches during critical 0-day exploits or consensus safety violations.
  3. Formally verify air-gapped cryptographic proofs before advising validator operators to halt or resume.
- **Sunsetting:** Security council emergency powers automatically expire or transition to fully on-chain DAO governance following Phase 18 and formal stability verification.

---

## 2. COUNCIL COMPOSITION (5-of-7)

The 7 council seats are geographically and organizationally distributed across independent stakeholders:

| Seat # | Role / Organization | Geographical Region | Key Type | Hardware Storage |
| :---: | :--- | :--- | :---: | :--- |
| **1** | Foundation Security Lead | Europe (Zurich) | Ed25519 | YubiHSM 2 / Air-gapped |
| **2** | Independent Security Auditor Alpha | North America (NYC) | Ed25519 | Ledger Hardware Signer |
| **3** | Independent Security Auditor Beta | Asia-Pacific (Tokyo) | Ed25519 | FIPS 140-2 Level 3 HSM |
| **4** | Core Protocol Engineering Lead | Europe (London) | Ed25519 | Air-gapped Cold Laptop |
| **5** | Ecosystem Staker Representative | North America (SF) | Ed25519 | Trezor Hardware Wallet |
| **6** | Global Node Operator Representative 1 | Asia-Pacific (Singapore) | Ed25519 | AWS CloudHSM |
| **7** | Global Node Operator Representative 2 | Latin America (São Paulo) | Ed25519 | Air-gapped Cold Storage |

---

## 3. CRYPTOGRAPHIC THRESHOLD VERIFICATION

Any emergency pause signal must satisfy the canonical format:

$$\text{Domain} = \text{TOLA\_SECURITY\_COUNCIL\_EMERGENCY\_PAUSE\_V1}$$
$$\text{Payload} = \text{Domain} \parallel \text{ChainId} \parallel \text{Timestamp} \parallel \text{Reason}$$

### Verification Conditions
1. Each signatory must exist in the canonical `SecurityCouncilPolicy`.
2. Each signature is an RFC 8032 pure Ed25519 signature over the payload.
3. No duplicate signatures from the same public key.
4. Total distinct verified signatures $K \ge 5$.

```bash
# Verify pause signal against registered council policy
tola-cli security-council verify-pause \
  --signal pause-signal.json \
  --policy security-council-policy.json
```

---

## 4. SIGNING RUNBOOK FOR COUNCIL MEMBERS

When an emergency pause is declared:

1. **Receive Incident Briefing:** All 7 members join a verified encrypted video bridge.
2. **Review Incident Artifacts:** Verify the exploit vector, state root divergence, or consensus deadlock.
3. **Sign Payload Air-Gapped:**
   Each member executes on their offline machine:
   ```bash
   tola-cli security-council sign-pause \
     --key <OFFLINE_MEMBER_PRIVATE_KEY_HEX> \
     --reason "Exploit mitigation in block processing" \
     --chain-id 9999
   ```
4. **Publish Public Key & Signature:** The member transmits only the 32-byte public key and 64-byte signature to the coordinator.
5. **Aggregate & Publish:** Once 5 valid signatures are compiled into `pause-signal.json`, the signal is broadcast to all node operators and validators.

---

## 5. RESUMPTION & RECOVERY PROTOCOL

Once the vulnerability is patched:
1. Validator operators deploy the patched release binary.
2. A matching resumption signal is signed by 5-of-7 council members.
3. Validators restart node services with standard peer listening:
   ```bash
   sudo systemctl restart tola-node.service
   ```
4. Consensus automatically resumes Jolteon pacemaker rounds and finalizes new blocks.
