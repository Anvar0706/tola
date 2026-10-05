# SUBSYSTEM: Chaos Engineering & Network Simulation

## Objective
Design and implement the fault-injection simulation environment to evaluate TOLA Network's safety, liveness, and self-healing resilience under extreme real-world network and hardware failures.

## Why It Exists
Real-world distributed systems face flaky networks, hardware crashes, corrupt disks, and malicious validator coalitions. Chaos testing proves that TOLA maintains safety and resumes liveness without manual intervention.

## Dependencies
- `prompts/12-p2p.md`
- `prompts/13-consensus.md`
- `prompts/16-finality.md`

## Non-Goals
- Simulating unprovable physics-level failures.

## Requirements
- Chaos Scenarios:
  1. **Validator Crash & Reboot**: Kill 10% to 30% of validators mid-round; verify seamless recovery.
  2. **Network Partitions**: Split network into 50/50 and 70/30 partitions; verify that the minority halts safely while majority proceeds (or both halt safely if split 50/50).
  3. **Adversarial Latency & Packet Loss**: Inject 300ms packet latency and 20% packet drop rates; verify BFT pacemaker timeouts and round step-ups.
  4. **Byzantine Fault Injection**: Nodes proposing conflicting blocks (equivocation), broadcasting invalid signatures, or manipulating timestamps.
  5. **Abrupt Power Loss**: Force `kill -9` during RocksDB atomic block write batch; verify WAL recovery on reboot.

## Architecture
```
Chaos Controller (Daemon)
        │ Injects: Network Latency / Process Kill / Disk Corruption
┌───────┴───────────────────────┬───────────────────────────────┐
│                               │                               │
▼                               ▼                               ▼
Node 1 (Normal)          Node 2 (Partitioned)            Node 3 (Crash Reboot)
│                               │                               │
└───────────────────────────────┼───────────────────────────────┘
                                ▼
              Safety Invariant Verifier (Zero Double Spends)
```

## Interfaces
```rust
pub trait ChaosInjector {
    fn partition_network(&mut self, partition_a: Vec<NodeId>, partition_b: Vec<NodeId>);
    fn inject_packet_loss(&mut self, node: NodeId, loss_rate: f64);
    fn terminate_node(&mut self, node: NodeId, restart_delay_ms: u64);
    fn corrupt_storage(&mut self, node: NodeId);
}
```

## Data Structures
```rust
pub struct ChaosExperiment {
    pub experiment_name: String,
    pub failure_modes: Vec<FailureType>,
    pub duration_seconds: u64,
    pub expected_safety: bool,
    pub expected_liveness: bool,
}
```

## Security Requirements
- Zero Safety Violations: Under NO circumstances may two conflicting blocks be finalized at the same height, even under severe chaos injection.
- Automatic recovery: When network connectivity is restored, partitioned nodes must automatically catch up and resume validation.

## Failure Modes
- Consensus deadlock requiring manual hard-fork intervention.
- Corrupted local database requiring complete resync from scratch.

## Testing Requirements
- Automated chaos test suite executed on multi-node local cluster before every major release.

## Performance Requirements
- Node recovery and state reconciliation $< 30\text{ seconds}$ after partition healing.

## Migration Requirements
- Test scripts executable across both Docker and Kubernetes test environments.

## Acceptance Criteria
- Chaos testing framework documented in `docs/operations/chaos-testing.md`.
- 100% pass rate on core safety invariants during partition experiments.

## Open Questions
- What network emulation framework (Pumba, Chaos Mesh, Toxiproxy) provides the most reliable automation?

## Implementation Tasks
1. Build containerized multi-node testbed with Toxiproxy network control.
2. Author scripted chaos test scenarios (partitions, crashes, drops).
3. Author automated safety assertion monitor.

## Completion Criteria
Chaos engineering framework operational, test scenarios validated, and zero safety invariants violated.
