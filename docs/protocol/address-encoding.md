# TOLA Network: Address Encoding & Bech32m Specification

## 1. Overview & Architectural Motivation
Compliant with **ADR-0006**, TOLA Network establishes **Bech32m (BIP-350)** as the primary human-readable address format across all user interfaces, wallets, CLI tooling, and SDKs.

### Why Not Raw Hex?
- Raw hexadecimal strings (e.g., `0x71C...`) lack native error detection, causing permanent asset loss if a single character is mistyped.
- EIP-55 mixed-case checksumming is fragile, easily corrupted by case-flattening databases or URL decoders, and cannot distinguish networks (e.g. sending mainnet funds to a testnet address).

### Why Bech32m?
- **Network-Scoping:** Clear human-readable prefixes (`tola`, `ttola`, `dtola`) prevent accidental cross-network misdirection.
- **BCH Error Correction:** Mathematical polynomial detects up to 4 arbitrary character substitutions and all burst errors of length up to 8 characters.
- **QR Optimization:** Uses alphanumeric character set optimized for compact high-density QR code generation.
- **BIP-350 Resilience:** Avoids the single-character substitution vulnerability present in original Bech32 by employing the `0x2bc830a3` constant.

---

## 2. Canonical Address Parameters

| Network | Human-Readable Part (HRP) | Sample Address Length | Example Prefix |
| :--- | :--- | :--- | :--- |
| **Mainnet** | `tola` | 43 characters | `tola1...` |
| **Testnet** | `ttola` | 44 characters | `ttola1...` |
| **Devnet** | `dtola` | 44 characters | `dtola1...` |

---

## 3. Mathematical Encoding Algorithm

### 3.1 Step 1: Payload Derivation
The canonical 20-byte account address is derived from the uncompressed Secp256k1 public key:
$$\text{RawAddress} = \text{Keccak256}(\text{PubKey}_{\text{uncompressed}}[1..65])[12..32]$$

### 3.2 Step 2: 8-Bit to 5-Bit Conversion
The 20 raw bytes ($160$ bits) are re-grouped into 5-bit words without remainder:
$$160 \div 5 = 32 \text{ words}$$

### 3.3 Step 3: Polymod BCH Checksum Computation
The Human-Readable Part (HRP) is expanded:
$$\text{hrp\_expand}(H) = [c \gg 5 \mid c \in H] \parallel [0] \parallel [c \ \& \ 31 \mid c \in H]$$
The checksum is calculated over the concatenation of the expanded HRP, the 32 5-bit payload words, and 6 zero padding words:
$$\text{checksum} = \text{polymod}(\text{expanded\_hrp} \parallel \text{data} \parallel [0; 6]) \oplus \text{0x2bc830a3}$$
The 6 resulting 5-bit integers are mapped to the Bech32 character set:
`qpzry9x8gf2tvdw0s3jn54khce6mua7l`

---

## 4. Dual-Format Compatibility

While Bech32m is the canonical display representation, the underlying internal representation in memory and storage is strictly `[u8; 20]`. 

For Ethereum JSON-RPC tooling (`eth_getBalance`, `eth_sendRawTransaction`, MetaMask), the node transparently accepts and outputs standard lowercase `0x` hexadecimal strings via `Address::to_hex()` and `Address::from_hex()`.
