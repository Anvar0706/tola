# TOLA Network: Peer-to-Peer Networking & Wire Protocol Specification

## 1. Overview & Architectural Motivation
The **TOLA P2P Network** (`tola-p2p`) provides resilient, low-latency, and Sybil-resistant communication across decentralized validator nodes and non-validating full nodes.
In accordance with **prompts/12-p2p.md** and **architecture/networking-specification-v0.1.md**, the networking layer solves four fundamental distributed network threats:
1. **Network Partition & Eclipse Attacks:** Malicious entities attempting to monopolize peer connection slots from cloud subnets.
2. **Gossip Amplification & Broadcast Storms:** Duplicate relaying leading to exponential bandwidth exhaustion.
3. **Poisoned Block & Transaction Propagation:** Malformed payloads degrading CPU verification queues.
4. **Wire Tampering & Silent Corruption:** Frame corruption over degraded transit links.

---

## 2. Wire Protocol Framing

Every TCP and QUIC payload transmitted between TOLA nodes conforms to a deterministic 44-byte frame header followed by canonical payload bytes:

```
+---------------------------------------------------------------------------------------+
| Magic (4B) | Version (2B) | NetId (1B) | Type (1B) | Length (4B) | BLAKE3 Checksum (32B) |
+---------------------------------------------------------------------------------------+
| Payload Bytes (Length bytes) ...                                                      |
+---------------------------------------------------------------------------------------+
```

### 2.1 Frame Header Fields
- `Magic (4 Bytes)`: ASCII `0x54, 0x4F, 0x4C, 0x41` (`"TOLA"`). Rejects alien connections instantly.
- `Version (2 Bytes)`: Protocol version (Current: `1` Little-Endian).
- `NetworkId (1 Byte)`: Network isolation tag (`0x01` Mainnet, `0x02` Testnet, `0x03` Devnet). Cross-network connections are rejected.
- `MessageType (1 Byte)`: Message discriminator (`0x01` Handshake, `0x02` HandshakeAck, `0x03` Ping, `0x04` Pong, `0x10` GossipTx, `0x11` GossipBlock, `0x12` GossipVote, `0x20` GetBlocks, `0x21` Blocks, `0x30` Disconnect).
- `Length (4 Bytes LE)`: Length of payload in bytes ($L \le 4\text{ MB} + 64\text{ KB}$).
- `Checksum (32 Bytes)`: $\text{BLAKE3}(\text{PayloadBytes})$. If checksum mismatches, frame is dropped and peer is penalized.

---

## 3. GossipSub v1.1 Topic Hierarchy & Message Validation

TOLA partitions gossip traffic across three dedicated topics:

| Topic Identifier | Max Payload | Propagation Content | Inbound Validation Policy |
| :--- | :--- | :--- | :--- |
| `/tola/tx/1.0.0` | 128 KB | Unconfirmed Transactions | Stateless validity, signature recovery, minimum base fee solvency |
| `/tola/block/1.0.0` | 4 MB | Proposed Blocks & Commit QCs | Header height monotonicity, proposer signature, transactions root |
| `/tola/vote/1.0.0` | 64 KB | Consensus Prevote & Precommit | Active validator committee check, epoch/round freshness, Ed25519/BLS signature |

### 3.1 Duplicate Message Suppression
To prevent infinite propagation loops, every gossip message is indexed by a deterministic cryptographic message identifier:
$$\text{MessageId} = \text{BLAKE3}(\text{TopicBytes} \parallel \text{PayloadBytes})$$
Nodes maintain an in-memory `MessageSeenCache` with a bounded FIFO/LRU window (default 50,000 entries). Received messages present in the cache are discarded before relaying.

---

## 4. Peer Reputation & Behavior Scoring

Every connected peer is continuously tracked by the `PeerScoreManager`. The composite score $S$ is calculated dynamically:

$$S_{\text{peer}} = \min(T \cdot W_{\text{mesh}}, W_{\text{max\_mesh}}) + (D \cdot W_{\text{first}}) - (I \cdot W_{\text{invalid}}) - (R \cdot W_{\text{rate}})$$

### 4.1 Scoring Parameters & Weights
- $T$ (Time in Mesh): Weight $W_{\text{mesh}} = +0.1/\text{sec}$, capped at $+10.0$.
- $D$ (First Deliveries): Weight $W_{\text{first}} = +1.0$ per unique valid message delivered first.
- $I$ (Invalid Messages): Weight $W_{\text{invalid}} = -50.0$ per malformed frame or invalid cryptographic signature.
- $R$ (Rate Violations): Weight $W_{\text{rate}} = -20.0$ per rate limit violation.
- **Decay Half-Life:** Scores decay by factor $\gamma = 0.90$ per epoch.

### 4.2 Automated Sanctions & Penalties
- **Disconnect Threshold ($S \le -50.0$):** Connection terminated with `NetworkError::ScoreTooLow`.
- **Ban Threshold ($S \le -100.0$):** Peer ID and originating IP subnet added to blacklist for 24 hours (`NetworkError::PeerBanned`).

---

## 5. Anti-Eclipse Subnet Partitioning

To thwart eclipse attacks where a single adversary spins up hundreds of VPS instances within the same hosting datacenter to monopolize a victim's connection slots, the `SubnetLimiter` partitions connections:
- **IPv4 Quota:** Maximum 2 concurrent connections per `/24` subnet.
- **IPv6 Quota:** Maximum 2 concurrent connections per `/48` subnet.
- Connections exceeding the quota are rejected with `NetworkError::SubnetLimitExceeded`.

---

## 6. Transport Layer Roadmap

1. **Phase 7 Baseline:** Portable wire protocol with BLAKE3 checksums, GossipSub topic routing, peer scoring, and subnet limits.
2. **Phase 12 Node Daemon Integration:** Dual transport:
   - **QUIC (RFC 9000) / TLS 1.3:** Primary high-throughput transport for consensus votes and block streaming.
   - **TCP / Noise Protocol (XX handshake):** Automatic fallback for corporate firewalls and restrictive UDP environments.
