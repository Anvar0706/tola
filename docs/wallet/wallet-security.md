# TOLA Network Wallet Security & Cryptographic Isolation Standard

## 1. Threat Model & Design Philosophy

The TOLA Network wallet stack is engineered to withstand hostile environments, malicious dependencies, untrusted networks, and side-channel leakage.

Core design rules:
1. **Zero Secret Leakage**: Private keys and mnemonic seeds must never be exposed via logging, debug printing, object stringification, JSON serialization, or RPC endpoints.
2. **Deterministic Cryptography**: Eliminate randomness-based attacks (bad RNG / biased nonce leakage) by enforcing RFC 6979.
3. **Malleability Resistance**: Enforce strict Low-S signature canonization.
4. **Domain Separation & Anti-Replay**: Prevent cross-network replaying using Chain ID binding in signing digests.
5. **Strict Cryptographic Segregation**: Maintain total separation between user transactions (Secp256k1) and consensus/validator signing (Ed25519).

---

## 2. Memory Protection & Secret Isolation

### Rust (`crates/tola-sdk`)
- `TolaWallet` implements `core::fmt::Debug` and `core::fmt::Display` with automated redaction:
  ```rust
  // Output: TolaWallet([REDACTED: 0x3b45664e195dbacd0035046b511491e09bd3ca8a])
  ```
- No public fields expose raw private key scalars.
- Access requires explicit method calls (`wallet.private_key()`).

### TypeScript (`sdk/typescript`)
- `TolaWallet` encapsulates `#privateKey` and `#mnemonic` as ECMAScript private fields (`#privateKey`).
- Custom `[util.inspect.custom]()` returns `[TolaWallet: tola1...]`.
- `toJSON()` returns only `{ address, rawAddress }`, preventing inadvertent leakage via `JSON.stringify()`.

### Python (`sdk/python`)
- Private scalar stored in `self._private_key` and mnemonic in `self._mnemonic`.
- `__repr__()` returns `<TolaWallet address=tola1...>` and `__str__()` returns `tola1...`.
- `to_dict()` exports only public address fields.

---

## 3. Cryptographic Hardening

### Deterministic Nonces (RFC 6979)
ECDSA signature generation traditionally requires a cryptographically secure random number $k$. If the RNG fails, repeats a nonce, or leaks biased bits, an attacker can extract the private key from two signatures:
$$k = \frac{e_1 - e_2}{s_1 - s_2} \pmod N$$
TOLA Network mandates **RFC 6979** across all SDKs: $k$ is derived deterministically via HMAC-SHA256 from the private key and transaction digest.

### Malleability Immunity (BIP-62 Low-S)
For any valid ECDSA signature $(r, s)$, the tuple $(r, N - s)$ is also a mathematically valid signature for the same public key. To prevent transaction hash malleability attacks:
- Any calculated $s > N/2$ is replaced with $N - s$.
- Recovery parameter $v$ is flipped ($v \oplus 1$).
- Mempools and validators reject any transaction with $s > N/2$.

### Replay Attack Immunity
Transactions are signed over the BLAKE3 digest:
$$\text{Digest} = \text{BLAKE3}(\text{DOMAIN\_TAG} \parallel \text{ChainID} \parallel \text{Epoch} \parallel \text{BLAKE3}(\text{PayloadBytes}))$$
A transaction signed for Testnet (Chain ID 102) is invalid on Mainnet (Chain ID 9999).

---

## 4. Cryptographic Separation of Concerns

| Domain | Algorithm | Curve / Primitives | Key Role |
| :--- | :--- | :--- | :--- |
| **User Accounts** | ECDSA (RFC 6979) | Secp256k1, Keccak-256 | Balance transfers, smart contracts, dApps |
| **Consensus / Validators** | EdDSA | Ed25519, BLAKE3 | Block proposal, prevote, precommit |

User wallets are strictly restricted to Secp256k1 derivation and cannot generate validator consensus keys, guaranteeing that compromise of a client-side wallet cannot impact validator consensus state.
