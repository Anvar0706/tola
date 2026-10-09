# TOLA Network — Official Testnet Validator Onboarding Guide

## 1. Overview & Role of Validators

Validators form the backbone of the TOLA Network Proof-of-Stake consensus engine. They run the `tola-node` daemon to:
- Propose new blocks containing validated user transactions when scheduled as view leader.
- Verify incoming block proposals against the Jellyfish Merkle Tree state commitments.
- Sign cryptographic precommit votes using Ed25519 threshold keys.
- Assemble Quorum Certificates (QCs) and enforce Jolteon 2-chain commit finality.
- Participate in dynamic epoch committee rotation and bonded stake security.

---

## 2. Hardware & Network Requirements

| Specification | Minimum Requirement | Recommended Production |
| :--- | :--- | :--- |
| **CPU** | 8 vCPU / 4 Cores (x86_64 or aarch64) | 16 vCPU / 8 Cores (AMD EPYC or Intel Xeon) |
| **Memory** | 16 GB RAM | 32 GB ECC RAM |
| **Storage** | 250 GB NVMe SSD | 1 TB NVMe SSD ($\ge 10,000$ IOPS) |
| **Network** | 250 Mbps Symmetrical | 1 Gbps Dedicated Symmetrical |
| **Public IP** | 1 Static Public IPv4 | Static IPv4 + IPv6 Subnet |
| **Operating System** | Ubuntu 22.04 LTS / Debian 12 | Ubuntu 24.04 LTS / Debian 12 |

### Firewall Port Configuration
- **Port 30303 (TCP):** Inbound P2P Wire Protocol (MUST be open to public internet).
- **Port 8545 (TCP):** JSON-RPC / Prometheus Metrics (MUST be firewalled or restricted to localhost / private VPC).

---

## 3. Installation & Setup

### Option A: Building from Source
Ensure Rust 1.80+ is installed:
```bash
git clone https://github.com/tolanetwork/tola.git
cd tola
cargo build --release --bin tola-node --bin tola-cli
sudo cp target/release/tola-node /usr/local/bin/
sudo cp target/release/tola-cli /usr/local/bin/
```

### Option B: Pre-built Docker Image
```bash
docker pull tolanetwork/tola-node:latest
```

---

## 4. Key Generation Ceremony

Validators require two cryptographic identities:
1. **Consensus Keypair (Ed25519):** Signs BFT consensus votes and block proposals.
2. **Operator Keypair (Secp256k1):** Controls validator rewards, staking, and governance.

Generate keys using `tola-cli`:
```bash
# Generate operator account
tola-cli account new

# Example output:
# Address (Hex):      0xcecc1507dc1ddd7295951c290888f095adb9044d
# Address (Testnet):  ttola1cecc1507dc1ddd7295951c290888f095af50z8
# Public Key (Hex):   0x03a1b2c3...
```

Generate or provide a 32-byte Ed25519 consensus seed hex:
```bash
openssl rand -hex 32 > /etc/tola/consensus.key
chmod 600 /etc/tola/consensus.key
```

---

## 5. Directory Setup & Genesis Synchronization

Create system directories:
```bash
sudo mkdir -p /var/lib/tola/data
sudo mkdir -p /etc/tola
sudo useradd -r -s /bin/false tola
sudo chown -R tola:tola /var/lib/tola /etc/tola
```

Download the official `genesis.json`:
```bash
curl -sSL https://testnet.tola.network/genesis.json -o /etc/tola/genesis.json

# Verify genesis state root
tola-cli genesis show --file /etc/tola/genesis.json
```

---

## 6. Systemd Service Configuration

Create `/etc/systemd/system/tola-node.service`:

```ini
[Unit]
Description=TOLA Network Validator Node
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=tola
Group=tola
WorkingDirectory=/var/lib/tola
ExecStart=/usr/local/bin/tola-node \
    --chain-id 102 \
    --data-dir /var/lib/tola/data \
    --p2p-addr 0.0.0.0:30303 \
    --rpc-addr 127.0.0.1:8545 \
    --validator \
    --validator-key $(cat /etc/tola/consensus.key) \
    --genesis /etc/tola/genesis.json \
    --bootstrap-peer 198.51.100.1:30303
Restart=always
RestartSec=5s
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
```

Enable and start the validator daemon:
```bash
sudo systemctl daemon-reload
sudo systemctl enable tola-node
sudo systemctl start tola-node
sudo journalctl -u tola-node -f
```

---

## 7. Staking & Committee Registration

To be elected into the active committee, a validator must meet the minimum self-stake threshold ($100,000\text{ TOLA}$):

```bash
# Request testnet funds from faucet if needed
tola-cli faucet request --to 0xYOUR_OPERATOR_ADDRESS --faucet-url http://faucet.testnet.tola.network

# Check balance
tola-cli account balance 0xYOUR_OPERATOR_ADDRESS
```

Once funded and registered, the node will be elected into the active committee at the next epoch boundary ($21,600\text{ blocks}$).

---

## 8. Slashing Protection & Best Practices

1. **NEVER run two nodes with the same validator consensus key:**
   Running redundant backup instances with the same consensus key will cause double-voting equivocation, triggering an automatic 10% stake slash and a 30-day jailing penalty.
2. **Use warm standby rather than active-active replication:**
   If operating high-availability infrastructure, use a manual or strictly isolated failover mechanism that guarantees the primary node is fully stopped before activating the standby node.
3. **Monitor node health and metrics:**
   Configure Prometheus to scrape `http://127.0.0.1:8545/metrics` and alert on height stalling or peer count falling below 3.
