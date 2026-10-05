# ADR-0005: Decoupled Ecosystem & Messenger Architecture

## Title
Decoupled Ecosystem and Messenger Architecture

## Status
ACCEPTED

## Context
TOLA Network envisions a native ecosystem including social communication, mini-apps, and digital collectibles. However, tying the blockchain protocol directly to an external application stack threatens decentralization and protocol longevity.

## Problem
How to integrate social and messaging capabilities without compromising blockchain independence.

## Options
1. *Tightly-Coupled Monolith*: Store chat data, contacts, and messaging servers on-chain or make validators run messenger infrastructure.
2. *Strict Protocol Decoupling*: TOLA Layer-1 remains an autonomous public ledger. The messenger is built as a first-class client using the standard TOLA SDK and TOLA Connect protocol.

## Decision
Select Option 2: **Strict Protocol Decoupling**. The blockchain operates independently. All chat, media, and private contact graphs remain off-chain.

## Why
Ensures that if the messenger client is compromised, banned, or refactored, the TOLA blockchain continues to function seamlessly.

## Consequences
Requires robust client-side integration protocols (TOLA Connect) and cryptographic identity binding mechanisms.

## Security Impact
Protects user privacy by keeping personal communication data completely off the immutable public ledger.

## Scalability Impact
Prevents massive state bloat from multimedia and social messaging traffic.

## Migration Impact
None (Genesis).
