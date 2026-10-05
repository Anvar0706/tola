//! Canonical binary serialization codec for TOLA Network.

use tola_types::*;

#[derive(Debug, PartialEq, Eq)]
pub enum CodecError {
    UnexpectedEof,
    TrailingBytes,
    InvalidEncoding(String),
}

pub trait CanonicalSerialize {
    fn serialize_canonical(&self, buf: &mut Vec<u8>);
    fn to_canonical_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.serialize_canonical(&mut buf);
        buf
    }
}

pub trait CanonicalDeserialize: Sized {
    fn deserialize_canonical(bytes: &[u8]) -> Result<Self, CodecError>;
}

impl CanonicalSerialize for Hash {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.0);
    }
}

impl CanonicalDeserialize for Hash {
    fn deserialize_canonical(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 32 {
            return Err(CodecError::InvalidEncoding("Hash must be exactly 32 bytes".into()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(bytes);
        Ok(Hash(arr))
    }
}

impl CanonicalSerialize for Address {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.0);
    }
}

impl CanonicalDeserialize for Address {
    fn deserialize_canonical(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 20 {
            return Err(CodecError::InvalidEncoding("Address must be exactly 20 bytes".into()));
        }
        let mut arr = [0u8; 20];
        arr.copy_from_slice(bytes);
        Ok(Address(arr))
    }
}

impl CanonicalSerialize for Amount {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.0.to_le_bytes());
    }
}

impl CanonicalDeserialize for Amount {
    fn deserialize_canonical(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 16 {
            return Err(CodecError::InvalidEncoding("Amount must be 16 bytes le".into()));
        }
        let mut arr = [0u8; 16];
        arr.copy_from_slice(bytes);
        Ok(Amount(u128::from_le_bytes(arr)))
    }
}
