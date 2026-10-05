# SUBSYSTEM: Observability, Metrics & Telemetry

## Objective
Design the node observability subsystem, Prometheus metrics exposition, distributed tracing, structured logging, and validator alerting pipelines for TOLA Network.

## Why It Exists
Operating a distributed validator network in production without observability is impossible. Node operators and network engineers need real-time insights into consensus health, mempool pressure, peer counts, and system latencies.

## Dependencies
- `prompts/12-p2p.md`
- `prompts/13-consensus.md`
- `prompts/26-rpc.md`

## Non-Goals
- Leaking private user IP addresses or transaction payloads into public telemetry.

## Requirements
- Metrics Engine: Native Prometheus metrics exporter exposed on configurable port (e.g. `:9100/metrics`).
- Metrics Catalog:
  - Consensus: `tola_consensus_height`, `tola_consensus_round`, `tola_consensus_block_time_seconds`, `tola_consensus_missed_proposals`.
  - Network: `tola_p2p_peers_connected`, `tola_p2p_bytes_received_total`, `tola_p2p_bytes_sent_total`, `tola_p2p_gossip_dropped_messages`.
  - Mempool: `tola_mempool_size_transactions`, `tola_mempool_bytes`, `tola_mempool_evictions_total`.
  - Execution: `tola_execution_tps`, `tola_execution_gas_used_total`, `tola_state_db_read_latency_nanos`.
- Structured Logging: JSON structured logging via `tracing` crate with configurable log levels (`TRACE`, `DEBUG`, `INFO`, `WARN`, `ERROR`).
- Health Probes: Standard liveness (`/healthz`) and readiness (`/readyz`) HTTP endpoints.

## Architecture
```
Node Process -> Prometheus Exporter (:9100) -> Prometheus Server -> Grafana Dashboard
             -> Structured Tracing (Stdout / File) -> Fluentbit -> Loki / ELK
```

## Interfaces
```rust
pub trait MetricsRegistry {
    fn register_counter(&self, name: &str, help: &str) -> Counter;
    fn register_gauge(&self, name: &str, help: &str) -> Gauge;
    fn register_histogram(&self, name: &str, help: &str, buckets: Vec<f64>) -> Histogram;
}
```

## Data Structures
```rust
pub struct NodeHealthStatus {
    pub is_syncing: bool,
    pub current_height: u64,
    pub peer_count: usize,
    pub latest_block_timestamp: u64,
}
```

## Security Requirements
- Observability endpoints must be bound to `localhost` by default to avoid exposing node internals to the public internet.
- Zero sensitive data (private keys, mnemonics, raw passwords) written to logs.

## Failure Modes
- High-volume tracing consuming excessive disk I/O and CPU cycles.
- Metrics scraper crashing node due to unbounded histogram buckets.

## Testing Requirements
- Unit tests verifying Prometheus metric exposition syntax.
- Health check verification during simulated node startup, sync, and steady state.

## Performance Requirements
- Telemetry instrumentation overhead $< 1\%$ total CPU utilization.

## Migration Requirements
- Standard metric naming adhering strictly to OpenTelemetry conventions.

## Acceptance Criteria
- Monitoring specification and Grafana dashboards documented in `docs/operations/monitoring.md`.
- Health check endpoints operational.

## Open Questions
- Should OpenTelemetry distributed tracing (OTLP) be enabled for transaction lifecycles?

## Implementation Tasks
1. Integrate `prometheus` and `tracing` crates into core node daemon.
2. Instrument consensus, mempool, and P2P modules with metrics.
3. Build official production Grafana dashboard JSON templates.

## Completion Criteria
Prometheus metrics exporter operational, dashboard templates created, and alerting rules defined.
