# TOLA Network: Networking Specification v0.1

## 1. Overlay Network Architecture

- **Foundation:** `rust-libp2p` v0.53+.
- **Transport:** TCP and QUIC with **Noise Protocol (XX handshake)** end-to-end encryption.
- **Peer Discovery:** Kademlia Distributed Hash Table (DHT) with bootstrap bootnode fallbacks.
- **Message Propagation:** GossipSub v1.1 with explicit topic scoring and mesh maintenance.

---

## 2. GossipSub Topic Hierarchy

| Topic Name | Wire Encoding | Max Message Size | Validation Rule |
| :--- | :--- | :--- | :--- |
| `/tola/tx/1.0.0` | Borsh / RLP | 128 KB | Stateless validation + balance check |
| `/tola/block/1.0.0` | Borsh | 4 MB | Proposer signature + header check |
| `/tola/vote/1.0.0` | Borsh | 256 Bytes | Active validator signature verification |

---

## 3. Peer Scoring & Anti-Sybil Parameters

Nodes monitor peer behavior using continuous score decay:

$$S_{\text{peer}} = W_{\text{mesh}} \cdot S_{\text{time}} + W_{\text{first}} \cdot S_{\text{deliveries}} - W_{\text{invalid}} \cdot S_{\text{bad\_msgs}}$$

- **Penalty Threshold ($S < -50$):** Peer is immediately disconnected.
- **Ban Threshold ($S < -100$):** Peer IP subnet is blacklisted for 24 hours.
