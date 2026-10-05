# SUBSYSTEM: Non-Custodial Wallet Architecture

## Objective
Design the non-custodial wallet reference architecture, key management protocols, transaction confirmation UX, and hardware security integrations for TOLA Network.

## Why It Exists
The wallet is the user's primary interface to the blockchain. A secure, intuitive wallet prevents asset theft, protects user privacy, and enables frictionless dApp and messenger interactions.

## Dependencies
- `prompts/03-cryptography.md`
- `prompts/05-addresses.md`
- `prompts/06-transaction-model.md`
- `prompts/27-sdk.md`

## Non-Goals
- Centralized custodial key storage or custodial wallet backends.

## Requirements
- Security: Private keys never leave the user's client device unencrypted.
- Key Generation: BIP-39 12/24-word seed phrases with BIP-44 derivation paths.
- Hardware Enclave Integration: iOS Secure Enclave, Android KeyStore, WebAuthn / Passkeys, and Ledger hardware wallet support.
- Clear-Signing Modals: Every signature request must display human-readable recipient, asset type, exact amount, network fee, and contract method name.
- Multi-Account & Multi-Network Support: Seamless switching between Mainnet, Testnet, and local Devnet.

## Architecture
```
User Interface -> Clear-Signing Modal -> Biometric / PIN Auth
               -> Secure Enclave (Decryption) -> Transaction Signing
               -> Broadcast via TOLA SDK RPC
```

## Interfaces
```rust
pub trait KeyStore {
    fn generate_mnemonic(&self) -> Result<String, WalletError>;
    fn derive_keypair(&self, mnemonic: &str, account_index: u32) -> Result<KeyPair, WalletError>;
    fn sign_transaction(&self, keypair: &KeyPair, tx: &Transaction) -> Result<Vec<u8>, WalletError>;
}
```

## Data Structures
```rust
pub struct WalletAccount {
    pub index: u32,
    pub address: Address,
    pub public_key: [u8; 32],
    pub label: String,
}
```

## Security Requirements
- Anti-phishing clear signing: Plaintext hex data must be flagged with explicit warnings unless decoded via verified contract ABI.
- Memory protection: Seed phrases and private keys zeroed out immediately following signature creation.

## Failure Modes
- User loss of seed phrase (mitigated by prominent backup verification onboarding flows).
- Blind signing exploits through deceptive dApp calldata.

## Testing Requirements
- Security penetration testing on keystore encryption and export mechanisms.
- Compatibility testing with standard hardware wallet devices.

## Performance Requirements
- Local transaction signing latency $< 50\text{ ms}$.

## Migration Requirements
- Standardized keystore export/import format (JSON V3 keystore format).

## Acceptance Criteria
- Wallet architecture and security guidelines documented in `docs/wallet/wallet-architecture.md`.
- Key derivation and clear-signing specifications ratified.

## Open Questions
- Should account abstraction (ERC-4337 style social recovery) be built into the wallet core natively?

## Implementation Tasks
1. Define BIP-44 derivation path hierarchy for TOLA.
2. Specify clear-signing modal parameters.
3. Design encrypted keystore JSON schema.

## Completion Criteria
Wallet architecture document approved and aligned with TOLA Connect and Messenger specifications.
