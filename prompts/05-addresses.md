# SUBSYSTEM: Address Specification & Encoding

## Objective
Design and implement the canonical address format for TOLA Network, providing human-readable, error-detecting, versioned, and network-aware addresses.

## Why It Exists
Raw hexadecimal addresses are prone to silent user typo errors, lack network scope awareness (leading to testnet funds sent to mainnet), and produce poor QR code density.

## Dependencies
- `prompts/03-cryptography.md`
- `prompts/04-account-model.md`

## Non-Goals
- Variable-length domain name resolution (handled by `prompts/43-username-system.md`).

## Requirements
- Encoding standard: **Bech32m** (BIP-350) with BCH error-correcting codes.
- Human-Readable Prefixes (HRP):
  - Mainnet: `tola` (e.g. `tola1q09x7...`)
  - Testnet: `ttola` (e.g. `ttola1q09x7...`)
  - Devnet: `dtola` (e.g. `dtola1q09x7...`)
- Address Payload:
  - 1-byte version identifier (supports future cryptographic curve upgrades).
  - 20-byte payload (hash digest of public key: $\text{RIPEMD160}(\text{BLAKE3}(\text{PublicKey}))$ or $\text{Keccak256}(\text{PublicKey})[12..32]$).
- Dual Representation: Support lossless conversion between Bech32m and 20-byte EVM hex address.

## Architecture
```
Public Key (Secp256k1 / Ed25519)
               │ BLAKE3 / Keccak-256
      20-Byte Address Digest
               │ Prepend Version Byte (0x01)
               │ Encode Bech32m with HRP ("tola")
   Final Address String: tola1w4d8q9...
```

## Interfaces
```rust
pub struct Address([u8; 20]);

impl Address {
    pub fn from_pubkey(pk: &PublicKey) -> Self;
    pub fn to_bech32m(&self, hrp: &str) -> Result<String, AddressError>;
    pub fn from_bech32m(encoded: &str) -> Result<(Self, NetworkType), AddressError>;
    pub fn to_hex(&self) -> String;
}
```

## Data Structures
```rust
pub enum NetworkType {
    Mainnet,
    Testnet,
    Devnet,
}

pub struct AddressEnvelope {
    pub network: NetworkType,
    pub version: u8,
    pub raw_bytes: [u8; 20],
}
```

## Security Requirements
- Bech32m provides guaranteed detection of up to 4 character substitution errors and any burst errors up to length 6.
- Strict network validation: Mainnet node rejects transactions addressed to `ttola1...`.

## Failure Modes
- Invalid HRP causing address rejection.
- Checksum mismatch on user input.

## Testing Requirements
- Unit tests with 1,000 randomly generated addresses.
- Boundary tests for character substitutions, case variations, and length variations.
- Round-trip conversions: $\text{Bytes} \to \text{Bech32m} \to \text{Bytes}$.

## Performance Requirements
- Bech32m encode/decode throughput $> 100,000$ operations/sec.

## Migration Requirements
- Address version byte allows migrating from 20-byte hashes to 32-byte hashes in future hard forks.

## Acceptance Criteria
- Address format specification documented in `docs/protocol/addresses.md`.
- ADR-0006 updated and ratified.

## Open Questions
- Should the internal address payload be 20 bytes (EVM standard) or 32 bytes (Ed25519 standard)?

## Implementation Tasks
1. Implement Bech32m encoder/decoder crate wrapper.
2. Author test vectors across Mainnet, Testnet, and Devnet.
3. Integrate into account serialization pipeline.

## Completion Criteria
Address specification verified, test vectors published, and encoder/decoder library validated.
