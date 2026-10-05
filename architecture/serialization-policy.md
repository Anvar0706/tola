# TOLA Network: Canonical Serialization Policy

## 1. Rationale & Analysis of Encodings
A blockchain must maintain absolute binary determinism. If two compliant nodes serialize the identical transaction or block into differing byte arrays, hash digests will diverge, causing an immediate catastrophic consensus split.

During Phase 1, both Borsh and RLP were referenced. This policy establishes the definitive protocol-wide serialization standard and dictates where each encoding is legally permitted.

---

## 2. Comparative Evaluation of Candidate Codecs

| Feature | Borsh | RLP (Ethereum) | SSZ (Ethereum Beacon) | Protobuf | Custom Binary |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Strict Determinism** | **100% Guaranteed** | Ambiguous on integers | 100% Guaranteed | No (Field map order) | Depends on code |
| **Zero-Copy Deserialization**| Supported | Supported | Supported | Partial | Supported |
| **Decoding Safety** | **Checked bounds** | Stack overflow risks | Checked bounds | Moderate | Manual risk |
| **Overhead / Padding** | **Zero byte padding** | Variable prefix | Uniform padding | Field tags | Zero padding |
| **EVM Tooling Compatibility** | Low | **Native (Web3/Ethers)** | Low | Low | Low |
| **Primary Domain** | **L1 Internal & Consensus**| **External EVM RPC** | Consensus Merkleization| Microservices | Niche |

---

## 3. The Two-Domain Serialization Policy

To achieve peak internal performance and determinism while maintaining seamless compatibility with the global Ethereum developer ecosystem, TOLA Network establishes a **Strict Two-Domain Serialization Boundary**:

```
[External Web3 / RPC World]                     [Internal Consensus & Storage World]
Solidity / Ethers.js / MetaMask                 Validators / P2P Swarm / MDBX State
             │                                                  │
             ▼                                                  ▼
     EIP-2718 / RLP Wire Format                         Borsh Canonical Binary
(eth_sendRawTransaction / eth_call)             (P2P Gossip, Headers, QCs, JMT Nodes)
             │                                                  │
             └───────────────────────┬──────────────────────────┘
                                     ▼
                            tola-codec Envelope
                     (Converts RLP Tx -> Borsh Tx)
```

### Domain A: Canonical Protocol Wire & State (Strict Borsh)
All consensus messages, P2P gossip payloads, block headers, block bodies, Quorum Certificates, and persistent database entries MUST use **Borsh (Binary Object Representation Serializer for Hashing)**.
- **Determinism Rules:**
  - Little-endian encoding for all integers (`u16`, `u32`, `u64`, `u128`).
  - No floating-point serialization permitted.
  - Variable-length vectors are prefixed with an explicit `u32` length byte.
  - Struct fields are serialized strictly in the order of their Rust struct definition.
  - Zero trailing bytes allowed: Any deserializer encountering unconsumed trailing bytes must return `CodecError::TrailingBytes`.

### Domain B: External JSON-RPC Compatibility Layer (RLP / EIP-2718)
- External transactions submitted via `eth_sendRawTransaction` arrive as standard legacy or EIP-1559/EIP-2718 RLP-encoded hex strings.
- The `tola-rpc` boundary decodes the RLP envelope, extracts sender, nonce, gas parameters, payload, and signature, and instantiates the canonical `tola-types::Transaction` structure.
- Internally, all node subsystems (mempool, block engine, consensus) handle only the canonical Borsh representation.
