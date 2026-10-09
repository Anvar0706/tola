//! Canonical binary serialization codec for TOLA Network.
//!
//! Enforces strict binary determinism, little-endian integer encoding,
//! checked bounds, and zero trailing bytes.
//!
//! Complies with ADR-0006, ADR-0010, and Serialization Policy v0.1.

pub mod rlp;

use tola_types::*;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum CodecError {
    UnexpectedEof,
    TrailingBytes,
    InvalidEncoding(String),
}

impl core::fmt::Display for CodecError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CodecError::UnexpectedEof => write!(f, "Unexpected end of input stream"),
            CodecError::TrailingBytes => {
                write!(f, "Unconsumed trailing bytes in canonical payload")
            }
            CodecError::InvalidEncoding(s) => write!(f, "Invalid canonical encoding: {s}"),
        }
    }
}

impl std::error::Error for CodecError {}

pub trait CanonicalSerialize {
    fn serialize_canonical(&self, buf: &mut Vec<u8>);
    fn to_canonical_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.serialize_canonical(&mut buf);
        buf
    }
}

pub trait CanonicalDeserialize: Sized {
    fn deserialize_canonical(bytes: &[u8]) -> Result<Self, CodecError> {
        let mut reader = ByteReader::new(bytes);
        let val = Self::read_canonical(&mut reader)?;
        reader.ensure_consumed()?;
        Ok(val)
    }

    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError>;
}

// ----------------------------------------------------------------------------
// Zero-Copy Streaming Byte Reader
// ----------------------------------------------------------------------------

pub struct ByteReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    pub fn peek_u8(&self) -> Option<u8> {
        if self.pos < self.buf.len() {
            Some(self.buf[self.pos])
        } else {
            None
        }
    }

    pub fn peek_slice(&self, len: usize) -> Option<&'a [u8]> {
        if self.pos + len <= self.buf.len() {
            Some(&self.buf[self.pos..self.pos + len])
        } else {
            None
        }
    }

    pub fn ensure_consumed(&self) -> Result<(), CodecError> {
        if self.pos == self.buf.len() {
            Ok(())
        } else {
            Err(CodecError::TrailingBytes)
        }
    }

    pub fn read_u8(&mut self) -> Result<u8, CodecError> {
        if self.pos + 1 > self.buf.len() {
            return Err(CodecError::UnexpectedEof);
        }
        let b = self.buf[self.pos];
        self.pos += 1;
        Ok(b)
    }

    pub fn read_u16_le(&mut self) -> Result<u16, CodecError> {
        let slice = self.read_exact(2)?;
        let mut arr = [0u8; 2];
        arr.copy_from_slice(slice);
        Ok(u16::from_le_bytes(arr))
    }

    pub fn read_u32_le(&mut self) -> Result<u32, CodecError> {
        let slice = self.read_exact(4)?;
        let mut arr = [0u8; 4];
        arr.copy_from_slice(slice);
        Ok(u32::from_le_bytes(arr))
    }

    pub fn read_u64_le(&mut self) -> Result<u64, CodecError> {
        let slice = self.read_exact(8)?;
        let mut arr = [0u8; 8];
        arr.copy_from_slice(slice);
        Ok(u64::from_le_bytes(arr))
    }

    pub fn read_u128_le(&mut self) -> Result<u128, CodecError> {
        let slice = self.read_exact(16)?;
        let mut arr = [0u8; 16];
        arr.copy_from_slice(slice);
        Ok(u128::from_le_bytes(arr))
    }

    pub fn read_exact(&mut self, len: usize) -> Result<&'a [u8], CodecError> {
        if self.pos + len > self.buf.len() {
            return Err(CodecError::UnexpectedEof);
        }
        let slice = &self.buf[self.pos..self.pos + len];
        self.pos += len;
        Ok(slice)
    }

    pub fn read_vec(&mut self, max_len: usize) -> Result<Vec<u8>, CodecError> {
        let len = self.read_u32_le()? as usize;
        if len > max_len {
            return Err(CodecError::InvalidEncoding(format!(
                "Vector length {len} exceeds bounds {max_len}"
            )));
        }
        let slice = self.read_exact(len)?;
        Ok(slice.to_vec())
    }
}

// ----------------------------------------------------------------------------
// Primitives Implementations
// ----------------------------------------------------------------------------

impl CanonicalSerialize for u8 {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        buf.push(*self);
    }
}

impl CanonicalDeserialize for u8 {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        reader.read_u8()
    }
}

impl CanonicalSerialize for u16 {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
}

impl CanonicalDeserialize for u16 {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        reader.read_u16_le()
    }
}

impl CanonicalSerialize for u32 {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
}

impl CanonicalDeserialize for u32 {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        reader.read_u32_le()
    }
}

impl CanonicalSerialize for u64 {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
}

impl CanonicalDeserialize for u64 {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        reader.read_u64_le()
    }
}

impl CanonicalSerialize for u128 {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
}

impl CanonicalDeserialize for u128 {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        reader.read_u128_le()
    }
}

impl CanonicalSerialize for Hash {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.0);
    }
}

impl CanonicalDeserialize for Hash {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        let slice = reader.read_exact(32)?;
        let mut arr = [0u8; 32];
        arr.copy_from_slice(slice);
        Ok(Hash(arr))
    }
}

impl CanonicalSerialize for Address {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.0);
    }
}

impl CanonicalDeserialize for Address {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        let slice = reader.read_exact(20)?;
        let mut arr = [0u8; 20];
        arr.copy_from_slice(slice);
        Ok(Address(arr))
    }
}

impl CanonicalSerialize for Amount {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.0.serialize_canonical(buf);
    }
}

impl CanonicalDeserialize for Amount {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        Ok(Amount(reader.read_u128_le()?))
    }
}

impl CanonicalSerialize for Nonce {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.0.serialize_canonical(buf);
    }
}

impl CanonicalDeserialize for Nonce {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        Ok(Nonce(reader.read_u64_le()?))
    }
}

impl CanonicalSerialize for Gas {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.0.serialize_canonical(buf);
    }
}

impl CanonicalDeserialize for Gas {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        Ok(Gas(reader.read_u64_le()?))
    }
}

impl CanonicalSerialize for ChainId {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.0.serialize_canonical(buf);
    }
}

impl CanonicalDeserialize for ChainId {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        Ok(ChainId(reader.read_u64_le()?))
    }
}

impl CanonicalSerialize for BlockHeight {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.0.serialize_canonical(buf);
    }
}

impl CanonicalDeserialize for BlockHeight {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        Ok(BlockHeight(reader.read_u64_le()?))
    }
}

impl CanonicalSerialize for Epoch {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.0.serialize_canonical(buf);
    }
}

impl CanonicalDeserialize for Epoch {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        Ok(Epoch(reader.read_u64_le()?))
    }
}

impl CanonicalSerialize for Round {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.0.serialize_canonical(buf);
    }
}

impl CanonicalDeserialize for Round {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        Ok(Round(reader.read_u32_le()?))
    }
}

impl CanonicalSerialize for Timestamp {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.0.serialize_canonical(buf);
    }
}

impl CanonicalDeserialize for Timestamp {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        Ok(Timestamp(reader.read_u64_le()?))
    }
}

impl CanonicalSerialize for TransactionId {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.0.serialize_canonical(buf);
    }
}

impl CanonicalDeserialize for TransactionId {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        Ok(TransactionId(Hash::read_canonical(reader)?))
    }
}

// ----------------------------------------------------------------------------
// Cryptographic Keys & Signatures
// ----------------------------------------------------------------------------

impl CanonicalSerialize for PublicKey {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        match self {
            PublicKey::Ed25519(pk) => {
                buf.push(0x01);
                buf.extend_from_slice(pk);
            }
            PublicKey::Secp256k1(pk) => {
                buf.push(0x02);
                buf.extend_from_slice(pk);
            }
        }
    }
}

impl CanonicalDeserialize for PublicKey {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        let tag = reader.read_u8()?;
        match tag {
            0x01 => {
                let slice = reader.read_exact(32)?;
                let mut pk = [0u8; 32];
                pk.copy_from_slice(slice);
                Ok(PublicKey::Ed25519(pk))
            }
            0x02 => {
                let slice = reader.read_exact(33)?;
                let mut pk = [0u8; 33];
                pk.copy_from_slice(slice);
                Ok(PublicKey::Secp256k1(pk))
            }
            other => Err(CodecError::InvalidEncoding(format!(
                "Invalid PublicKey tag: {other}"
            ))),
        }
    }
}

impl CanonicalSerialize for Signature {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        match self {
            Signature::Ed25519(s) => {
                buf.push(0x01);
                buf.extend_from_slice(s);
            }
            Signature::Secp256k1 { r, s, v } => {
                buf.push(0x02);
                buf.extend_from_slice(r);
                buf.extend_from_slice(s);
                buf.push(*v);
            }
            Signature::Secp256k1Eth {
                r,
                s,
                y_parity,
                chain_id,
            } => {
                buf.push(0x03);
                buf.extend_from_slice(r);
                buf.extend_from_slice(s);
                buf.push(*y_parity);
                buf.extend_from_slice(&chain_id.to_le_bytes());
            }
        }
    }
}

impl CanonicalDeserialize for Signature {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        let tag = reader.read_u8()?;
        match tag {
            0x01 => {
                let slice = reader.read_exact(64)?;
                let mut s = [0u8; 64];
                s.copy_from_slice(slice);
                Ok(Signature::Ed25519(s))
            }
            0x02 => {
                let r_slice = reader.read_exact(32)?;
                let mut r = [0u8; 32];
                r.copy_from_slice(r_slice);

                let s_slice = reader.read_exact(32)?;
                let mut s = [0u8; 32];
                s.copy_from_slice(s_slice);

                let v = reader.read_u8()?;
                Ok(Signature::Secp256k1 { r, s, v })
            }
            0x03 | TAG_SECP256K1_ETH_TYPED => {
                let r_slice = reader.read_exact(32)?;
                let mut r = [0u8; 32];
                r.copy_from_slice(r_slice);

                let s_slice = reader.read_exact(32)?;
                let mut s = [0u8; 32];
                s.copy_from_slice(s_slice);

                let y_parity = reader.read_u8()?;
                let chain_id = reader.read_u64_le()?;
                Ok(Signature::Secp256k1Eth {
                    r,
                    s,
                    y_parity,
                    chain_id,
                })
            }
            other => Err(CodecError::InvalidEncoding(format!(
                "Invalid Signature tag: {other}"
            ))),
        }
    }
}

// ----------------------------------------------------------------------------
// Account State (Canonical 92-byte leaf)
// ----------------------------------------------------------------------------

impl CanonicalSerialize for AccountState {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.nonce.serialize_canonical(buf);
        self.balance.serialize_canonical(buf);
        self.code_hash.serialize_canonical(buf);
        self.storage_root.serialize_canonical(buf);
        self.flags.serialize_canonical(buf);
    }
}

impl CanonicalDeserialize for AccountState {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        let nonce = Nonce::read_canonical(reader)?;
        let balance = Amount::read_canonical(reader)?;
        let code_hash = Hash::read_canonical(reader)?;
        let storage_root = Hash::read_canonical(reader)?;
        let flags = reader.read_u32_le()?;
        Ok(AccountState {
            nonce,
            balance,
            code_hash,
            storage_root,
            flags,
        })
    }
}

// ----------------------------------------------------------------------------
// Transaction Envelope
// ----------------------------------------------------------------------------

pub fn serialize_unsigned_transaction(tx: &Transaction, buf: &mut Vec<u8>) {
    tx.chain_id.serialize_canonical(buf);
    tx.nonce.serialize_canonical(buf);
    tx.from.serialize_canonical(buf);

    if let Some(ref to) = tx.to {
        buf.push(1u8);
        to.serialize_canonical(buf);
    } else {
        buf.push(0u8);
    }

    tx.value.serialize_canonical(buf);
    tx.gas_limit.serialize_canonical(buf);
    tx.max_fee_per_gas.serialize_canonical(buf);
    tx.max_priority_fee_per_gas.serialize_canonical(buf);
    tx.valid_until_block.serialize_canonical(buf);

    let len = tx.data.len() as u32;
    len.serialize_canonical(buf);
    buf.extend_from_slice(&tx.data);

    if let Some(ref memo) = tx.memo {
        let memo_bytes = memo.as_bytes();
        if !memo_bytes.is_empty() {
            (memo_bytes.len() as u32).serialize_canonical(buf);
            buf.extend_from_slice(memo_bytes);
        }
    }
}

/// Wire discriminant tag for Secp256k1Eth signatures carrying typed EVM transaction extensions (tx_type, access_list).
pub const TAG_SECP256K1_ETH_TYPED: u8 = 0x05;

impl CanonicalSerialize for Transaction {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        serialize_unsigned_transaction(self, buf);
        if self.is_evm() && (self.tx_type == 1 || !self.access_list.is_empty()) {
            if let Signature::Secp256k1Eth {
                r,
                s,
                y_parity,
                chain_id,
            } = &self.signature
            {
                buf.push(TAG_SECP256K1_ETH_TYPED);
                buf.extend_from_slice(r);
                buf.extend_from_slice(s);
                buf.push(*y_parity);
                buf.extend_from_slice(&chain_id.to_le_bytes());
                buf.push(self.tx_type);
                buf.extend_from_slice(&(self.access_list.len() as u32).to_le_bytes());
                for item in &self.access_list {
                    item.address.serialize_canonical(buf);
                    buf.extend_from_slice(&(item.storage_keys.len() as u32).to_le_bytes());
                    for key in &item.storage_keys {
                        key.serialize_canonical(buf);
                    }
                }
                return;
            }
        }
        self.signature.serialize_canonical(buf);
    }
}

impl CanonicalDeserialize for Transaction {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        let chain_id = ChainId::read_canonical(reader)?;
        let nonce = Nonce::read_canonical(reader)?;
        let from = Address::read_canonical(reader)?;

        let has_to = reader.read_u8()?;
        let to = match has_to {
            0 => None,
            1 => Some(Address::read_canonical(reader)?),
            other => {
                return Err(CodecError::InvalidEncoding(format!(
                    "Invalid Option<Address> tag: {other}"
                )))
            }
        };

        let value = Amount::read_canonical(reader)?;
        let gas_limit = Gas::read_canonical(reader)?;
        let max_fee_per_gas = Amount::read_canonical(reader)?;
        let max_priority_fee_per_gas = Amount::read_canonical(reader)?;
        let valid_until_block = BlockHeight::read_canonical(reader)?;
        let data = reader.read_vec(MAX_TRANSACTION_PAYLOAD_BYTES)?;

        let memo = {
            let has_memo = if (reader.remaining() == 66 && reader.peek_u8() == Some(0x02))
                || (reader.remaining() == 65 && reader.peek_u8() == Some(0x01))
                || (reader.remaining() == 74 && reader.peek_u8() == Some(0x03))
                || reader.remaining() < 4
            {
                false
            } else if let Some(slice) = reader.peek_slice(4) {
                let candidate_len =
                    u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as usize;
                if candidate_len == 0 || candidate_len > MAX_MEMO_BYTES {
                    false
                } else if slice[0] == 0x01
                    || slice[0] == 0x02
                    || slice[0] == 0x03
                    || slice[0] == TAG_SECP256K1_ETH_TYPED
                {
                    let total_memo_and_sig_min = 4 + candidate_len + 65;
                    if reader.remaining() < total_memo_and_sig_min {
                        false
                    } else {
                        let after_memo_tag = reader
                            .peek_slice(4 + candidate_len + 1)
                            .map(|s| s[4 + candidate_len]);
                        matches!(
                            after_memo_tag,
                            Some(0x01) | Some(0x02) | Some(0x03) | Some(TAG_SECP256K1_ETH_TYPED)
                        )
                    }
                } else {
                    true
                }
            } else {
                false
            };

            if has_memo {
                let memo_len = reader.read_u32_le()? as usize;
                if memo_len > MAX_MEMO_BYTES {
                    return Err(CodecError::InvalidEncoding(format!(
                        "Memo length {memo_len} exceeds maximum bound {MAX_MEMO_BYTES}"
                    )));
                }
                let memo_slice = reader.read_exact(memo_len)?;
                let memo_str = std::str::from_utf8(memo_slice).map_err(|e| {
                    CodecError::InvalidEncoding(format!("Invalid UTF-8 in memo: {e}"))
                })?;
                if memo_str.is_empty() {
                    None
                } else {
                    Some(memo_str.to_string())
                }
            } else {
                None
            }
        };

        let sig_tag = reader.read_u8()?;
        let (signature, tx_type, access_list) = match sig_tag {
            0x01 => {
                let slice = reader.read_exact(64)?;
                let mut s = [0u8; 64];
                s.copy_from_slice(slice);
                (Signature::Ed25519(s), 0, Vec::new())
            }
            0x02 => {
                let r_slice = reader.read_exact(32)?;
                let mut r = [0u8; 32];
                r.copy_from_slice(r_slice);

                let s_slice = reader.read_exact(32)?;
                let mut s = [0u8; 32];
                s.copy_from_slice(s_slice);

                let v = reader.read_u8()?;
                (Signature::Secp256k1 { r, s, v }, 0, Vec::new())
            }
            0x03 => {
                let r_slice = reader.read_exact(32)?;
                let mut r = [0u8; 32];
                r.copy_from_slice(r_slice);

                let s_slice = reader.read_exact(32)?;
                let mut s = [0u8; 32];
                s.copy_from_slice(s_slice);

                let y_parity = reader.read_u8()?;
                let sig_chain_id = reader.read_u64_le()?;
                let sig = Signature::Secp256k1Eth {
                    r,
                    s,
                    y_parity,
                    chain_id: sig_chain_id,
                };
                let resolved_type = resolve_eth_tx_type(
                    chain_id,
                    nonce,
                    from,
                    to,
                    value,
                    gas_limit,
                    max_fee_per_gas,
                    max_priority_fee_per_gas,
                    valid_until_block,
                    &data,
                    memo.as_deref(),
                    &sig,
                );
                (sig, resolved_type, Vec::new())
            }
            TAG_SECP256K1_ETH_TYPED => {
                let r_slice = reader.read_exact(32)?;
                let mut r = [0u8; 32];
                r.copy_from_slice(r_slice);

                let s_slice = reader.read_exact(32)?;
                let mut s = [0u8; 32];
                s.copy_from_slice(s_slice);

                let y_parity = reader.read_u8()?;
                let sig_chain_id = reader.read_u64_le()?;
                let sig = Signature::Secp256k1Eth {
                    r,
                    s,
                    y_parity,
                    chain_id: sig_chain_id,
                };
                let parsed_type = reader.read_u8()?;
                let al_count = reader.read_u32_le()? as usize;
                if al_count > MAX_ACCESS_LIST_ITEMS {
                    return Err(CodecError::InvalidEncoding(format!(
                        "Access list items count {al_count} exceeds maximum bound {MAX_ACCESS_LIST_ITEMS}"
                    )));
                }
                let mut al = Vec::with_capacity(al_count);
                for _ in 0..al_count {
                    let address = Address::read_canonical(reader)?;
                    let keys_count = reader.read_u32_le()? as usize;
                    if keys_count > 16_000 {
                        return Err(CodecError::InvalidEncoding(format!(
                            "Access list storage keys count {keys_count} exceeds maximum bound 16000"
                        )));
                    }
                    let mut storage_keys = Vec::with_capacity(keys_count);
                    for _ in 0..keys_count {
                        storage_keys.push(Hash::read_canonical(reader)?);
                    }
                    al.push(AccessListItem {
                        address,
                        storage_keys,
                    });
                }
                (sig, parsed_type, al)
            }
            other => {
                return Err(CodecError::InvalidEncoding(format!(
                    "Invalid Signature tag: {other}"
                )))
            }
        };

        Ok(Transaction {
            chain_id,
            nonce,
            from,
            to,
            value,
            gas_limit,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            valid_until_block,
            data,
            memo,
            signature,
            tx_type,
            access_list,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_eth_tx_type(
    chain_id: ChainId,
    nonce: Nonce,
    from: Address,
    to: Option<Address>,
    value: Amount,
    gas_limit: Gas,
    max_fee_per_gas: Amount,
    max_priority_fee_per_gas: Amount,
    valid_until_block: BlockHeight,
    data: &[u8],
    memo: Option<&str>,
    signature: &Signature,
) -> u8 {
    match signature {
        Signature::Secp256k1Eth {
            r,
            s,
            y_parity,
            chain_id: sig_chain_id,
        } => {
            // Fast-path: In Legacy EIP-155 transactions, gas_price is singular and aliases
            // into both fee fields. If max_fee != max_priority, it cannot be Legacy.
            if max_fee_per_gas != max_priority_fee_per_gas {
                2
            } else {
                // Equal-fee path: construct Type-2 signing preimage and test signature recovery
                let mut sig_65 = [0u8; 65];
                sig_65[0..32].copy_from_slice(r);
                sig_65[32..64].copy_from_slice(s);
                sig_65[64] = *y_parity;

                let candidate_tx = Transaction {
                    chain_id,
                    nonce,
                    from,
                    to,
                    value,
                    gas_limit,
                    max_fee_per_gas,
                    max_priority_fee_per_gas,
                    valid_until_block,
                    data: data.to_vec(),
                    memo: memo.map(|s| s.to_string()),
                    signature: Signature::Secp256k1Eth {
                        r: *r,
                        s: *s,
                        y_parity: *y_parity,
                        chain_id: *sig_chain_id,
                    },
                    tx_type: 0,
                    access_list: vec![],
                };

                let h_type2 = rlp::compute_type2_signing_hash(&candidate_tx);
                match tola_crypto::recover_secp256k1_address(h_type2.as_bytes(), &sig_65) {
                    Ok(recovered) if recovered == from => 2,
                    _ => {
                        let h_type1 = rlp::compute_type1_signing_hash(&candidate_tx);
                        match tola_crypto::recover_secp256k1_address(h_type1.as_bytes(), &sig_65) {
                            Ok(recovered1) if recovered1 == from => 1,
                            _ => 0,
                        }
                    }
                }
            }
        }
        _ => 0,
    }
}

// ----------------------------------------------------------------------------
// Block Header & Block
// ----------------------------------------------------------------------------

impl CanonicalSerialize for BlockHeader {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.parent_hash.serialize_canonical(buf);
        self.height.serialize_canonical(buf);
        self.epoch.serialize_canonical(buf);
        self.round.serialize_canonical(buf);
        self.timestamp.serialize_canonical(buf);
        self.proposer.serialize_canonical(buf);
        self.state_root.serialize_canonical(buf);
        self.txs_root.serialize_canonical(buf);
        self.receipts_root.serialize_canonical(buf);
        self.gas_used.serialize_canonical(buf);
        self.gas_limit.serialize_canonical(buf);
        self.base_fee_per_gas.serialize_canonical(buf);

        let len = self.extra_data.len() as u32;
        len.serialize_canonical(buf);
        buf.extend_from_slice(&self.extra_data);
    }
}

impl CanonicalDeserialize for BlockHeader {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        let parent_hash = Hash::read_canonical(reader)?;
        let height = BlockHeight::read_canonical(reader)?;
        let epoch = Epoch::read_canonical(reader)?;
        let round = Round::read_canonical(reader)?;
        let timestamp = Timestamp::read_canonical(reader)?;
        let proposer = Address::read_canonical(reader)?;
        let state_root = Hash::read_canonical(reader)?;
        let txs_root = Hash::read_canonical(reader)?;
        let receipts_root = Hash::read_canonical(reader)?;
        let gas_used = Gas::read_canonical(reader)?;
        let gas_limit = Gas::read_canonical(reader)?;
        let base_fee_per_gas = Amount::read_canonical(reader)?;
        let extra_data = reader.read_vec(64 * 1024)?;

        Ok(BlockHeader {
            parent_hash,
            height,
            epoch,
            round,
            timestamp,
            proposer,
            state_root,
            txs_root,
            receipts_root,
            gas_used,
            gas_limit,
            base_fee_per_gas,
            extra_data,
        })
    }
}

// ----------------------------------------------------------------------------
// Execution Receipts & Logs
// ----------------------------------------------------------------------------

impl CanonicalSerialize for Log {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.address.serialize_canonical(buf);
        (self.topics.len() as u32).serialize_canonical(buf);
        for topic in &self.topics {
            topic.serialize_canonical(buf);
        }
        (self.data.len() as u32).serialize_canonical(buf);
        buf.extend_from_slice(&self.data);
    }
}

impl CanonicalDeserialize for Log {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        let address = Address::read_canonical(reader)?;
        let topics_count = reader.read_u32_le()? as usize;
        if topics_count > 16 {
            return Err(CodecError::InvalidEncoding("Topics count > 16".into()));
        }
        let mut topics = Vec::with_capacity(topics_count);
        for _ in 0..topics_count {
            topics.push(Hash::read_canonical(reader)?);
        }
        let data = reader.read_vec(64 * 1024)?;
        Ok(Log {
            address,
            topics,
            data,
        })
    }
}

impl CanonicalSerialize for Receipt {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.tx_id.serialize_canonical(buf);
        buf.push(if self.success { 1 } else { 0 });
        self.gas_used.serialize_canonical(buf);
        self.cumulative_gas_used.serialize_canonical(buf);

        (self.logs.len() as u32).serialize_canonical(buf);
        for log in &self.logs {
            log.serialize_canonical(buf);
        }

        (self.return_data.len() as u32).serialize_canonical(buf);
        buf.extend_from_slice(&self.return_data);
    }
}

impl CanonicalDeserialize for Receipt {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        let tx_id = TransactionId::read_canonical(reader)?;
        let success = reader.read_u8()? != 0;
        let gas_used = Gas::read_canonical(reader)?;
        let cumulative_gas_used = Gas::read_canonical(reader)?;

        let logs_count = reader.read_u32_le()? as usize;
        if logs_count > 1024 {
            return Err(CodecError::InvalidEncoding("Logs count > 1024".into()));
        }
        let mut logs = Vec::with_capacity(logs_count);
        for _ in 0..logs_count {
            logs.push(Log::read_canonical(reader)?);
        }

        let return_data = reader.read_vec(64 * 1024)?;

        Ok(Receipt {
            tx_id,
            success,
            gas_used,
            cumulative_gas_used,
            logs,
            return_data,
        })
    }
}

// ----------------------------------------------------------------------------
// Consensus & Block Canonical Codec Implementations
// ----------------------------------------------------------------------------

impl CanonicalSerialize for VoteType {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        match self {
            VoteType::Prevote => buf.push(0x00),
            VoteType::Precommit => buf.push(0x01),
        }
    }
}

impl CanonicalDeserialize for VoteType {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        let tag = reader.read_u8()?;
        match tag {
            0x00 => Ok(VoteType::Prevote),
            0x01 => Ok(VoteType::Precommit),
            other => Err(CodecError::InvalidEncoding(format!(
                "Invalid VoteType tag: 0x{other:02x}"
            ))),
        }
    }
}

impl CanonicalSerialize for ValidatorId {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.0.serialize_canonical(buf);
    }
}

impl CanonicalDeserialize for ValidatorId {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        let addr = Address::read_canonical(reader)?;
        Ok(ValidatorId(addr))
    }
}

impl CanonicalSerialize for Vote {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.vote_type.serialize_canonical(buf);
        self.height.serialize_canonical(buf);
        self.round.serialize_canonical(buf);
        self.block_hash.serialize_canonical(buf);
        self.validator.serialize_canonical(buf);
        self.signature.serialize_canonical(buf);
    }
}

impl CanonicalDeserialize for Vote {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        let vote_type = VoteType::read_canonical(reader)?;
        let height = BlockHeight::read_canonical(reader)?;
        let round = Round::read_canonical(reader)?;
        let block_hash = Hash::read_canonical(reader)?;
        let validator = ValidatorId::read_canonical(reader)?;
        let signature = Signature::read_canonical(reader)?;

        Ok(Vote {
            vote_type,
            height,
            round,
            block_hash,
            validator,
            signature,
        })
    }
}

impl CanonicalSerialize for QuorumCertificate {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.height.serialize_canonical(buf);
        self.round.serialize_canonical(buf);
        self.block_hash.serialize_canonical(buf);
        self.vote_type.serialize_canonical(buf);

        buf.extend_from_slice(&(self.validator_bitfield.len() as u32).to_le_bytes());
        buf.extend_from_slice(&self.validator_bitfield);

        buf.extend_from_slice(&(self.aggregated_signature.len() as u32).to_le_bytes());
        buf.extend_from_slice(&self.aggregated_signature);
    }
}

impl CanonicalDeserialize for QuorumCertificate {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        let height = BlockHeight::read_canonical(reader)?;
        let round = Round::read_canonical(reader)?;
        let block_hash = Hash::read_canonical(reader)?;
        let vote_type = VoteType::read_canonical(reader)?;

        let bitfield = reader.read_vec(1024 * 64)?;
        let sig = reader.read_vec(1024 * 64)?;

        Ok(QuorumCertificate {
            height,
            round,
            block_hash,
            vote_type,
            validator_bitfield: bitfield,
            aggregated_signature: sig,
        })
    }
}

impl CanonicalSerialize for Block {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        self.header.serialize_canonical(buf);

        buf.extend_from_slice(&(self.transactions.len() as u32).to_le_bytes());
        for tx in &self.transactions {
            tx.serialize_canonical(buf);
        }

        match &self.last_commit_qc {
            Some(qc) => {
                buf.push(0x01);
                qc.serialize_canonical(buf);
            }
            None => {
                buf.push(0x00);
            }
        }
    }
}

impl CanonicalDeserialize for Block {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        let header = BlockHeader::read_canonical(reader)?;

        let tx_count = reader.read_u32_le()? as usize;
        if tx_count > 50_000 {
            return Err(CodecError::InvalidEncoding(
                "Block transaction count exceeds limit of 50000".into(),
            ));
        }
        let mut transactions = Vec::with_capacity(tx_count);
        for _ in 0..tx_count {
            transactions.push(Transaction::read_canonical(reader)?);
        }

        let has_qc = reader.read_u8()?;
        let last_commit_qc = match has_qc {
            0x00 => None,
            0x01 => Some(QuorumCertificate::read_canonical(reader)?),
            other => {
                return Err(CodecError::InvalidEncoding(format!(
                    "Invalid QC flag: 0x{other:02x}"
                )))
            }
        };

        Ok(Block {
            header,
            transactions,
            last_commit_qc,
        })
    }
}

// ----------------------------------------------------------------------------
// Unit Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_roundtrip() {
        let original = Hash([42u8; 32]);
        let bytes = original.to_canonical_bytes();
        assert_eq!(bytes.len(), 32);
        let decoded = Hash::deserialize_canonical(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_account_state_roundtrip() {
        let account = AccountState {
            nonce: Nonce(15),
            balance: Amount::from_u128(999_888_777),
            code_hash: Hash([0xaa; 32]),
            storage_root: Hash([0xbb; 32]),
            flags: FLAG_CONTRACT | FLAG_VALIDATOR,
        };
        let bytes = account.to_canonical_bytes();
        assert_eq!(bytes.len(), 8 + 16 + 32 + 32 + 4); // Exactly 92 bytes!
        let decoded = AccountState::deserialize_canonical(&bytes).unwrap();
        assert_eq!(decoded, account);
    }

    #[test]
    fn test_transaction_roundtrip() {
        let tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(42),
            from: Address([0x11; 20]),
            to: Some(Address([0x22; 20])),
            value: Amount::from_u128(1_000_000),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(20),
            max_priority_fee_per_gas: Amount::from_u128(2),
            valid_until_block: BlockHeight(500),
            data: vec![1, 2, 3, 4, 5],
            memo: None,
            signature: Signature::Secp256k1 {
                r: [0x33; 32],
                s: [0x44; 32],
                v: 1,
            },
            tx_type: 0,
            access_list: vec![],
        };

        let bytes = tx.to_canonical_bytes();
        let decoded = Transaction::deserialize_canonical(&bytes).unwrap();
        assert_eq!(decoded, tx);
    }

    #[test]
    fn test_transaction_memo_roundtrip() {
        let mut tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(42),
            from: Address([0x11; 20]),
            to: Some(Address([0x22; 20])),
            value: Amount::from_u128(1_000_000),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(20),
            max_priority_fee_per_gas: Amount::from_u128(2),
            valid_until_block: BlockHeight(500),
            data: vec![1, 2, 3, 4, 5],
            memo: Some("Invoice #1042".to_string()),
            signature: Signature::Secp256k1 {
                r: [0x33; 32],
                s: [0x44; 32],
                v: 1,
            },
            tx_type: 0,
            access_list: vec![],
        };

        // Roundtrip with English memo
        let bytes = tx.to_canonical_bytes();
        let decoded = Transaction::deserialize_canonical(&bytes).unwrap();
        assert_eq!(decoded, tx);

        // Roundtrip with Uzbek characters and emoji
        tx.memo = Some("To‘lov 💰 #99".to_string());
        let bytes2 = tx.to_canonical_bytes();
        let decoded2 = Transaction::deserialize_canonical(&bytes2).unwrap();
        assert_eq!(decoded2, tx);

        // Max 256 bytes memo roundtrip
        tx.memo = Some("M".repeat(256));
        let bytes3 = tx.to_canonical_bytes();
        let decoded3 = Transaction::deserialize_canonical(&bytes3).unwrap();
        assert_eq!(decoded3, tx);
    }

    #[test]
    fn test_block_header_roundtrip() {
        let header = BlockHeader {
            parent_hash: Hash([0x01; 32]),
            height: BlockHeight(10),
            epoch: Epoch(1),
            round: Round(2),
            timestamp: Timestamp(1700000000),
            proposer: Address([0x77; 20]),
            state_root: Hash([0x02; 32]),
            txs_root: Hash([0x03; 32]),
            receipts_root: Hash([0x04; 32]),
            gas_used: Gas(42_000),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(15),
            extra_data: vec![0xca, 0xfe],
        };

        let bytes = header.to_canonical_bytes();
        let decoded = BlockHeader::deserialize_canonical(&bytes).unwrap();
        assert_eq!(decoded, header);
    }

    #[test]
    fn test_trailing_bytes_rejected() {
        let hash = Hash([0x99; 32]);
        let mut bytes = hash.to_canonical_bytes();
        bytes.push(0x00); // 1 extra byte
        assert_eq!(
            Hash::deserialize_canonical(&bytes).unwrap_err(),
            CodecError::TrailingBytes
        );
    }

    #[test]
    fn test_vote_roundtrip() {
        let vote = Vote {
            vote_type: VoteType::Precommit,
            height: BlockHeight(100),
            round: Round(3),
            block_hash: Hash([0x55; 32]),
            validator: ValidatorId(Address([0x88; 20])),
            signature: Signature::Secp256k1 {
                r: [0x11; 32],
                s: [0x22; 32],
                v: 0,
            },
        };

        let bytes = vote.to_canonical_bytes();
        let decoded = Vote::deserialize_canonical(&bytes).unwrap();
        assert_eq!(decoded, vote);
    }

    #[test]
    fn test_block_roundtrip() {
        let header = BlockHeader {
            parent_hash: Hash([0x01; 32]),
            height: BlockHeight(10),
            epoch: Epoch(1),
            round: Round(2),
            timestamp: Timestamp(1700000000),
            proposer: Address([0x77; 20]),
            state_root: Hash([0x02; 32]),
            txs_root: Hash([0x03; 32]),
            receipts_root: Hash([0x04; 32]),
            gas_used: Gas(42_000),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(15),
            extra_data: vec![0xca, 0xfe],
        };
        let block = Block {
            header,
            transactions: vec![],
            last_commit_qc: Some(QuorumCertificate {
                height: BlockHeight(9),
                round: Round(1),
                block_hash: Hash([0x01; 32]),
                vote_type: VoteType::Precommit,
                validator_bitfield: vec![0b00000001],
                aggregated_signature: vec![0x77; 64],
            }),
        };

        let bytes = block.to_canonical_bytes();
        let decoded = Block::deserialize_canonical(&bytes).unwrap();
        assert_eq!(decoded, block);
    }

    #[test]
    fn test_type1_with_access_list_canonical_roundtrip() {
        let al_item = AccessListItem {
            address: Address([0xaa; 20]),
            storage_keys: vec![Hash([0x11; 32]), Hash([0x22; 32])],
        };
        let tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(10),
            from: Address([0x11; 20]),
            to: Some(Address([0x22; 20])),
            value: Amount::from_u128(500_000),
            gas_limit: Gas(50_000),
            max_fee_per_gas: Amount::from_u128(20_000_000_000),
            max_priority_fee_per_gas: Amount::from_u128(20_000_000_000),
            valid_until_block: BlockHeight(500),
            data: vec![0x12, 0x34],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0x55; 32],
                s: [0x66; 32],
                y_parity: 1,
                chain_id: ChainId::MAINNET.0,
            },
            tx_type: 1,
            access_list: vec![al_item],
        };

        let bytes = tx.to_canonical_bytes();
        // Verify TAG_SECP256K1_ETH_TYPED tag is present in the serialized bytes
        assert!(bytes.contains(&TAG_SECP256K1_ETH_TYPED));

        let decoded = Transaction::deserialize_canonical(&bytes).unwrap();
        assert_eq!(decoded.tx_type, 1);
        assert_eq!(decoded.access_list.len(), 1);
        assert_eq!(decoded.access_list[0].address, Address([0xaa; 20]));
        assert_eq!(decoded.access_list[0].storage_keys.len(), 2);
        assert_eq!(decoded, tx);
    }

    #[test]
    fn test_type2_with_access_list_canonical_roundtrip() {
        let al_item1 = AccessListItem {
            address: Address([0xbb; 20]),
            storage_keys: vec![Hash([0x33; 32])],
        };
        let al_item2 = AccessListItem {
            address: Address([0xcc; 20]),
            storage_keys: vec![Hash([0x44; 32]), Hash([0x55; 32])],
        };
        let tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(42),
            from: Address([0x33; 20]),
            to: Some(Address([0x44; 20])),
            value: Amount::from_u128(1_000_000),
            gas_limit: Gas(100_000),
            max_fee_per_gas: Amount::from_u128(30_000_000_000),
            max_priority_fee_per_gas: Amount::from_u128(2_000_000_000),
            valid_until_block: BlockHeight(600),
            data: vec![0xca, 0xfe],
            memo: Some("Typed Tx Memo".into()),
            signature: Signature::Secp256k1Eth {
                r: [0x77; 32],
                s: [0x88; 32],
                y_parity: 0,
                chain_id: ChainId::MAINNET.0,
            },
            tx_type: 2,
            access_list: vec![al_item1, al_item2],
        };

        let bytes = tx.to_canonical_bytes();
        assert!(bytes.contains(&TAG_SECP256K1_ETH_TYPED));

        let decoded = Transaction::deserialize_canonical(&bytes).unwrap();
        assert_eq!(decoded.tx_type, 2);
        assert_eq!(decoded.access_list.len(), 2);
        assert_eq!(decoded.memo, Some("Typed Tx Memo".into()));
        assert_eq!(decoded, tx);
    }

    #[test]
    fn test_mixed_block_canonical_roundtrip() {
        let native_tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(1),
            from: Address([0x10; 20]),
            to: Some(Address([0x20; 20])),
            value: Amount::from_u128(100),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(1),
            max_priority_fee_per_gas: Amount::from_u128(1),
            valid_until_block: BlockHeight(10),
            data: vec![],
            memo: None,
            signature: Signature::Ed25519([0x99; 64]),
            tx_type: 0,
            access_list: vec![],
        };

        let legacy_eth_tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(2),
            from: Address([0x11; 20]),
            to: Some(Address([0x21; 20])),
            value: Amount::from_u128(200),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(10),
            max_priority_fee_per_gas: Amount::from_u128(10),
            valid_until_block: BlockHeight(10),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0x01; 32],
                s: [0x02; 32],
                y_parity: 0,
                chain_id: ChainId::MAINNET.0,
            },
            tx_type: 0,
            access_list: vec![],
        };

        let type1_tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(3),
            from: Address([0x12; 20]),
            to: Some(Address([0x22; 20])),
            value: Amount::from_u128(300),
            gas_limit: Gas(30_000),
            max_fee_per_gas: Amount::from_u128(15),
            max_priority_fee_per_gas: Amount::from_u128(15),
            valid_until_block: BlockHeight(10),
            data: vec![0xaa],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0x03; 32],
                s: [0x04; 32],
                y_parity: 1,
                chain_id: ChainId::MAINNET.0,
            },
            tx_type: 1,
            access_list: vec![AccessListItem {
                address: Address([0x30; 20]),
                storage_keys: vec![Hash([0x40; 32])],
            }],
        };

        let type2_tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(4),
            from: Address([0x13; 20]),
            to: Some(Address([0x23; 20])),
            value: Amount::from_u128(400),
            gas_limit: Gas(40_000),
            max_fee_per_gas: Amount::from_u128(20),
            max_priority_fee_per_gas: Amount::from_u128(5),
            valid_until_block: BlockHeight(10),
            data: vec![0xbb],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0x05; 32],
                s: [0x06; 32],
                y_parity: 0,
                chain_id: ChainId::MAINNET.0,
            },
            tx_type: 2,
            access_list: vec![AccessListItem {
                address: Address([0x50; 20]),
                storage_keys: vec![Hash([0x60; 32]), Hash([0x70; 32])],
            }],
        };

        let type2_empty_al_tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(5),
            from: Address([0x14; 20]),
            to: Some(Address([0x24; 20])),
            value: Amount::from_u128(500),
            gas_limit: Gas(50_000),
            max_fee_per_gas: Amount::from_u128(25),
            max_priority_fee_per_gas: Amount::from_u128(3),
            valid_until_block: BlockHeight(10),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0x07; 32],
                s: [0x08; 32],
                y_parity: 1,
                chain_id: ChainId::MAINNET.0,
            },
            tx_type: 2,
            access_list: vec![],
        };

        let block = Block {
            header: BlockHeader {
                parent_hash: Hash([0x01; 32]),
                height: BlockHeight(100),
                epoch: Epoch(1),
                round: Round(2),
                timestamp: Timestamp(1700000000),
                proposer: Address([0x77; 20]),
                state_root: Hash([0x02; 32]),
                txs_root: Hash([0x03; 32]),
                receipts_root: Hash([0x04; 32]),
                gas_used: Gas(100_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(10),
                extra_data: vec![],
            },
            transactions: vec![
                native_tx.clone(),
                legacy_eth_tx.clone(),
                type1_tx.clone(),
                type2_tx.clone(),
                type2_empty_al_tx.clone(),
            ],
            last_commit_qc: Some(QuorumCertificate {
                height: BlockHeight(99),
                round: Round(1),
                block_hash: Hash([0x01; 32]),
                vote_type: VoteType::Precommit,
                validator_bitfield: vec![0b00000001],
                aggregated_signature: vec![0x77; 64],
            }),
        };

        let bytes = block.to_canonical_bytes();
        let decoded = Block::deserialize_canonical(&bytes).unwrap();

        assert_eq!(decoded.transactions.len(), 5);
        assert_eq!(decoded.transactions[0], native_tx);
        assert_eq!(decoded.transactions[1], legacy_eth_tx);
        assert_eq!(decoded.transactions[2], type1_tx);
        assert_eq!(decoded.transactions[3], type2_tx);
        assert_eq!(decoded.transactions[4], type2_empty_al_tx);
        assert_eq!(decoded, block);
    }

    #[test]
    fn test_collision_regression_chain_id_21778260() {
        // Reproduce the exact framing collision where Tx 1's chain_id begins with
        // bytes [0x54, 0x4F, 0x4C, 0x01] (21,778,260 in little-endian).
        // Under the old magic-peek parser, Tx 0 misinterpreted Tx 1's chain_id
        // as its own extension and corrupted the block stream.
        let magic_chain_id = ChainId(21_778_260);

        let tx0 = Transaction {
            chain_id: magic_chain_id,
            nonce: Nonce(0),
            from: Address([0x11; 20]),
            to: Some(Address([0x22; 20])),
            value: Amount::from_u128(100),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(10),
            max_priority_fee_per_gas: Amount::from_u128(10),
            valid_until_block: BlockHeight(10),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0x01; 32],
                s: [0x02; 32],
                y_parity: 0,
                chain_id: magic_chain_id.0,
            },
            tx_type: 0,
            access_list: vec![],
        };

        let tx1 = Transaction {
            chain_id: magic_chain_id,
            nonce: Nonce(1),
            from: Address([0x33; 20]),
            to: Some(Address([0x44; 20])),
            value: Amount::from_u128(200),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(10),
            max_priority_fee_per_gas: Amount::from_u128(10),
            valid_until_block: BlockHeight(10),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0x03; 32],
                s: [0x04; 32],
                y_parity: 0,
                chain_id: magic_chain_id.0,
            },
            tx_type: 0,
            access_list: vec![],
        };

        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(0),
                timestamp: Timestamp(1000),
                proposer: Address([0x99; 20]),
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(42_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::ZERO,
                extra_data: vec![],
            },
            transactions: vec![tx0.clone(), tx1.clone()],
            last_commit_qc: None,
        };

        let block_bytes = block.to_canonical_bytes();
        let decoded = Block::deserialize_canonical(&block_bytes)
            .expect("Block with collision chain_id 21,778,260 must deserialize cleanly");

        assert_eq!(decoded.transactions.len(), 2);
        assert_eq!(decoded.transactions[0], tx0);
        assert_eq!(decoded.transactions[1], tx1);
        assert_eq!(decoded, block);
    }

    #[test]
    fn test_adjacent_transactions_all_combinations() {
        let native_ed = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(1),
            from: Address([0x01; 20]),
            to: Some(Address([0x02; 20])),
            value: Amount::from_u128(10),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(1),
            max_priority_fee_per_gas: Amount::from_u128(1),
            valid_until_block: BlockHeight(10),
            data: vec![],
            memo: None,
            signature: Signature::Ed25519([0x11; 64]),
            tx_type: 0,
            access_list: vec![],
        };

        let native_secp = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(2),
            from: Address([0x03; 20]),
            to: Some(Address([0x04; 20])),
            value: Amount::from_u128(20),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(1),
            max_priority_fee_per_gas: Amount::from_u128(1),
            valid_until_block: BlockHeight(10),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1 {
                r: [0x22; 32],
                s: [0x33; 32],
                v: 27,
            },
            tx_type: 0,
            access_list: vec![],
        };

        let legacy_eth = Transaction {
            chain_id: ChainId(21_778_260),
            nonce: Nonce(3),
            from: Address([0x05; 20]),
            to: Some(Address([0x06; 20])),
            value: Amount::from_u128(30),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(10),
            max_priority_fee_per_gas: Amount::from_u128(10),
            valid_until_block: BlockHeight(10),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0x44; 32],
                s: [0x55; 32],
                y_parity: 0,
                chain_id: 21_778_260,
            },
            tx_type: 0,
            access_list: vec![],
        };

        let typed_type1 = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(4),
            from: Address([0x07; 20]),
            to: Some(Address([0x08; 20])),
            value: Amount::from_u128(40),
            gas_limit: Gas(30_000),
            max_fee_per_gas: Amount::from_u128(15),
            max_priority_fee_per_gas: Amount::from_u128(15),
            valid_until_block: BlockHeight(10),
            data: vec![0x01],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0x66; 32],
                s: [0x77; 32],
                y_parity: 1,
                chain_id: ChainId::MAINNET.0,
            },
            tx_type: 1,
            access_list: vec![AccessListItem {
                address: Address([0x10; 20]),
                storage_keys: vec![Hash([0x20; 32])],
            }],
        };

        let typed_type2 = Transaction {
            chain_id: ChainId(21_778_260),
            nonce: Nonce(5),
            from: Address([0x09; 20]),
            to: Some(Address([0x0a; 20])),
            value: Amount::from_u128(50),
            gas_limit: Gas(40_000),
            max_fee_per_gas: Amount::from_u128(20),
            max_priority_fee_per_gas: Amount::from_u128(5),
            valid_until_block: BlockHeight(10),
            data: vec![0x02],
            memo: Some("type2".into()),
            signature: Signature::Secp256k1Eth {
                r: [0x88; 32],
                s: [0x99; 32],
                y_parity: 0,
                chain_id: 21_778_260,
            },
            tx_type: 2,
            access_list: vec![AccessListItem {
                address: Address([0x30; 20]),
                storage_keys: vec![Hash([0x40; 32]), Hash([0x50; 32])],
            }],
        };

        // Permutation 1: NativeEd, Typed1, LegacyEth, Typed2, NativeSecp
        let block1 = Block {
            header: BlockHeader {
                parent_hash: Hash([0x10; 32]),
                height: BlockHeight(10),
                epoch: Epoch(1),
                round: Round(1),
                timestamp: Timestamp(100),
                proposer: Address([0xaa; 20]),
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(100_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::ZERO,
                extra_data: vec![],
            },
            transactions: vec![
                native_ed.clone(),
                typed_type1.clone(),
                legacy_eth.clone(),
                typed_type2.clone(),
                native_secp.clone(),
            ],
            last_commit_qc: None,
        };

        let b1_bytes = block1.to_canonical_bytes();
        let d1 = Block::deserialize_canonical(&b1_bytes).unwrap();
        assert_eq!(d1, block1);

        // Permutation 2: Typed2, Typed1, LegacyEth, NativeEd with QC
        let block2 = Block {
            header: BlockHeader {
                parent_hash: Hash([0x20; 32]),
                height: BlockHeight(11),
                epoch: Epoch(1),
                round: Round(2),
                timestamp: Timestamp(200),
                proposer: Address([0xbb; 20]),
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(80_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::ZERO,
                extra_data: vec![],
            },
            transactions: vec![
                typed_type2.clone(),
                typed_type1.clone(),
                legacy_eth.clone(),
                native_ed.clone(),
            ],
            last_commit_qc: Some(QuorumCertificate {
                height: BlockHeight(10),
                round: Round(1),
                block_hash: Hash([0x10; 32]),
                vote_type: VoteType::Precommit,
                validator_bitfield: vec![0b00000011],
                aggregated_signature: vec![0x88; 64],
            }),
        };

        let b2_bytes = block2.to_canonical_bytes();
        let d2 = Block::deserialize_canonical(&b2_bytes).unwrap();
        assert_eq!(d2, block2);
    }

    #[test]
    fn test_malformed_and_truncated_bounds_enforcement() {
        // 1. Invalid signature tag
        let mut bad_tag_bytes = vec![0u8; 120];
        bad_tag_bytes[100] = 0x99; // unknown tag
        assert!(matches!(
            Transaction::deserialize_canonical(&bad_tag_bytes),
            Err(CodecError::InvalidEncoding(_)) | Err(CodecError::UnexpectedEof)
        ));

        // 2. Trailing garbage in standalone deserialization
        let tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(1),
            from: Address([0x11; 20]),
            to: None,
            value: Amount::ZERO,
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(1),
            max_priority_fee_per_gas: Amount::from_u128(1),
            valid_until_block: BlockHeight(10),
            data: vec![],
            memo: None,
            signature: Signature::Ed25519([0x01; 64]),
            tx_type: 0,
            access_list: vec![],
        };
        let mut bytes = tx.to_canonical_bytes();
        bytes.push(0xff); // trailing garbage byte
        assert!(matches!(
            Transaction::deserialize_canonical(&bytes),
            Err(CodecError::TrailingBytes)
        ));

        // 3. Truncated block
        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(0),
                timestamp: Timestamp(0),
                proposer: Address::ZERO,
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas::ZERO,
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::ZERO,
                extra_data: vec![],
            },
            transactions: vec![tx],
            last_commit_qc: None,
        };
        let b_bytes = block.to_canonical_bytes();
        let truncated = &b_bytes[..b_bytes.len() - 10];
        assert!(matches!(
            Block::deserialize_canonical(truncated),
            Err(CodecError::UnexpectedEof)
        ));
    }
}
