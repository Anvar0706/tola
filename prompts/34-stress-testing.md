# SUBSYSTEM: Stress, Load & Capacity Testing

## Objective
Design the load generation frameworks, transaction injection pipelines, and sustained stress testing methodology to determine the throughput, latency, and resource ceilings of TOLA Network.

## Why It Exists
A blockchain may appear flawless under 10 transactions per second, only to experience mempool lockups, disk write stalls, or P2P message loss when subjected to sustained bursts of thousands of transactions per second.

## Dependencies
- `prompts/11-mempool.md`
- `prompts/17-execution-engine.md`
- `prompts/26-rpc.md`

## Non-Goals
- Testing on production Mainnet.

## Requirements
- Distributed Load Generator: Dedicated load generator tool capable of broadcasting 1,000 to 20,000 transactions/second across multiple RPC nodes.
- Traffic Profiles:
  1. **Uniform Transfers**: Simple peer-to-peer native value transfers.
  2. **Contended Smart Contract**: Thousands of transactions simultaneously interacting with a single hot contract (e.g. DEX pool or popular NFT mint).
  3. **High-Gas Storage Burners**: Contracts allocating and writing massive storage slots.
- Metrics Collection: Continuous measurement of TPS, block fill ratio, memory residency, CPU utilization, disk write amplification, and network I/O.

## Architecture
```
Traffic Generator Cluster (5-10 Nodes)
              │ Multi-threaded Tx Submission
      ┌───────▼───────┐
      │ TOLA RPC Pool │
      └───────┬───────┘
              │ Mempool Injection
      ┌───────▼───────┐
      │ Validator Set │ (Simulated 20-Node Network)
      └───────┬───────┘
              │ Prometheus Metrics
      ┌───────▼───────┐
      │ Grafana Dash  │ -> Latency, TPS, IOPS, Memory
      └───────────────┘
```

## Interfaces
```rust
pub trait LoadGenerator {
    fn configure_profile(&mut self, profile: TrafficProfile);
    fn start_injection(&mut self, target_tps: usize, duration_seconds: u64);
    fn report_metrics(&self) -> PerformanceReport;
}
```

## Data Structures
```rust
pub struct PerformanceReport {
    pub sustained_tps: f64,
    pub peak_tps: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub total_transactions_committed: u64,
    pub failed_transactions: u64,
}
```

## Security Requirements
- Safe test harness: Load testing restricted to local Docker networks and isolated devnets.
- Resource isolation: Stress tests must not leak uncollected metrics to production dashboards.

## Failure Modes
- Mempool starvation or crash under sustained transaction overload.
- RocksDB write stalls freezing block commit threads.

## Testing Requirements
- 24-hour continuous burn-in test at 2,000 sustained TPS without node crashes or memory leaks.

## Performance Requirements
- Load injection tool overhead must not bottleneck the testing pipeline.

## Migration Requirements
- Configurable load scenarios maintained as reproducible code scripts.

## Acceptance Criteria
- Load testing tooling documented in `docs/operations/stress-testing.md`.
- Baseline performance benchmark reports published.

## Open Questions
- What is the maximum sustainable disk write bandwidth on standard NVMe drives during 5,000 TPS workloads?

## Implementation Tasks
1. Author high-throughput transaction spammer in Rust.
2. Script automated multi-node load test deployment via Docker Compose.
3. Configure Grafana dashboard for live TPS and latency monitoring.

## Completion Criteria
Stress testing framework established, benchmark baselines established, and reports documented.
