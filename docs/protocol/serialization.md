# TOLA Network: Canonical Serialization & Codec Specification

## 1. Overview & Architectural Policy
TOLA Network enforces a strict **Two-Domain Serialization Architecture** to guarantee absolute binary determinism across internal validator nodes while providing transparent JSON-RPC and transaction compatibility with standard Ethereum tools (Foundry, Hardhat, MetaMask).

---

## 2. Domain A: Internal Consensus & Storage (Borsh)
All consensus messages, block headers, transaction envelopes, Quorum Certificates, and state records persisted in MDBX are encoded using **Borsh (Binary Object Representation Serializer for Hashing)**.

### Strict Determinism Rules:
1. **Little-Endian Fixed Integers:** All integer types (`u16`, `u32`, `u64`, `u128`) are encoded as little-endian byte arrays without padding.
2. **Lengths as Little-Endian `u32`:** All variable-length byte arrays and vectors are prefixed with a 4-byte `u32` length field.
3. **No Floats:** Floating-point representations (`f32`, `f64`) are strictly prohibited in canonical messages.
4. **Field Ordering:** Fields are serialized in the exact order of their canonical Rust struct definition.
5. **Zero Trailing Bytes:** Any decoding operation encountering unconsumed trailing bytes fails immediately with `CodecError::TrailingBytes`.
6. **Recursion Depth Bounds:** Recursive structures are restricted to a maximum depth of 32 frames to prevent stack exhaustion attacks.

---

## 3. Domain B: External JSON-RPC Transactions (RLP / EIP-2718)
- External transactions submitted via `eth_sendRawTransaction` arrive as standard EIP-1559 or legacy RLP-encoded hex strings.
- The `tola-rpc` boundary decodes the RLP envelope, verifies signatures, and converts the parameters into canonical `tola-types::Transaction` instances.
- Internal consensus validators never process raw RLP bytes; only canonical Borsh transaction envelopes circulate across P2P GossipSub and BFT blocks.

---

## 4. Canonical Byte Schemas

### 4.1 Address (20 Bytes)
```
[0..20]: Raw 20-byte address hash
```

### 4.2 Hash (32 Bytes)
```
[0..32]: Raw 32-byte BLAKE3/Keccak digest
```

### 4.3 Transaction Envelope (Borsh)
```
[0..8]   : chain_id (u64, LE)
[8..16]  : nonce (u64, LE)
[16..36] : from (Address, 20 bytes)
[36..37] : to_is_some (u8 flag: 0 = None, 1 = Some)
[37..57] : to (Address, 20 bytes, if present)
[..]     : value (u128, 16 bytes, LE)
[..]     : gas_limit (u64, 8 bytes, LE)
[..]     : max_fee_per_gas (u128, 16 bytes, LE)
[..]     : max_priority_fee_per_gas (u128, 16 bytes, LE)
[..]     : valid_until_block (u64, 8 bytes, LE)
[..]     : data_length (u32, 4 bytes, LE)
[..]     : data_bytes (N bytes)
[..]     : memo (optional: u32 LE len + UTF-8 bytes, if present and non-empty, max 256 bytes)
[..]     : signature (Signature enum: 1 byte tag + 64/65 bytes payload)
```

