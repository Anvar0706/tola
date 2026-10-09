# ADR-0013: TOLA Wallet BIP-44 Derivation Path & Coin Type Standard

**Status:** PROPOSED & RATIFIED FOR TESTNET / PRE-MAINNET  
**Date:** 2026-10-07  
**Deciders:** TOLA Network Core Architecture, Security & Ecosystem Council  
**Applies To:** `tola-crypto`, `tola-sdk`, `tola-cli`, `@tola/sdk` (TypeScript), `tola` (Python)  

---

## 1. Context & Problem Statement

To provide users with non-custodial wallet backup and deterministic recovery, TOLA Network requires a standardized hierarchical deterministic (HD) wallet derivation standard complying with BIP-32 and BIP-44.

Under BIP-44:
$$\mathbf{m / 44' / \text{coin\_type}' / \text{account}' / \text{change} / \text{address\_index}}$$

Where:
- `44'`: Purpose (hardened).
- `coin_type'`: Registered coin type from SatoshiLabs SLIP-0044.
- `account'`: Hardened account index ($0', 1', \dots$).
- `change`: Sub-account change index ($0$ for external receiving, $1$ for internal change).
- `address_index`: Sequential integer ($0, 1, 2, \dots$).

TOLA Network does not yet have an internationally registered entry in the public SatoshiLabs SLIP-0044 repository. Before finalizing production wallet recovery, a formal architecture decision is required to determine the coin type, network differentiation, backward compatibility, hardware wallet interoperability, and long-term ecosystem migration strategy.

---

## 2. Evaluation of Alternatives

### Alternative A: Ethereum Coin Type `60'` (`m/44'/60'/0'/0/0`)
- **Pros:** Maximum compatibility with generic EVM hardware wallets (Ledger Ethereum App, Trezor, MetaMask, WalletConnect).
- **Cons:** Address collisions with Ethereum accounts; users might accidentally use the same address on Ethereum and TOLA, causing confusion or cross-chain user error. Deviates from TOLA's independent L1 identity (ADR-0002, ADR-0005).

### Alternative B: General Testnet Coin Type `1'` (`m/44'/1'/0'/0/0`)
- **Pros:** Standard SLIP-0044 assignment for all non-Bitcoin testnets.
- **Cons:** Does not distinguish TOLA from other testnets; leaves Mainnet path undefined.

### Alternative C: Chain-ID Mirroring `9999'` (`m/44'/9999'/0'/0/0`) [PROPOSED]
- **Pros:**
  - Exactly mirrors the canonical Mainnet Chain ID `9999` (as harmonized in Pre-Mainnet Blocker Remediation).
  - Clean separation from Ethereum (`60'`) and Bitcoin (`0'`).
  - No collision with existing high-volume blockchains in SLIP-0044 (where 9999 is currently unallocated).
  - Can be registered with SatoshiLabs prior to public Mainnet genesis.
  - Testnet can use `102'` (`m/44'/102'/0'/0/0`) or fallback to `1'`.
- **Cons:** Not yet merged into upstream SatoshiLabs SLIP-0044 repository; requires explicit derivation path configuration on third-party hardware wallet apps until custom TOLA app is distributed.

---

## 3. Decision

1. **Proposed Primary Coin Type:**
   - **TOLA Mainnet:** Coin Type **`9999'`** (`0x8000270F`).
     $$\mathbf{m / 44' / 9999' / 0' / 0 / 0}$$
   - **TOLA Testnet:** Coin Type **`102'`** (`0x80000066`) matching Testnet Chain ID `102`, with optional fallback support for SLIP-0044 general testnet **`1'`** (`0x80000001`).
     $$\mathbf{m / 44' / 102' / 0' / 0 / 0} \quad \text{or} \quad \mathbf{m / 44' / 1' / 0' / 0 / 0}$$

2. **Status Classification:**
   - The path `m/44'/9999'/0'/0/0` is classified as **PROPOSED (INTERNAL CANONICAL SPECIFICATION)** until the upstream SLIP-0044 pull request is ratified by SatoshiLabs.
   - All internal tools (`tola-crypto`, `tola-sdk`, `tola-cli`) will use `9999'` for Mainnet and `102'` for Testnet as default, while allowing programmatic customization of the derivation path.

3. **Key Type & Curve:**
   - Derivation utilizes the **Secp256k1** curve via standard BIP-32 HMAC-SHA512 CKD (Child Key Derivation).
   - The derived private key is used with the canonical TOLA address algorithm:
     $$\text{Address} = \text{Keccak-256}(\text{PubKey}_{\text{uncompressed}}[1\dots65])[12\dots32]$$
   - This ensures 100% backward compatibility with all existing TOLA nodes, transactions, mempools, and smart contracts.

4. **Validator Key Boundary:**
   - Validator Ed25519 keys remain outside the user wallet derivation boundary during Phase 1. Consensus validator identities continue to use dedicated key management (`tola-cli ceremony keygen`).

---

## 4. Consequences & Migration Path

- **Backward Compatibility:** Completely preserved. Existing wallets imported via raw private key bytes continue to work identically. Wallets generated from mnemonic produce valid, standard TOLA accounts.
- **Hardware Wallet Compatibility:**
  - Ledger / Trezor integration will support custom derivation path `m/44'/9999'/0'/0/0` under the TOLA app.
  - While waiting for Ledger app store approval, users can optionally use standard EVM derivation paths (`m/44'/60'/0'/0/0`) if explicitly configured.
- **SLIP-0044 Registration:**
  - The TOLA Foundation will submit a pull request to `satoshilabs/slips` registering Coin Type `9999` with symbol `TOLA` and name `TOLA Network`.
