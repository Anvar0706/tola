# SUBSYSTEM: Node Deployment, Orchestration & Portability

## Objective
Design the reproducible, containerized deployment infrastructure, Docker blueprints, bare-metal provisioning scripts, and configuration management for TOLA Network nodes.

## Why It Exists
Node deployment must be straightforward, portable, and reproducible across local development workstations, cheap VPS instances, bare-metal servers, and enterprise cloud infrastructure without vendor lock-in.

## Dependencies
- `prompts/09-storage.md`
- `prompts/12-p2p.md`
- `prompts/37-monitoring.md`

## Non-Goals
- Proprietary cloud-only deployment templates (e.g. AWS-only or GCP-only lock-in).
- Hosting validator signing processes on serverless platforms (e.g. Vercel).

## Requirements
- Zero-Cost Development Strategy: The entire node stack must build, run, and test seamlessly on a standard developer machine ($0 licensing, $0 cloud cost).
- Docker Blueprints:
  - Optimized multi-stage `Dockerfile` producing minimal, unprivileged Alpine/Debian-slim container images.
  - `docker-compose.yml` for instantly launching a local 4-node private validator network.
- Hardware Profile Tiers:
  - **Minimum (Dev/Light)**: 2 vCPU, 4 GB RAM, 50 GB SSD.
  - **Standard (Full Node / RPC)**: 4 vCPU, 8 GB RAM, 200 GB NVMe SSD.
  - **Production (Validator)**: 8 vCPU, 16 GB RAM, 500 GB Enterprise NVMe SSD, 1 Gbps network.
- Configuration Management: Single canonical `tola.toml` configuration file with environment variable overrides.

## Architecture
```
Docker Compose (Local Dev)  ──┐
Systemd Unit (Bare Metal)   ──┼──► TOLA Node Daemon (Rust Binary) ──► Storage / P2P
Ansible / Terraform         ──┘
```

## Interfaces
- Node CLI: `tola init`, `tola start`, `tola validator-keys`, `tola status`.
- Configuration File: `tola.toml`.

## Data Structures
```toml
# tola.toml reference layout
[node]
chain_id = 9999
data_dir = "/var/lib/tola"
log_level = "info"

[p2p]
listen_addr = "/ip4/0.0.0.0/tcp/26656"
bootnodes = []
max_peers = 50

[rpc]
enabled = true
listen_addr = "127.0.0.1:8545"

[validator]
enabled = false
key_file = "/var/lib/tola/validator_key.json"
```

## Security Requirements
- Least privilege: Node process must run as a dedicated unprivileged user (`tola:tola`).
- Firewall guidance: RPC port (`8545`) bound to local interface only; P2P port (`26656`) exposed to internet.

## Failure Modes
- Node failing to start due to file permission errors on data directory.
- Accidental exposure of private RPC methods to the public internet.

## Testing Requirements
- Automated container build and launch tests in CI environment.
- Multi-node local docker-compose health check assertions.

## Performance Requirements
- Container image size $< 80\text{ MB}$.
- Node startup time $< 3\text{ seconds}$.

## Migration Requirements
- Seamless binary replacement with zero configuration file breakage.

## Acceptance Criteria
- Dockerfile, compose manifests, and systemd units documented in `docs/deployment/node-deployment.md`.
- One-command local network launch operational.

## Open Questions
- Should Kubernetes Helm charts be provided natively in Phase 39 or left to community maintainers?

## Implementation Tasks
1. Author multi-stage `Dockerfile`.
2. Author `docker-compose.yml` for 4-node local cluster.
3. Author canonical `tola.toml` configuration template.
4. Draft systemd unit files for production Linux servers.

## Completion Criteria
Deployment manifests verified, container images building successfully, and documentation published.
