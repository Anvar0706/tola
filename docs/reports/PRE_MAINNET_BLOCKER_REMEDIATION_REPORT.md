# TOLA NETWORK — PRE-MAINNET BLOCKER REMEDIATION REPORT

**Document Identifier:** `TOLA-REMEDIATION-2026-MAINNET-01`  
**Execution Phase:** Pre-Mainnet Blocker Remediation  
**Status:** COMPLETE  
**Final Verdict:** **READY FOR GENESIS CEREMONY**  
**Timestamp:** October 7, 2026  

---

## 1. EXECUTIVE SUMMARY & VERDICT

In accordance with the findings of the **Final Mainnet Readiness Audit** (Mainnet Readiness Score: 91/100, Conditional Go), a targeted remediation stage was executed to resolve the two verified Mainnet blockers without expanding scope, without modifying consensus/tokenomics/governance, and strictly maintaining the boundary prior to Phase 18.

### Definitive Remediation Verdict

```
┌────────────────────────────────────────────────────────────────────────┐
│                        REMEDIATION DETERMINATION                       │
│                                                                        │
│                >>> READY FOR GENESIS CEREMONY <<<                      │
│                                                                        │
│  BLOCKER-01 (Chain ID Schism): Fully resolved and harmonized.         │
│  - Mainnet Chain ID is canonical 9999 across Core, SDKs, Node, Docs.   │
│  - Testnet Chain ID remains strictly isolated at 102.                  │
│                                                                        │
│  BLOCKER-02 (Mock Genesis Keys): Ceremony infrastructure prepared.     │
│  - Offline key generation runbook & ceremony specification authored.   │
│  - Status correctly designated as PENDING AUTHENTIC CEREMONY KEYS.     │
│  - Zero private keys generated or stored within the repository.        │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 2. BLOCKER-01: CANONICAL MAINNET CHAIN ID REMEDIATION

### 2.1 State Before Remediation
Prior to remediation, a three-way schism existed across the repository:
- `genesis-mainnet.json` declared Chain ID **9999**.
- `crates/tola-types/src/primitives.rs` defined `MAINNET = Self(101)`.
- Client SDKs (`@tola/sdk` in TypeScript and `tola` in Python) hardcoded `Network.MAINNET` to **101**.
- Node CLI daemon defaulted unspecified invocations to **102**.
- Consequence: SDK clients attempting to connect to Mainnet nodes running `genesis-mainnet.json` generated transactions that were rejected with `ChainIdMismatch`.

### 2.2 Harmonized Canonical Architecture
The architecture is now synchronized and invariant:
- **Canonical Mainnet Chain ID:** `9999`
- **Canonical Testnet Chain ID:** `102`
- **Devnet Chain ID:** `1337`

### 2.3 Exact Changes Applied

1. **Rust Core Types (`crates/tola-types/src/primitives.rs`):**
   ```rust
   impl ChainId {
       pub const MAINNET: Self = Self(9999);
       pub const TESTNET: Self = Self(102);
       pub const DEVNET: Self = Self(1337);
   }
   ```
2. **Node Daemon Execution & P2P Isolation (`crates/tola-node/src/main.rs`):**
   - Automatically synchronizes node `chain_id` with `genesis_config.chain_id` when `--genesis` is provided.
   - Prevents contradictory CLI arguments by checking for conflicts between `--chain-id` and the loaded genesis file.
   - Binds P2P transport network isolation tag dynamically: `ChainId::MAINNET` $\to$ `NetworkId::Mainnet` (`0x01`), `ChainId::TESTNET` $\to$ `NetworkId::Testnet` (`0x02`), completely preventing cross-network packet leakage.
3. **RPC Dispatcher & Server Health (`crates/tola-rpc`):**
   - Synchronized `net_version` response for `ChainId::MAINNET` to `"9999"`.
   - Updated HTTP health test assertions to verify `chain_id: 9999`.
4. **TypeScript SDK (`sdk/typescript/src/types.js` & `dist/types.js`):**
   ```javascript
   export const NETWORKS = {
     mainnet: { name: 'Mainnet', hrp: 'tola', chainId: 9999n },
     testnet: { name: 'Testnet', hrp: 'ttola', chainId: 102n },
     devnet: { name: 'Devnet', hrp: 'dtola', chainId: 1337n },
   };
   ```
5. **Python SDK (`sdk/python/tola/types.py`):**
   ```python
   @property
   def chain_id(self) -> int:
       if self == Network.MAINNET:
           return 9999
       elif self == Network.TESTNET:
           return 102
       elif self == Network.DEVNET:
           return 1337
       return 9999
   ```
6. **Protocol & RPC Documentation:**
   - Synchronized `docs/protocol/json-rpc-engine.md` (`net_version: "9999"`, `eth_chainId: "0x270f"`).
   - Synchronized `docs/sdk/web3-provider.md` (`eth_chainId: 0x270f` for Mainnet).

### 2.4 Chain ID Consistency Audit
A complete repository scan for `101`, `102`, and `9999` confirmed:
- **`101`:** Zero active production or testnet configurations use `101`. The number `101` exists solely as sliding-window block heights (`heights 101..=105` in `mainnet_tokenomics_governance_test.rs`) and as a loopback port in P2P unit tests (`40101`).
- **`102`:** Strictly and consistently applied across all Testnet environments, `genesis.json`, and testnet docker-compose configs.
- **`9999`:** Uniformly referenced across `genesis-mainnet.json`, `primitives.rs`, SDKs, and all protocol specifications.

---

## 3. BLOCKER-02: MAINNET GENESIS VALIDATOR KEYS REMEDIATION

### 3.1 State Before Remediation
`genesis-mainnet.json` contained synthetic placeholder keys (`0xaaaa...aaaa`, `0xbbbb...bbbb`, `0xcccc...cccc`, `0xdddd...dddd`). Without private keys, no validator can participate in consensus, leading to a permanent halt at Genesis.

### 3.2 Key Security Policy & Compliance
In strict adherence to production cryptographic standards:
- **Zero Private Keys:** No production or ceremony private keys were generated, committed, or stored anywhere in this repository.
- **No Mock Replacements:** The placeholder keys were deliberately NOT replaced with pseudo-random local development keys to falsely simulate completion.
- **Official Status:** **`Genesis Key Ceremony: PENDING AUTHENTIC CEREMONY KEYS`**.

### 3.3 Ceremony Runbook & Procedure Implementation
Created [`docs/deployment/mainnet-genesis-ceremony.md`](../deployment/mainnet-genesis-ceremony.md) defining the exhaustive 10-stage physical/offline ceremony protocol:
1. **Key Generation Algorithm:** RFC 8032 pure Ed25519 using Hardware Security Modules (HSMs) or air-gapped `tola-cli`.
2. **Air-Gapped Offline Environment:** Physical isolation, networking interfaces disabled, verified live OS.
3. **Entropy Requirements:** Minimum 256 bits from hardware TRNG + OS CSPRNG.
4. **Private Key Storage:** Multi-party cold storage, physical safes, Shamir's secret sharing.
5. **Public Key Extraction:** Format specifications (`0x<64 hex characters>`).
6. **Proof of Possession (PoP):** Mandatory digital signature of canonical ceremony payload proving key ownership without private key disclosure.
7. **Operator Identity Verification:** Multi-party live attestation and cross-signing.
8. **Genesis Registration Procedure:** Exact JSON injection schema preserving supply ($10^{27}$ base units) and stake distribution.
9. **Genesis Hash & JMT State Root Derivation:** Independent computation and multi-operator verification.
10. **Independent Multi-Party Confirmation:** Cryptographic sign-off by all 4 operators.

The document also provides the canonical **Operator Registration Template** to record the 4 institutional validator entities during the live event.

---

## 4. VERIFICATION EVIDENCE & TEST SUITE RESULTS

### 4.1 Automated Test Suites

| Test Suite | Command | Result | Notes |
| :--- | :--- | :---: | :--- |
| **Rust Workspace** | `cargo test --workspace` | **PASS (132/132)** | 0 failed; all 14 crates pass |
| **Rust Compilation** | `cargo check --workspace --all-targets` | **PASS** | 0 errors, 0 warnings |
| **Release Build** | `cargo build --workspace --release` | **PASS** | Clean release binary generated |
| **TypeScript SDK** | `node --test sdk/typescript/test/sdk.test.mjs` | **PASS (7/7)** | Chain ID 9999 verified |
| **Python SDK** | `python -m unittest discover -s sdk/python/tests` | **PASS (7/7)** | Chain ID 9999 verified |

### 4.2 Cross-Chain Replay Protection & Isolation Verification
The updated `test_cross_chain_replay_attack_isolation` in `crates/tola-node/tests/security_audit_test.rs` proved:
1. A valid transaction targeting **Mainnet (Chain ID 9999)** is successfully accepted.
2. A legacy transaction signed for **Chain ID 101** submitted to Mainnet is rejected with:
   $$\text{Err}(\text{InvariantViolation::InvalidChainId} \{ \text{expected}: 9999, \text{actual}: 101 \})$$
3. A Testnet transaction signed for **Chain ID 102** submitted to Mainnet is rejected with:
   $$\text{Err}(\text{InvariantViolation::InvalidChainId} \{ \text{expected}: 9999, \text{actual}: 102 \})$$
4. A Mainnet transaction signed for **Chain ID 9999** submitted to Testnet is rejected with:
   $$\text{Err}(\text{InvariantViolation::InvalidChainId} \{ \text{expected}: 102, \text{actual}: 9999 \})$$

### 4.3 Genesis Integrity Verification
Executing `tola-cli genesis show --file genesis-mainnet.json` confirms:
- **Chain ID:** `9999`
- **Total Supply:** `1,000,000,000` TOLA ($10^{27}$ base units)
- **Account Allocations:** 8 accounts summing to 100.0% of genesis supply
- **Initial Validators:** 4 validators with $50,000,000$ TOLA stake each
- **Deterministic State Root:** `0xda826fabad07bfe445c5dd5c38dfcbabd459b5cf7ac58dc852420dc5afc8cc84`

---

## 5. SUMMARY OF RESOLVED DEFECTS

| Blocker ID | Description | Before | After | Status |
| :--- | :--- | :--- | :--- | :---: |
| **BLOCKER-01** | Mainnet Chain ID Inconsistency | Schism: `101`, `102`, `9999` | Mainnet = `9999`, Testnet = `102` | **RESOLVED** |
| **BLOCKER-02** | Mainnet Genesis Validator Keys | Synthetic mock keys (`aaaa...`) | Ceremony runbook & infrastructure ready; keys designated **PENDING** | **READY FOR CEREMONY** |

---

## 6. NEXT STEPS TOWARD PHASE 18

1. **Schedule Genesis Ceremony:** Convene the 4 designated institutional validator organizations.
2. **Execute Ceremony:** Follow [`docs/deployment/mainnet-genesis-ceremony.md`](../deployment/mainnet-genesis-ceremony.md) in air-gapped environments to collect authentic public keys and Proofs of Possession.
3. **Inject Keys:** Replace mock pubkeys in `genesis-mainnet.json` with authentic keys and re-verify the deterministic state root.
4. **Commence Phase 18:** Proceed to public Mainnet genesis bootstrap and node deployment.

---
**Remediation Engineer:** Lead Systems & Protocol Engineer  
**Sign-off:** Independent Pre-Mainnet Verification Suite
