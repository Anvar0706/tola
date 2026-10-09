//! Canonical wire framing, magic handshakes, and packet codecs for TOLA Network.
//!
//! Enforces binary framing, BLAKE3 payload checksums, network isolation,
//! and strictly bounded memory allocations.

use tola_codec::{ByteReader, CanonicalDeserialize, CanonicalSerialize, CodecError};
use tola_core::NetworkError;
use tola_crypto::blake3_hash;
use tola_types::{Block, BlockHeight, Hash, Network, Transaction, Vote};

/// Protocol magic header: ASCII for "TOLA" (0x54, 0x4F, 0x4C, 0x41).
pub const PROTOCOL_MAGIC: [u8; 4] = [0x54, 0x4F, 0x4C, 0x41];

/// Current wire protocol version.
pub const PROTOCOL_VERSION: u16 = 1;

/// Maximum payload size allowed for a single wire frame (4 MB + 64 KB overhead).
pub const MAX_PAYLOAD_SIZE: usize = 4 * 1024 * 1024 + 64 * 1024;

/// Wire frame header size:
/// Magic (4B) + Version (2B) + Network (1B) + Type (1B) + Length (4B) + Checksum (32B) = 44 bytes.
pub const FRAME_HEADER_SIZE: usize = 44;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum NetworkId {
    Mainnet = 1,
    Testnet = 2,
    Devnet = 3,
}

impl NetworkId {
    pub fn from_u8(val: u8) -> Result<Self, NetworkError> {
        match val {
            1 => Ok(NetworkId::Mainnet),
            2 => Ok(NetworkId::Testnet),
            3 => Ok(NetworkId::Devnet),
            _other => Err(NetworkError::NetworkMismatch),
        }
    }

    pub fn matches_network(&self, net: Network) -> bool {
        matches!(
            (self, net),
            (NetworkId::Mainnet, Network::Mainnet)
                | (NetworkId::Testnet, Network::Testnet)
                | (NetworkId::Devnet, Network::Devnet)
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum MessageType {
    Handshake = 0x01,
    HandshakeAck = 0x02,
    Ping = 0x03,
    Pong = 0x04,
    GossipTransaction = 0x10,
    GossipBlock = 0x11,
    GossipVote = 0x12,
    GetBlocks = 0x20,
    Blocks = 0x21,
    Disconnect = 0x30,
}

impl MessageType {
    pub fn from_u8(val: u8) -> Result<Self, NetworkError> {
        match val {
            0x01 => Ok(MessageType::Handshake),
            0x02 => Ok(MessageType::HandshakeAck),
            0x03 => Ok(MessageType::Ping),
            0x04 => Ok(MessageType::Pong),
            0x10 => Ok(MessageType::GossipTransaction),
            0x11 => Ok(MessageType::GossipBlock),
            0x12 => Ok(MessageType::GossipVote),
            0x20 => Ok(MessageType::GetBlocks),
            0x21 => Ok(MessageType::Blocks),
            0x30 => Ok(MessageType::Disconnect),
            other => Err(NetworkError::MalformedMessage(format!(
                "Unknown message type: 0x{other:02x}"
            ))),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum DisconnectReason {
    Normal = 0,
    IncompatibleVersion = 1,
    IncompatibleNetwork = 2,
    GenesisMismatch = 3,
    SubnetLimitExceeded = 4,
    BadBehavior = 5,
    RateLimitExceeded = 6,
}

impl DisconnectReason {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => DisconnectReason::IncompatibleVersion,
            2 => DisconnectReason::IncompatibleNetwork,
            3 => DisconnectReason::GenesisMismatch,
            4 => DisconnectReason::SubnetLimitExceeded,
            5 => DisconnectReason::BadBehavior,
            6 => DisconnectReason::RateLimitExceeded,
            _ => DisconnectReason::Normal,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HandshakePayload {
    pub network_id: NetworkId,
    pub genesis_hash: Hash,
    pub head_height: BlockHeight,
    pub head_hash: Hash,
    pub client_version: String,
    pub listen_port: u16,
}

impl CanonicalSerialize for HandshakePayload {
    fn serialize_canonical(&self, buf: &mut Vec<u8>) {
        buf.push(self.network_id as u8);
        self.genesis_hash.serialize_canonical(buf);
        self.head_height.serialize_canonical(buf);
        self.head_hash.serialize_canonical(buf);
        let version_bytes = self.client_version.as_bytes();
        buf.extend_from_slice(&(version_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(version_bytes);
        buf.extend_from_slice(&self.listen_port.to_le_bytes());
    }
}

impl CanonicalDeserialize for HandshakePayload {
    fn read_canonical(reader: &mut ByteReader) -> Result<Self, CodecError> {
        let net_byte = reader.read_u8()?;
        let network_id = NetworkId::from_u8(net_byte)
            .map_err(|_| CodecError::InvalidEncoding(format!("Invalid network id: {net_byte}")))?;
        let genesis_hash = Hash::read_canonical(reader)?;
        let head_height = BlockHeight::read_canonical(reader)?;
        let head_hash = Hash::read_canonical(reader)?;

        let version_raw = reader.read_vec(128)?;
        let client_version = String::from_utf8(version_raw)
            .map_err(|_| CodecError::InvalidEncoding("Client version not UTF-8".into()))?;

        let listen_port = reader.read_u16_le()?;

        Ok(HandshakePayload {
            network_id,
            genesis_hash,
            head_height,
            head_hash,
            client_version,
            listen_port,
        })
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WireMessage {
    Handshake(HandshakePayload),
    HandshakeAck {
        success: bool,
        message: String,
    },
    Ping(u64),
    Pong(u64),
    GossipTransaction(Transaction),
    GossipBlock(Block),
    GossipVote(Vote),
    GetBlocks {
        start_height: BlockHeight,
        max_count: u32,
    },
    Blocks(Vec<Block>),
    Disconnect {
        reason: DisconnectReason,
        message: String,
    },
}

impl WireMessage {
    pub fn message_type(&self) -> MessageType {
        match self {
            WireMessage::Handshake(_) => MessageType::Handshake,
            WireMessage::HandshakeAck { .. } => MessageType::HandshakeAck,
            WireMessage::Ping(_) => MessageType::Ping,
            WireMessage::Pong(_) => MessageType::Pong,
            WireMessage::GossipTransaction(_) => MessageType::GossipTransaction,
            WireMessage::GossipBlock(_) => MessageType::GossipBlock,
            WireMessage::GossipVote(_) => MessageType::GossipVote,
            WireMessage::GetBlocks { .. } => MessageType::GetBlocks,
            WireMessage::Blocks(_) => MessageType::Blocks,
            WireMessage::Disconnect { .. } => MessageType::Disconnect,
        }
    }

    pub fn encode_payload(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            WireMessage::Handshake(payload) => payload.serialize_canonical(&mut buf),
            WireMessage::HandshakeAck { success, message } => {
                buf.push(if *success { 1 } else { 0 });
                let msg_bytes = message.as_bytes();
                buf.extend_from_slice(&(msg_bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(msg_bytes);
            }
            WireMessage::Ping(nonce) => buf.extend_from_slice(&nonce.to_le_bytes()),
            WireMessage::Pong(nonce) => buf.extend_from_slice(&nonce.to_le_bytes()),
            WireMessage::GossipTransaction(tx) => tx.serialize_canonical(&mut buf),
            WireMessage::GossipBlock(block) => block.serialize_canonical(&mut buf),
            WireMessage::GossipVote(vote) => vote.serialize_canonical(&mut buf),
            WireMessage::GetBlocks {
                start_height,
                max_count,
            } => {
                start_height.serialize_canonical(&mut buf);
                buf.extend_from_slice(&max_count.to_le_bytes());
            }
            WireMessage::Blocks(blocks) => {
                buf.extend_from_slice(&(blocks.len() as u32).to_le_bytes());
                for b in blocks {
                    b.serialize_canonical(&mut buf);
                }
            }
            WireMessage::Disconnect { reason, message } => {
                buf.push(*reason as u8);
                let msg_bytes = message.as_bytes();
                buf.extend_from_slice(&(msg_bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(msg_bytes);
            }
        }
        buf
    }

    pub fn decode_payload(msg_type: MessageType, payload: &[u8]) -> Result<Self, NetworkError> {
        let mut reader = ByteReader::new(payload);
        let res = match msg_type {
            MessageType::Handshake => {
                let p = HandshakePayload::read_canonical(&mut reader)
                    .map_err(|e| NetworkError::MalformedMessage(e.to_string()))?;
                WireMessage::Handshake(p)
            }
            MessageType::HandshakeAck => {
                let success = reader
                    .read_u8()
                    .map_err(|e| NetworkError::MalformedMessage(e.to_string()))?
                    != 0;
                let bytes = reader
                    .read_vec(512)
                    .map_err(|e| NetworkError::MalformedMessage(e.to_string()))?;
                let message = String::from_utf8(bytes)
                    .map_err(|_| NetworkError::MalformedMessage("Ack message not UTF-8".into()))?;
                WireMessage::HandshakeAck { success, message }
            }
            MessageType::Ping => {
                let nonce = reader
                    .read_u64_le()
                    .map_err(|e| NetworkError::MalformedMessage(e.to_string()))?;
                WireMessage::Ping(nonce)
            }
            MessageType::Pong => {
                let nonce = reader
                    .read_u64_le()
                    .map_err(|e| NetworkError::MalformedMessage(e.to_string()))?;
                WireMessage::Pong(nonce)
            }
            MessageType::GossipTransaction => {
                let tx = Transaction::read_canonical(&mut reader)
                    .map_err(|e| NetworkError::MalformedMessage(e.to_string()))?;
                WireMessage::GossipTransaction(tx)
            }
            MessageType::GossipBlock => {
                let block = Block::read_canonical(&mut reader)
                    .map_err(|e| NetworkError::MalformedMessage(e.to_string()))?;
                WireMessage::GossipBlock(block)
            }
            MessageType::GossipVote => {
                let vote = Vote::read_canonical(&mut reader)
                    .map_err(|e| NetworkError::MalformedMessage(e.to_string()))?;
                WireMessage::GossipVote(vote)
            }
            MessageType::GetBlocks => {
                let start_height = BlockHeight::read_canonical(&mut reader)
                    .map_err(|e| NetworkError::MalformedMessage(e.to_string()))?;
                let max_count = reader
                    .read_u32_le()
                    .map_err(|e| NetworkError::MalformedMessage(e.to_string()))?;
                WireMessage::GetBlocks {
                    start_height,
                    max_count,
                }
            }
            MessageType::Blocks => {
                let count = reader
                    .read_u32_le()
                    .map_err(|e| NetworkError::MalformedMessage(e.to_string()))?
                    as usize;
                if count > 2048 {
                    return Err(NetworkError::MalformedMessage(
                        "GetBlocks returned > 2048 blocks".into(),
                    ));
                }
                let mut blocks = Vec::with_capacity(count);
                for _ in 0..count {
                    blocks.push(
                        Block::read_canonical(&mut reader)
                            .map_err(|e| NetworkError::MalformedMessage(e.to_string()))?,
                    );
                }
                WireMessage::Blocks(blocks)
            }
            MessageType::Disconnect => {
                let reason_code = reader
                    .read_u8()
                    .map_err(|e| NetworkError::MalformedMessage(e.to_string()))?;
                let bytes = reader
                    .read_vec(512)
                    .map_err(|e| NetworkError::MalformedMessage(e.to_string()))?;
                let message = String::from_utf8(bytes).map_err(|_| {
                    NetworkError::MalformedMessage("Disconnect message not UTF-8".into())
                })?;
                WireMessage::Disconnect {
                    reason: DisconnectReason::from_u8(reason_code),
                    message,
                }
            }
        };

        reader
            .ensure_consumed()
            .map_err(|_| NetworkError::MalformedMessage("Trailing bytes in wire payload".into()))?;

        Ok(res)
    }
}

/// Serializes a complete binary wire frame with header and BLAKE3 checksum.
pub fn encode_frame(network_id: NetworkId, message: &WireMessage) -> Vec<u8> {
    let payload = message.encode_payload();
    let payload_len = payload.len() as u32;
    let checksum = blake3_hash(&payload);

    let mut frame = Vec::with_capacity(FRAME_HEADER_SIZE + payload.len());
    frame.extend_from_slice(&PROTOCOL_MAGIC);
    frame.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    frame.push(network_id as u8);
    frame.push(message.message_type() as u8);
    frame.extend_from_slice(&payload_len.to_le_bytes());
    frame.extend_from_slice(&checksum.0);
    frame.extend_from_slice(&payload);

    frame
}

/// Decodes and validates a binary wire frame, verifying magic, checksum, and network ID.
pub fn decode_frame(expected_net: NetworkId, frame: &[u8]) -> Result<WireMessage, NetworkError> {
    if frame.len() < FRAME_HEADER_SIZE {
        return Err(NetworkError::MalformedMessage(
            "Frame smaller than header".into(),
        ));
    }

    if frame[0..4] != PROTOCOL_MAGIC {
        return Err(NetworkError::InvalidMagic);
    }

    let version = u16::from_le_bytes([frame[4], frame[5]]);
    if version != PROTOCOL_VERSION {
        return Err(NetworkError::MalformedMessage(format!(
            "Unsupported wire version: {version}"
        )));
    }

    let net_id = NetworkId::from_u8(frame[6])?;
    if net_id != expected_net {
        return Err(NetworkError::NetworkMismatch);
    }

    let msg_type = MessageType::from_u8(frame[7])?;
    let payload_len = u32::from_le_bytes([frame[8], frame[9], frame[10], frame[11]]) as usize;

    if payload_len > MAX_PAYLOAD_SIZE {
        return Err(NetworkError::MessageTooLarge);
    }

    if frame.len() != FRAME_HEADER_SIZE + payload_len {
        return Err(NetworkError::MalformedMessage(format!(
            "Frame size mismatch: expected {}, got {}",
            FRAME_HEADER_SIZE + payload_len,
            frame.len()
        )));
    }

    let mut checksum = [0u8; 32];
    checksum.copy_from_slice(&frame[12..44]);

    let payload = &frame[44..];
    let computed_checksum = blake3_hash(payload);

    if computed_checksum.0 != checksum {
        return Err(NetworkError::ChecksumMismatch);
    }

    WireMessage::decode_payload(msg_type, payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wire_handshake_roundtrip() {
        let handshake = HandshakePayload {
            network_id: NetworkId::Mainnet,
            genesis_hash: Hash([0xaa; 32]),
            head_height: BlockHeight(1234),
            head_hash: Hash([0xbb; 32]),
            client_version: "tola-node/0.1.0".to_string(),
            listen_port: 9000,
        };

        let msg = WireMessage::Handshake(handshake.clone());
        let frame = encode_frame(NetworkId::Mainnet, &msg);

        let decoded = decode_frame(NetworkId::Mainnet, &frame).expect("Valid frame decode");
        match decoded {
            WireMessage::Handshake(h) => assert_eq!(h, handshake),
            other => panic!("Unexpected message type: {other:?}"),
        }
    }

    #[test]
    fn test_wire_checksum_corruption_detected() {
        let msg = WireMessage::Ping(42);
        let mut frame = encode_frame(NetworkId::Mainnet, &msg);

        // Corrupt a byte in the payload
        let last = frame.len() - 1;
        frame[last] ^= 0xff;

        let err = decode_frame(NetworkId::Mainnet, &frame).unwrap_err();
        assert_eq!(err, NetworkError::ChecksumMismatch);
    }

    #[test]
    fn test_wire_invalid_magic_rejected() {
        let msg = WireMessage::Ping(42);
        let mut frame = encode_frame(NetworkId::Mainnet, &msg);
        frame[0] = 0x00; // Break magic byte

        let err = decode_frame(NetworkId::Mainnet, &frame).unwrap_err();
        assert_eq!(err, NetworkError::InvalidMagic);
    }

    #[test]
    fn test_wire_network_mismatch_rejected() {
        let msg = WireMessage::Ping(42);
        let frame = encode_frame(NetworkId::Devnet, &msg);

        let err = decode_frame(NetworkId::Mainnet, &frame).unwrap_err();
        assert_eq!(err, NetworkError::NetworkMismatch);
    }
}
