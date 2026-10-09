# TOLA Network: Persistent Jellyfish Merkle Tree (JMT) Specification

## 1. Overview & Architectural Motivation
Compliant with **ADR-0010**, TOLA Network adopts the **Jellyfish Merkle Tree (JMT)** with **BLAKE3 hashing** as its authenticated state commitment structure.

### Why JMT Over SMT and Ethereum MPT?
- **Naive Binary SMT:** Requires up to 256 hash operations per leaf update, creating an unacceptable CPU bottleneck ($> 80\text{ ms}$ per block).
- **Ethereum Hexary MPT:** Suffers from severe RLP serialization overhead, path expansion, and high disk amplification.
- **Jellyfish Merkle Tree (JMT):** Utilizes **16-ary radix path compression**. Paths branch by 4-bit nibbles, reducing update operations to only $8 - 14$ hashes per leaf while generating compact cryptographic proofs ($< 1.5\text{ KB}$).

---

## 2. Key Space & Nibble Path Traversal

1. **Key Formulation:**
   The JMT key is the 32-byte BLAKE3 digest of the account address:
   $$\text{Key} = \text{BLAKE3}(\text{Address})$$
   This guarantees uniform pseudorandom distribution across the tree, rendering deep path collisions cryptographically impossible ($< 2^{-128}$).
2. **Nibble Traversal:**
   The 32-byte key is decomposed into 64 4-bit nibbles:
   $$\text{Nibble}_i = \begin{cases} (\text{Key}[i/2] \gg 4) \ \& \ \text{0x0F} & \text{if } i \text{ is even} \\ \text{Key}[i/2] \ \& \ \text{0x0F} & \text{if } i \text{ is odd} \end{cases}$$

---

## 3. Node Specifications & Domain Separation

### 3.1 Leaf Node
Represents an active account.
- **Payload:** `address: Address`, `account_state: AccountState`.
- **Domain Separator:** `b"TOLA-JMT-LF-v1--"` (16 bytes).
- **Leaf Commitment:**
  $$\text{LeafHash} = \text{BLAKE3}(\text{DOMAIN\_LEAF} \parallel \text{Address} \parallel \text{AccountState.to\_canonical\_bytes}())$$

### 3.2 Internal Node
Branches up to 16 child subtrees.
- **Payload:** `children: [Option<Hash>; 16]`.
- **Presence Bitmask:** 16-bit integer $M$ where bit $k$ is set iff `children[k]` is present.
- **Domain Separator:** `b"TOLA-JMT-INT-v1-"` (16 bytes).
- **Internal Commitment:**
  $$\text{InternalHash} = \text{BLAKE3}(\text{DOMAIN\_INT} \parallel M.\text{to\_le\_bytes}() \parallel \text{ChildHash}_{k_0} \parallel \text{ChildHash}_{k_1} \dots)$$

---

## 4. Multi-Version State Commitment & Deduplication

Nodes in `Table::TreeNodes` are content-addressed by their 32-byte hash:
$$\text{Key} = \text{NodeHash} \implies \text{Value} = \text{NodeBytes}$$

### Deduplication Benefit:
Because node hashes depend strictly on the cryptographic content of their subtrees:
- When a block updates 5,000 accounts, only the modified paths ($8 - 14$ nodes per update) are written to disk.
- Millions of unmodified subtrees from prior blocks remain shared and referenced by their immutable 32-byte hashes across blocks.
- Historical state roots remain verifiable indefinitely without duplicating unchanged accounts.
