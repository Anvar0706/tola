//! RFC 6455 WebSocket framing, handshake, and protocol engine for TOLA JSON-RPC.

use std::fmt;

pub const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
pub const MAX_HANDSHAKE_HEADER_SIZE: usize = 4096;
pub const MAX_MESSAGE_SIZE: usize = 65536; // 64 KB

// ============================================================================
// RFC 3174 SHA-1 & Base64 Self-Contained Implementation
// ============================================================================

/// Computes SHA-1 hash (RFC 3174) for WebSocket handshake calculation.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes raw bytes to a standard Base64 string.
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        let idx0 = (b0 >> 2) as usize;
        let idx1 = (((b0 & 0x03) << 4) | (b1 >> 4)) as usize;
        let idx2 = (((b1 & 0x0F) << 2) | (b2 >> 6)) as usize;
        let idx3 = (b2 & 0x3F) as usize;

        out.push(B64_ALPHABET[idx0] as char);
        out.push(B64_ALPHABET[idx1] as char);
        if chunk.len() > 1 {
            out.push(B64_ALPHABET[idx2] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_ALPHABET[idx3] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Computes the RFC 6455 `Sec-WebSocket-Accept` header response value.
pub fn compute_websocket_accept(key: &str) -> String {
    let mut input = Vec::with_capacity(key.len() + WS_GUID.len());
    input.extend_from_slice(key.trim().as_bytes());
    input.extend_from_slice(WS_GUID.as_bytes());
    let digest = sha1(&input);
    base64_encode(&digest)
}

// ============================================================================
// RFC 6455 Frame Protocol
// ============================================================================

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum WsOpcode {
    Continuation = 0x0,
    Text = 0x1,
    Binary = 0x2,
    Close = 0x8,
    Ping = 0x9,
    Pong = 0xA,
}

impl WsOpcode {
    pub fn from_u8(b: u8) -> Result<Self, WsError> {
        match b {
            0x0 => Ok(WsOpcode::Continuation),
            0x1 => Ok(WsOpcode::Text),
            0x2 => Ok(WsOpcode::Binary),
            0x8 => Ok(WsOpcode::Close),
            0x9 => Ok(WsOpcode::Ping),
            0xA => Ok(WsOpcode::Pong),
            other => Err(WsError::ProtocolError(format!(
                "Invalid reserved opcode 0x{:x}",
                other
            ))),
        }
    }

    pub fn is_control(&self) -> bool {
        matches!(self, WsOpcode::Close | WsOpcode::Ping | WsOpcode::Pong)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsFrame {
    pub fin: bool,
    pub opcode: WsOpcode,
    pub payload: Vec<u8>,
}

impl WsFrame {
    pub fn text(payload: String) -> Self {
        Self {
            fin: true,
            opcode: WsOpcode::Text,
            payload: payload.into_bytes(),
        }
    }

    pub fn ping(payload: Vec<u8>) -> Self {
        Self {
            fin: true,
            opcode: WsOpcode::Ping,
            payload,
        }
    }

    pub fn pong(payload: Vec<u8>) -> Self {
        Self {
            fin: true,
            opcode: WsOpcode::Pong,
            payload,
        }
    }

    pub fn close(code: u16, reason: &str) -> Self {
        let mut payload = Vec::with_capacity(2 + reason.len());
        payload.extend_from_slice(&code.to_be_bytes());
        payload.extend_from_slice(reason.as_bytes());
        Self {
            fin: true,
            opcode: WsOpcode::Close,
            payload,
        }
    }

    /// Encodes a server-to-client frame (MASK = 0 according to RFC 6455 §5.1).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(10 + self.payload.len());
        let fin_bit = if self.fin { 0x80 } else { 0x00 };
        out.push(fin_bit | (self.opcode as u8));

        let len = self.payload.len();
        if len <= 125 {
            out.push(len as u8); // Mask bit is 0
        } else if len <= 65535 {
            out.push(126);
            out.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            out.push(127);
            out.extend_from_slice(&(len as u64).to_be_bytes());
        }

        out.extend_from_slice(&self.payload);
        out
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum WsError {
    ProtocolError(String),   // Close 1002
    UnsupportedData(String), // Close 1003
    InvalidPayload(String),  // Close 1007
    PolicyViolation(String), // Close 1008
    MessageTooBig(String),   // Close 1009
    IncompleteHeader,
}

impl fmt::Display for WsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WsError::ProtocolError(s) => write!(f, "Protocol Error (1002): {}", s),
            WsError::UnsupportedData(s) => write!(f, "Unsupported Data (1003): {}", s),
            WsError::InvalidPayload(s) => write!(f, "Invalid Payload (1007): {}", s),
            WsError::PolicyViolation(s) => write!(f, "Policy Violation (1008): {}", s),
            WsError::MessageTooBig(s) => write!(f, "Message Too Big (1009): {}", s),
            WsError::IncompleteHeader => write!(f, "Incomplete Header"),
        }
    }
}

impl WsError {
    pub fn close_code(&self) -> u16 {
        match self {
            WsError::ProtocolError(_) => 1002,
            WsError::UnsupportedData(_) => 1003,
            WsError::InvalidPayload(_) => 1007,
            WsError::PolicyViolation(_) => 1008,
            WsError::MessageTooBig(_) => 1009,
            WsError::IncompleteHeader => 1002,
        }
    }
}

/// Streaming RFC 6455 frame decoder capable of consuming arbitrary TCP chunk slices.
pub struct WsFrameDecoder {
    buffer: Vec<u8>,
}

impl Default for WsFrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl WsFrameDecoder {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    pub fn with_initial_bytes(initial: &[u8]) -> Self {
        Self {
            buffer: initial.to_vec(),
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
    }

    pub fn unconsumed_bytes(&self) -> usize {
        self.buffer.len()
    }

    /// Attempts to decode the next frame from the buffer.
    /// Returns:
    /// - `Ok(Some(frame))` if a complete valid frame was parsed.
    /// - `Ok(None)` if more TCP bytes are needed.
    /// - `Err(WsError)` if an RFC 6455 protocol error occurred.
    pub fn decode_next(&mut self, max_payload_size: usize) -> Result<Option<WsFrame>, WsError> {
        if self.buffer.len() < 2 {
            return Ok(None);
        }

        let b0 = self.buffer[0];
        let b1 = self.buffer[1];

        let fin = (b0 & 0x80) != 0;
        let rsv = b0 & 0x70;
        if rsv != 0 {
            return Err(WsError::ProtocolError(
                "Reserved bits RSV1/2/3 must be zero".into(),
            ));
        }

        let opcode = WsOpcode::from_u8(b0 & 0x0F)?;
        let masked = (b1 & 0x80) != 0;

        // RFC 6455 §5.1: Client must mask all frames.
        if !masked {
            return Err(WsError::ProtocolError(
                "Client-to-server frame must be masked".into(),
            ));
        }

        let len_code = (b1 & 0x7F) as usize;
        let mut offset = 2;

        let payload_len: usize = if len_code <= 125 {
            len_code
        } else if len_code == 126 {
            if self.buffer.len() < 4 {
                return Ok(None);
            }
            let len = u16::from_be_bytes([self.buffer[2], self.buffer[3]]) as usize;
            offset += 2;
            len
        } else {
            // len_code == 127
            if self.buffer.len() < 10 {
                return Ok(None);
            }
            let mut len_bytes = [0u8; 8];
            len_bytes.copy_from_slice(&self.buffer[2..10]);
            let len_u64 = u64::from_be_bytes(len_bytes);
            if len_u64 > (max_payload_size as u64) {
                return Err(WsError::MessageTooBig(format!(
                    "Frame payload length {} exceeds maximum {}",
                    len_u64, max_payload_size
                )));
            }
            offset += 8;
            len_u64 as usize
        };

        if payload_len > max_payload_size {
            return Err(WsError::MessageTooBig(format!(
                "Payload length {} exceeds maximum {}",
                payload_len, max_payload_size
            )));
        }

        // Control frame restrictions RFC 6455 §5.5
        if opcode.is_control() {
            if !fin {
                return Err(WsError::ProtocolError(
                    "Control frames cannot be fragmented (FIN must be 1)".into(),
                ));
            }
            if payload_len > 125 {
                return Err(WsError::ProtocolError(
                    "Control frame payload cannot exceed 125 bytes".into(),
                ));
            }
        }

        let total_frame_len = offset + 4 + payload_len; // offset + mask (4) + payload
        if self.buffer.len() < total_frame_len {
            return Ok(None);
        }

        let mask = [
            self.buffer[offset],
            self.buffer[offset + 1],
            self.buffer[offset + 2],
            self.buffer[offset + 3],
        ];
        offset += 4;

        let mut payload = self.buffer[offset..offset + payload_len].to_vec();
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[i % 4];
        }

        // Remove consumed frame bytes from buffer
        self.buffer.drain(0..total_frame_len);

        Ok(Some(WsFrame {
            fin,
            opcode,
            payload,
        }))
    }
}

/// Message assembler collecting fragmented text frames up to MAX_MESSAGE_SIZE.
pub struct WsMessageAssembler {
    active_opcode: Option<WsOpcode>,
    payload_buffer: Vec<u8>,
}

impl Default for WsMessageAssembler {
    fn default() -> Self {
        Self::new()
    }
}

impl WsMessageAssembler {
    pub fn new() -> Self {
        Self {
            active_opcode: None,
            payload_buffer: Vec::new(),
        }
    }

    /// Ingests a frame. If a complete message is ready, returns `Ok(Some((opcode, payload)))`.
    pub fn process_frame(
        &mut self,
        frame: WsFrame,
        max_message_size: usize,
    ) -> Result<Option<(WsOpcode, Vec<u8>)>, WsError> {
        // Control frames are processed immediately without disturbing message assembly
        if frame.opcode.is_control() {
            return Ok(Some((frame.opcode, frame.payload)));
        }

        if frame.opcode == WsOpcode::Binary {
            return Err(WsError::UnsupportedData(
                "Binary WebSocket frames are not supported for JSON-RPC".into(),
            ));
        }

        if frame.opcode == WsOpcode::Text {
            if self.active_opcode.is_some() {
                return Err(WsError::ProtocolError(
                    "Received new Text frame while previous message uncompleted".into(),
                ));
            }

            if frame.fin {
                // Complete unfragmented text message
                return Ok(Some((WsOpcode::Text, frame.payload)));
            } else {
                // Start of fragmented message
                self.active_opcode = Some(WsOpcode::Text);
                self.payload_buffer = frame.payload;
                return Ok(None);
            }
        }

        if frame.opcode == WsOpcode::Continuation {
            let active = self.active_opcode.ok_or_else(|| {
                WsError::ProtocolError("Unexpected continuation frame without initial frame".into())
            })?;

            if self.payload_buffer.len() + frame.payload.len() > max_message_size {
                return Err(WsError::MessageTooBig(format!(
                    "Defragmented message exceeds maximum allowed {}",
                    max_message_size
                )));
            }

            self.payload_buffer.extend_from_slice(&frame.payload);

            if frame.fin {
                self.active_opcode = None;
                let payload = std::mem::take(&mut self.payload_buffer);
                return Ok(Some((active, payload)));
            } else {
                return Ok(None);
            }
        }

        Err(WsError::ProtocolError("Unrecognized frame state".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rfc6455_sec_websocket_accept_canonical_vector() {
        // Canonical RFC 6455 §1.3 test vector:
        // Key: "dGhlIHNhbXBsZSBub25jZQ=="
        // Expected Accept: "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let accept = compute_websocket_accept(key);
        assert_eq!(accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn test_client_masked_text_frame_decoding() {
        let mut decoder = WsFrameDecoder::new();
        // Client frame: Text, FIN=1, MASK=1, payload "hello" (5 bytes)
        let mask = [0x12, 0x34, 0x56, 0x78];
        let hello = b"hello";
        let masked_payload: Vec<u8> = hello
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ mask[i % 4])
            .collect();

        let mut wire = vec![0x81, 0x85]; // 0x81: FIN + Text, 0x85: MASK + len 5
        wire.extend_from_slice(&mask);
        wire.extend_from_slice(&masked_payload);

        decoder.feed(&wire);
        let frame = decoder.decode_next(MAX_MESSAGE_SIZE).unwrap().unwrap();
        assert_eq!(frame.fin, true);
        assert_eq!(frame.opcode, WsOpcode::Text);
        assert_eq!(frame.payload, b"hello");
    }

    #[test]
    fn test_unmasked_client_frame_rejected() {
        let mut decoder = WsFrameDecoder::new();
        let wire = vec![0x81, 0x05, b'h', b'e', b'l', b'l', b'o']; // MASK bit = 0
        decoder.feed(&wire);
        let err = decoder.decode_next(MAX_MESSAGE_SIZE).unwrap_err();
        assert_eq!(err.close_code(), 1002);
    }

    #[test]
    fn test_fragmented_text_message_assembly() {
        let mut decoder = WsFrameDecoder::new();
        let mut assembler = WsMessageAssembler::new();
        let mask = [0x01, 0x02, 0x03, 0x04];

        // Frame 1: Text, FIN=0, payload "hel"
        let f1_data = b"hel";
        let mut wire1 = vec![0x01, 0x83];
        wire1.extend_from_slice(&mask);
        wire1.extend(f1_data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));

        // Frame 2: Continuation, FIN=1, payload "lo"
        let f2_data = b"lo";
        let mut wire2 = vec![0x80, 0x82];
        wire2.extend_from_slice(&mask);
        wire2.extend(f2_data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));

        decoder.feed(&wire1);
        let frame1 = decoder.decode_next(MAX_MESSAGE_SIZE).unwrap().unwrap();
        let res1 = assembler.process_frame(frame1, MAX_MESSAGE_SIZE).unwrap();
        assert!(res1.is_none());

        decoder.feed(&wire2);
        let frame2 = decoder.decode_next(MAX_MESSAGE_SIZE).unwrap().unwrap();
        let res2 = assembler.process_frame(frame2, MAX_MESSAGE_SIZE).unwrap();
        assert!(res2.is_some());
        let (opcode, payload) = res2.unwrap();
        assert_eq!(opcode, WsOpcode::Text);
        assert_eq!(String::from_utf8(payload).unwrap(), "hello");
    }

    #[test]
    fn test_ping_pong_control_frame() {
        let ping_payload = b"ping123".to_vec();
        let frame = WsFrame::ping(ping_payload.clone());
        assert_eq!(frame.opcode, WsOpcode::Ping);
        let encoded = frame.encode();
        // Server encoded frame should have MASK=0
        assert_eq!(encoded[0], 0x89);
        assert_eq!(encoded[1], 7);
        assert_eq!(&encoded[2..], b"ping123");
    }
}
