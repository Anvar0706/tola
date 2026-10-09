# TOLA Network: Protocol Versioning & Upgrade Coordination

## 1. Overview

Decentralized networks must evolve while strictly safeguarding consensus continuity. TOLA Network enforces deterministic protocol versioning, validator signaling accumulators, and crash-safe halt invariants.

---

## 2. Versioning Specification (SemVer)

The core node binary conforms to Semantic Versioning (`MAJOR.MINOR.PATCH`):
- **PATCH** (`x.y.Z`): Internal performance optimizations, bug fixes, or logging enhancements with zero wire frame or state-transition changes.
- **MINOR / Soft Fork** (`x.Y.z`): Forward-compatible rule tightenings or optional feature sets that do not invalidate existing blocks.
- **MAJOR / Hard Fork** (`X.y.z`): Non-backward-compatible modifications (e.g. cryptographic primitive changes, state schema migrations, transaction serialization updates).

---

## 3. Validator Upgrade Signaling Mechanism

Protocol upgrades are signaled transparently on-chain:

```
[ Code Release v2.0.0 ]
       │
       ▼
[ Validators Propose Blocks Signaling Target Version in Block Headers ]
       │
       ▼
[ Sliding Evaluation Window (10,000 Blocks) Tracks Signal Supermajority ]
       │
       ├── Signaling < 80%: Upgrade remains unscheduled
       │
       └── Signaling >= 80%: Lock Activation Height H_upgrade (Lead Time: 14 Days)
                                      │
                                      ▼
                      [ Automated Protocol Transition at H_upgrade ]
```

### Evaluation Window Rules
- Sliding Window Size: **10,000 blocks** ($\approx 5.5 \text{ days}$).
- Threshold: **80.0% supermajority** of proposals in the window must signal the target version.
- Lead Time: A mandatory activation lead time (e.g. 14 days) guarantees sufficient time for remaining operators to update their binaries.

---

## 4. Hard Fork Safe-Halt Invariant

To eliminate state database corruption or consensus bifurcation when a hard fork activates:
1. Nodes running legacy software compare the current block height against any scheduled activation height $H_{\text{upgrade}}$.
2. If $H \ge H_{\text{upgrade}}$ and the running node's binary does not support the new `MAJOR` version, the node initiates an atomic **Safe Halt**:
   - Commits all valid state prior to $H_{\text{upgrade}}$.
   - Flushes persistent WAL and MDBX transaction logs.
   - Halts the daemon process with a clear log entry:
     ```
     CRITICAL: Protocol Hard Fork v2.0.0 active at height 1,500,000. Running binary (v1.0.0) incompatible. Safely halting daemon to preserve database integrity.
     ```
3. This guarantees that un-upgraded validators never propose invalid blocks or suffer slashing for state divergence.
