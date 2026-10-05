# ADR-0006: Account and Address Format Specification

## Title
Versioned Bech32m Checksummed Address Format

## Status
ACCEPTED

## Context
Raw hex addresses (e.g. `0x71C...`) lack native human error detection and network identification, leading to cross-network asset loss (e.g., sending testnet tokens to mainnet addresses).

## Problem
Selecting a robust, human-safe, machine-safe address encoding format.

## Options
1. *Unchecksummed Raw Hex*: Vulnerable to single-character typos.
2. *EIP-55 Mixed-Case Hex*: Better, but error-detection is limited and case-sensitive.
3. *Bech32m Encoding*: Human-readable prefix (`tola` / `ttola`), strong BCH checksum error-detection, case-insensitive, versioned, QR-code friendly.

## Decision
Adopt **Bech32m** address encoding with human-readable prefixes:
- Mainnet: `tola1...`
- Testnet: `ttola1...`
- Devnet: `dtola1...`

## Why
Provides the highest resilience against accidental typos, clear network scoping, and optimal QR-code density.

## Consequences
Requires encoding/decoding layers in SDKs and RPC adapters to convert between internal 20/32-byte hashes and Bech32m strings.

## Security Impact
Virtually eliminates accidental misdirection of funds due to typo errors.

## Scalability Impact
Negligible computational overhead.

## Migration Impact
None (Genesis).
