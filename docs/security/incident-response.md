# TOLA Network: Incident Response Protocol & Emergency Procedures
**Version:** 1.0.0-rc1  
**Classification:** Public Security Runbook  
**Target Audience:** Core Protocol Maintainers, Security Engineers, and Validator Operators  

---

## 1. Incident Severity Classification Matrix

| Level | Severity | Description | Target Response | Target Resolution |
| :--- | :--- | :--- | :--- | :--- |
| **P0** | **Critical** | Consensus stall, network partition, double spend, state corruption, or funds at risk. | $< 15\text{ minutes}$ | $< 4\text{ hours}$ |
| **P1** | **High** | Block production latency degraded $> 50\%$, validator crash loop, RPC node memory leak, or remote crash vector without fund risk. | $< 1\text{ hour}$ | $< 12\text{ hours}$ |
| **P2** | **Medium** | Minor RPC query discrepancies, P2P peer score jitter, SDK compatibility edge case, or non-consensus UI bug. | $< 4\text{ hours}$ | $< 48\text{ hours}$ |
| **P3** | **Low** | Informational warning, documentation mismatch, or minor metric formatting anomaly. | $< 24\text{ hours}$ | Next scheduled release |

---

## 2. Incident Response Lifecycle

```
┌──────────────┐     ┌──────────────┐     ┌───────────────┐     ┌──────────────┐
│  1. TRIAGE   │ ──> │2. CONTAINMENT│ ──> │3. ROOT CAUSE  │ ──> │4. RESOLUTION │
│ Verify & P-0 │     │ Quorum Safe  │     │  Reproduce &  │     │ Deploy Patch │
│ Classify Bug │     │  Guard Rail  │     │ Fix Invariant │     │ & Post-Mortem│
└──────────────┘     └──────────────┘     └───────────────┘     └──────────────┘
```

### Stage 1: Detection & Triage
1. **Initial Alert:** Telemetry alerts trigger via Prometheus metrics (`tola_block_height` stall $> 10\text{s}$, peer count drop $> 50\%$, consensus round backoff).
2. **On-Call Assessment:** Incident Commander (IC) validates report against live nodes and assigns severity level (P0–P3).
3. **War Room Activation:** For P0/P1 incidents, a private war room is established immediately.

### Stage 2: Containment & Emergency Safety Measures
1. **BFT Safety Guard:** If consensus safety is threatened, validators are instructed to halt voting until an official signed advisory is distributed.
2. **Mempool Ingress Pause:** Public RPC nodes temporarily reject new transaction ingress to isolate uncommitted transactions.
3. **P2P Quarantining:** If a specific payload exploit is discovered, nodes isolate traffic using P2P subnet rate-limiters.

### Stage 3: Root Cause Analysis & Invariant Testing
1. **Deterministic Reproducer:** Isolate minimal failing test case in `security_audit_test.rs` or `fuzz_and_chaos_test.rs`.
2. **Invariant Verification:** Confirm which protocol invariant was breached (Supply, Nonce, Finality, Low-S, or Merkle root).
3. **Patch Development:** Develop hotfix following clean-room engineering principles with 100% regression test coverage.

### Stage 4: Resolution & Validator Network Hotfix
1. **Binary Release:** Tag emergency release (e.g., `v0.1.1-hotfix`) with pre-compiled reproducible binaries and cryptographic SHA-256 checksums.
2. **Validator Coordinated Rollout:**
   * Broadcast cryptographic release hash over encrypted validator emergency channel.
   * Rolling restart of validators with zero state wipe: data directory preserved.
   * Monitor consensus resumption: verify Quorum Certificates reform and height advances.
3. **Public Post-Mortem:** Comprehensive public post-mortem published within 48 hours detailing root cause, timeline, impact, and permanent architectural mitigations.

---

## 3. Validator Emergency Runbook

In the event of a network-level halt or consensus timeout:

### 1. Check Node Status
```powershell
.\target\debug\tola-cli.exe status --rpc-url http://127.0.0.1:8545
```
Or view live telemetry via web dashboard:
```
http://127.0.0.1:8545/explorer
```

### 2. Inspect Consensus Logs
Check for `[STATUS]`, `[CONSENSUS]`, or `[PROPOSAL]` warnings:
```powershell
Get-Content -Tail 50 ./data/node.log
```

### 3. Graceful Restart Procedure
Do NOT delete the `/data` directory. The state database and transaction segments are atomic and crash-resilient:
```powershell
# Stop validator daemon
Stop-Process -Name tola-node

# Start updated binary with existing persistent directory
.\target\debug\tola-node.exe `
  --chain-id 102 `
  --data-dir ./data `
  --p2p-addr 0.0.0.0:30303 `
  --rpc-addr 127.0.0.1:8545 `
  --validator
```

---

## 4. Responsible Disclosure & Bug Bounty Program

We welcome vulnerability submissions from security researchers:
* **Contact:** `security@tola.network` (PGP key published in root security registry).
* **Scope:** All core workspace crates (`tola-node`, `tola-consensus`, `tola-execution`, `tola-state`, `tola-storage`, `tola-p2p`, `tola-rpc`, `tola-crypto`, `tola-codec`).
* **Bounty Tiers:**
  * **Critical (P0):** Up to \$100,000 in native TOLA tokens / USDC.
  * **High (P1):** Up to \$25,000.
  * **Medium (P2):** Up to \$5,000.
* **Safe Harbor:** Any good-faith security research conducted within bounty terms is protected under safe harbor immunity.
