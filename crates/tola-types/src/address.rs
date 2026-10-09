//! Address type, Bech32m encoding/decoding, and network representations.
//!
//! Complies with ADR-0006: Versioned Bech32m Checksummed Address Format
//! and BIP-350 (Bech32m constant: 0x2bc830a3).

use core::fmt;
use core::str::FromStr;

/// Canonical 20-byte address (EVM & TOLA compatible).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Address(pub [u8; 20]);

/// Network scope determining the Human-Readable Part (HRP) of the Bech32m address.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Network {
    Mainnet,
    Testnet,
    Devnet,
    Custom(String),
}

impl Network {
    pub fn hrp(&self) -> &str {
        match self {
            Network::Mainnet => "tola",
            Network::Testnet => "ttola",
            Network::Devnet => "dtola",
            Network::Custom(hrp) => hrp.as_str(),
        }
    }

    pub fn from_hrp(hrp: &str) -> Self {
        match hrp {
            "tola" => Network::Mainnet,
            "ttola" => Network::Testnet,
            "dtola" => Network::Devnet,
            other => Network::Custom(other.to_string()),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum AddressError {
    InvalidLength,
    InvalidCharacter(char),
    MixedCase,
    InvalidHrp,
    MissingSeparator,
    InvalidChecksum,
    InvalidPadding,
    HexDecodeError,
}

impl fmt::Display for AddressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AddressError::InvalidLength => write!(f, "Invalid address length"),
            AddressError::InvalidCharacter(c) => write!(f, "Invalid character in address: '{c}'"),
            AddressError::MixedCase => write!(f, "Address cannot have mixed case"),
            AddressError::InvalidHrp => write!(f, "Invalid human-readable part (HRP)"),
            AddressError::MissingSeparator => write!(f, "Missing '1' separator in Bech32m string"),
            AddressError::InvalidChecksum => write!(f, "Invalid Bech32m checksum"),
            AddressError::InvalidPadding => write!(f, "Invalid bit conversion padding"),
            AddressError::HexDecodeError => write!(f, "Failed to parse hexadecimal address"),
        }
    }
}

impl std::error::Error for AddressError {}

const BECH32M_CONST: u32 = 0x2bc830a3;
const CHARSET: &[u8; 32] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";

const GEN: [u32; 5] = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];

fn polymod(values: &[u8]) -> u32 {
    let mut chk: u32 = 1;
    for &val in values {
        let b = (chk >> 25) as u8;
        chk = ((chk & 0x1ffffff) << 5) ^ (val as u32);
        for (i, gen) in GEN.iter().enumerate() {
            if (b >> i) & 1 == 1 {
                chk ^= gen;
            }
        }
    }
    chk
}

fn hrp_expand(hrp: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(hrp.len() * 2 + 1);
    for b in hrp.bytes() {
        v.push(b >> 5);
    }
    v.push(0);
    for b in hrp.bytes() {
        v.push(b & 31);
    }
    v
}

fn convert_bits(
    data: &[u8],
    from_bits: u32,
    to_bits: u32,
    pad: bool,
) -> Result<Vec<u8>, AddressError> {
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let mut ret = Vec::new();
    let maxv: u32 = (1 << to_bits) - 1;
    let max_acc: u32 = (1 << (from_bits + to_bits - 1)) - 1;

    for &value in data {
        if (value as u32) >> from_bits != 0 {
            return Err(AddressError::InvalidPadding);
        }
        acc = ((acc << from_bits) | (value as u32)) & max_acc;
        bits += from_bits;
        while bits >= to_bits {
            bits -= to_bits;
            ret.push(((acc >> bits) & maxv) as u8);
        }
    }

    if pad {
        if bits > 0 {
            ret.push(((acc << (to_bits - bits)) & maxv) as u8);
        }
    } else if bits >= from_bits || ((acc << (to_bits - bits)) & maxv) != 0 {
        return Err(AddressError::InvalidPadding);
    }

    Ok(ret)
}

impl Address {
    pub const ZERO: Self = Self([0u8; 20]);

    /// Canonical Protocol Treasury account (0x2020202020202020202020202020202020202020).
    /// Receives 25% of all gas fee revenue per approved Decisions D01 and D04.
    pub const PROTOCOL_TREASURY: Self = Self([0x20; 20]);

    /// Canonical Development Reserve account (0x2121212121212121212121212121212121212121).
    /// Operational development budget allocated at genesis per Decision D01.
    pub const DEVELOPMENT_RESERVE: Self = Self([0x21; 20]);

    /// Canonical Validator Reward Pool account (0x2525252525252525252525252525252525252525).
    /// Receives 75% of all gas fee revenue per approved Decisions D01 and D04,
    /// settled daily to active validators based on 80% stake weight and 20% performance score.
    pub const VALIDATOR_REWARD_POOL: Self = Self([0x25; 20]);

    pub const fn from_bytes(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    /// Encode address into canonical Bech32m string for given network.
    pub fn to_bech32m(&self, network: &Network) -> String {
        let hrp = network.hrp();
        let data_5bit = convert_bits(&self.0, 8, 5, true)
            .expect("20 bytes conversion into 5-bit always succeeds");

        let mut values = hrp_expand(hrp);
        values.extend_from_slice(&data_5bit);
        values.extend_from_slice(&[0u8; 6]);

        let polymod_result = polymod(&values) ^ BECH32M_CONST;

        let mut checksum = [0u8; 6];
        for (i, cs) in checksum.iter_mut().enumerate() {
            *cs = ((polymod_result >> (5 * (5 - i))) & 31) as u8;
        }

        let mut out = String::with_capacity(hrp.len() + 1 + data_5bit.len() + 6);
        out.push_str(hrp);
        out.push('1');
        for &d in &data_5bit {
            out.push(CHARSET[d as usize] as char);
        }
        for &c in &checksum {
            out.push(CHARSET[c as usize] as char);
        }

        out
    }

    /// Decode address and detected network from a Bech32m string.
    pub fn from_bech32m(s: &str) -> Result<(Network, Self), AddressError> {
        if s.len() < 8 || s.len() > 90 {
            return Err(AddressError::InvalidLength);
        }

        let has_lower = s.chars().any(|c| c.is_ascii_lowercase());
        let has_upper = s.chars().any(|c| c.is_ascii_uppercase());
        if has_lower && has_upper {
            return Err(AddressError::MixedCase);
        }

        let s_lower = s.to_ascii_lowercase();
        let sep_idx = s_lower
            .rfind('1')
            .ok_prefix_err(AddressError::MissingSeparator)?;

        if sep_idx == 0 || sep_idx + 7 > s_lower.len() {
            return Err(AddressError::InvalidLength);
        }

        let hrp = &s_lower[..sep_idx];
        for b in hrp.bytes() {
            if !(33..=126).contains(&b) {
                return Err(AddressError::InvalidHrp);
            }
        }

        let data_part = &s_lower[sep_idx + 1..];
        let mut values_5bit = Vec::with_capacity(data_part.len());
        for c in data_part.chars() {
            let val = CHARSET
                .iter()
                .position(|&b| b == c as u8)
                .ok_or(AddressError::InvalidCharacter(c))?;
            values_5bit.push(val as u8);
        }

        let mut polymod_values = hrp_expand(hrp);
        polymod_values.extend_from_slice(&values_5bit);

        if polymod(&polymod_values) != BECH32M_CONST {
            return Err(AddressError::InvalidChecksum);
        }

        let payload_5bit = &values_5bit[..values_5bit.len() - 6];
        let payload_8bit = convert_bits(payload_5bit, 5, 8, false)?;

        if payload_8bit.len() != 20 {
            return Err(AddressError::InvalidLength);
        }

        let mut addr = [0u8; 20];
        addr.copy_from_slice(&payload_8bit);

        let network = Network::from_hrp(hrp);
        Ok((network, Address(addr)))
    }

    /// Encode address as lowercase hex with '0x' prefix.
    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(42);
        s.push_str("0x");
        for b in &self.0 {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    /// Decode address from hex string (with or without '0x' prefix).
    pub fn from_hex(s: &str) -> Result<Self, AddressError> {
        let clean = s
            .strip_prefix("0x")
            .or_else(|| s.strip_prefix("0X"))
            .unwrap_or(s);
        if clean.len() != 40 {
            return Err(AddressError::InvalidLength);
        }

        let mut bytes = [0u8; 20];
        for i in 0..20 {
            let byte_str = &clean[i * 2..i * 2 + 2];
            bytes[i] =
                u8::from_str_radix(byte_str, 16).map_err(|_| AddressError::HexDecodeError)?;
        }
        Ok(Address(bytes))
    }
}

trait OptionExt<T> {
    fn ok_prefix_err(self, err: AddressError) -> Result<T, AddressError>;
}

impl<T> OptionExt<T> for Option<T> {
    fn ok_prefix_err(self, err: AddressError) -> Result<T, AddressError> {
        self.ok_or(err)
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Address({})", self.to_hex())
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_bech32m(&Network::Mainnet))
    }
}

impl FromStr for Address {
    type Err = AddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("0x")
            || s.starts_with("0X")
            || (s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit()))
        {
            Address::from_hex(s)
        } else {
            let (_, addr) = Address::from_bech32m(s)?;
            Ok(addr)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bech32m_roundtrip_all_networks() {
        let addr = Address([
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
            0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc,
        ]);

        for network in &[Network::Mainnet, Network::Testnet, Network::Devnet] {
            let encoded = addr.to_bech32m(network);
            assert!(encoded.starts_with(network.hrp()));
            assert!(encoded.contains('1'));

            let (decoded_net, decoded_addr) =
                Address::from_bech32m(&encoded).expect("Valid decode");
            assert_eq!(&decoded_net, network);
            assert_eq!(decoded_addr, addr);
        }
    }

    #[test]
    fn test_bech32m_checksum_corruption_detection() {
        let addr = Address([0x42; 20]);
        let mut encoded = addr.to_bech32m(&Network::Mainnet);

        // Mutate a single character in the checksum
        let last_char = encoded.pop().unwrap();
        let corrupted_char = if last_char == 'p' { 'q' } else { 'p' };
        encoded.push(corrupted_char);

        let err = Address::from_bech32m(&encoded).unwrap_err();
        assert_eq!(err, AddressError::InvalidChecksum);
    }

    #[test]
    fn test_bech32m_mixed_case_rejected() {
        let addr = Address([0x01; 20]);
        let encoded = addr.to_bech32m(&Network::Mainnet);
        let mixed = format!("To{}", &encoded[2..]);

        let err = Address::from_bech32m(&mixed).unwrap_err();
        assert_eq!(err, AddressError::MixedCase);
    }

    #[test]
    fn test_hex_conversion_roundtrip() {
        let addr = Address([0xfe; 20]);
        let hex_str = addr.to_hex();
        assert_eq!(hex_str, "0xfefefefefefefefefefefefefefefefefefefefe");

        let parsed = Address::from_hex(&hex_str).expect("Valid hex parse");
        assert_eq!(parsed, addr);

        let parsed_no_prefix =
            Address::from_hex(&hex_str[2..]).expect("Valid hex parse without 0x");
        assert_eq!(parsed_no_prefix, addr);
    }

    #[test]
    fn test_from_str_trait() {
        let addr = Address([0x7a; 20]);
        let bech32_str = addr.to_bech32m(&Network::Mainnet);
        let hex_str = addr.to_hex();

        let parsed1: Address = bech32_str.parse().expect("Bech32m parse via FromStr");
        assert_eq!(parsed1, addr);

        let parsed2: Address = hex_str.parse().expect("Hex parse via FromStr");
        assert_eq!(parsed2, addr);
    }
}
