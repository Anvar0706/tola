# TOLA Network: Mainnet Launch Readiness & Genesis Ceremony Runbook

## 1. Readiness Gates & Criteria (Definition of Done)

Prior to initiating the canonical Mainnet Genesis block ceremony, all four launch gates must be satisfied:

| Launch Gate | Requirement | Verification Method | Status |
| :--- | :--- | :--- | :---: |
| **Gate 1: Protocol Completeness** | 100% of subsystem prompts (00–46) implemented | Process audit across all 14 workspace crates | **PASSED** |
| **Gate 2: Security & Adversarial Audit** | Zero critical/high vulnerabilities; invariant engine validated | Fuzz testing, chaos suite, formal invariant tests | **PASSED** |
| **Gate 3: Cross-Platform Parity** | Bit-for-bit test vector parity across Rust, TS, and Python | Automated multi-language CI test suites | **PASSED** |
| **Gate 4: Multi-Node Consensus** | Real TCP P2P networking, HotStuff 2-chain finality | 4-node distributed testnet with crash recovery | **PASSED** |

---

## 2. Canonical Mainnet Parameters

- **Network Name**: TOLA Mainnet
- **Chain ID**: `9999`
- **Address Bech32m HRP**: `tola` (e.g. `tola1...`)
- **Native Asset**: TOLA ($10^{18}$ base units)
- **Genesis Supply**: $1,000,000,000$ TOLA ($10^{27}$ tol)
- **Nominal Block Time**: $2.0 \text{ seconds}$
- **Initial Validators**: Minimum 4 genesis institutional / community validators

---

## 3. Genesis Block Ceremony Protocol

The genesis block creation incorporates verifiable public entropy to ensure trustless initialization:

1. **Entropy Beacon Selection**: Incorporate public randomness from the drand League of Entropy or a predetermined Bitcoin block header hash.
2. **Validator Key Registration**: Genesis validators publish their Ed25519 consensus public keys and Bech32m operator addresses signed with their cold administrative keys.
3. **Deterministic State Compilation**: Execute `compute_genesis_state_root` across sorted genesis accounts and validator sets.
4. **Binary Trust Anchor Imbedding**: The resultant 32-byte Merkle root (`Hash`) is baked into the canonical release tag of `tola-node`.
5. **Simultaneous Launch**: Genesis timestamp set to $T_0$; validators initiate TCP listening and begin Jolteon round 0 block proposal.

---

## 4. Emergency Pause & Multi-Sig Disaster Recovery

In the event of an unforeseen critical consensus stall or catastrophic 0-day exploit during the first 30 days of Mainnet:

1. **Protocol Safe Mode**: If $\ge 33.4\%$ of validators halt, consensus automatically pauses rather than finalizing corrupted state.
2. **Community Security Council**: A 5-of-7 multi-sig council composed of geographically dispersed foundation and community stakeholders can broadcast an emergency advisory pause signal.
3. **State Rollback Safeguard**: Nodes maintain atomic snapshots prior to epoch boundaries, permitting deterministic rollback to the latest finalized 2-chain block.
