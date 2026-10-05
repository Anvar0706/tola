# Research 15: Infrastructure, Deployment & Operational Cost Model

## 1. Executive Summary & Zero-Cost Development Strategy
TOLA Network is committed to an aggressive **Zero-Cost Development Strategy** during development, testing, and initial benchmarking, while guaranteeing total portability to professional, enterprise-grade validator infrastructure for Mainnet.

**Guiding Principles:**
1. **$0 Developer Baseline:** Core development, compilation, multi-node clustering, fuzzing, and benchmarking must run locally on developer workstations using standard open-source tools (Rust toolchain, Docker, Toxiproxy).
2. **Zero Vendor Lock-In:** Core protocol components must have zero dependencies on proprietary AWS, GCP, or Azure services. Nodes run on standard POSIX Linux/macOS/Windows.
3. **Decoupled Hosting:** Vercel/Cloudflare Pages are strictly restricted to stateless static frontends (Explorer Web, Documentation, Wallet Web UI). Core validator processes and blockchain state storage never touch serverless platforms.

---

## 2. Infrastructure Lifecycle Tiers

```
┌─────────────────────────────────────────────────────────────┐
│ TIER 0: LOCAL DEVELOPMENT ($0 / Month)                      │
│ - Developer Laptop / Workstation (x86_64 or Apple Silicon)  │
│ - Local Docker Compose multi-node private network (4 nodes) │
│ - Free GitHub Actions CI for automated unit & fuzz tests    │
└──────────────────────────────┬──────────────────────────────┘
                               │
┌──────────────────────────────▼──────────────────────────────┐
│ TIER 1: PUBLIC TESTNET ($15 - $40 / Month per Node)         │
│ - Commodity Cloud VPS (Hetzner, OVH, DigitalOcean)          │
│ - 4 vCPU, 8 GB RAM, 160 GB NVMe SSD                         │
│ - Distributed community-operated independent validators     │
│ - Free Vercel hosting for Testnet Explorer & Faucet web UI  │
└──────────────────────────────┬──────────────────────────────┘
                               │
┌──────────────────────────────▼──────────────────────────────┐
│ TIER 2: PRODUCTION MAINNET VALIDATOR ($120 - $250 / Month)  │
│ - Dedicated Bare-Metal Server or High-Performance VPS       │
│ - 8-16 vCPU, 32 GB RAM, 1 TB Enterprise NVMe SSD, 1 Gbps NIC│
│ - Isolated Remote Signer with Hardware Security Key (HSM)   │
│ - Prometheus + Grafana observability stack                  │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. Validator Hardware Profile Specifications

| Component | Minimum Specification (Testnet) | Recommended Specification (Mainnet) | Failure Mode if Insufficient |
| :--- | :--- | :--- | :--- |
| **CPU** | 4 Cores (x86_64 or ARM64) | 8 - 16 Cores (AMD EPYC / Intel Xeon) | Missed block proposal execution deadlines |
| **RAM** | 8 GB ECC RAM | 32 GB ECC RAM | Out-Of-Memory (OOM) kernel termination |
| **Disk Storage** | 200 GB NVMe SSD | 1 - 2 TB Enterprise NVMe (DWPD $\ge 1.0$) | Disk I/O stalls; storage exhaustion |
| **Disk IOPS** | 5,000 Random IOPS | 25,000+ Random IOPS | State trie lookup latency spikes |
| **Network** | 100 Mbps Duplex | 1 Gbps Unmetered Duplex | GossipSub block propagation packet loss |

---

## 4. Validator Key Isolation & Migration Safety

To prevent accidental double-signing during server migration:
1. **Remote Signer Architecture:** The consensus signing private key is isolated in a standalone process (`tola-signer`) running on a dedicated machine or HSM.
2. **Persistent Anti-Equivocation Watermark:** The signer maintains an atomic local database recording:
   $$\text{HighestSignedRecord} = (\text{Height}, \text{Round}, \text{BlockHash})$$
3. If an incoming vote request has $\text{Height} \le \text{HighestSignedRecord.Height}$, the request is **hard-rejected**, rendering accidental double-signing physically impossible even during automated server failovers.
