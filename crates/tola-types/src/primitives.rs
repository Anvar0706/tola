//! Canonical primitive types and scalar arithmetic for TOLA Network.

use core::fmt;
use core::ops::{Add, Sub};
use core::str::FromStr;

// ============================================================================
// 1. Hash (32 bytes)
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Hash(pub [u8; 32]);

impl Hash {
    pub const ZERO: Self = Self([0u8; 32]);

    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(66);
        s.push_str("0x");
        for b in &self.0 {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    pub fn from_hex(s: &str) -> Result<Self, String> {
        let clean = s
            .strip_prefix("0x")
            .or_else(|| s.strip_prefix("0X"))
            .unwrap_or(s);
        if clean.len() != 64 {
            return Err("Hash hex string must be 64 characters long".into());
        }
        let mut arr = [0u8; 32];
        for i in 0..32 {
            let byte_str = &clean[i * 2..i * 2 + 2];
            arr[i] = u8::from_str_radix(byte_str, 16).map_err(|e| e.to_string())?;
        }
        Ok(Hash(arr))
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({})", self.to_hex())
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl FromStr for Hash {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Hash::from_hex(s)
    }
}

// ============================================================================
// 2. Amount (128-bit unsigned integer with 18 decimal places)
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Amount(pub u128);

impl Amount {
    pub const ZERO: Self = Self(0);
    pub const ONE_TOLA: u128 = 1_000_000_000_000_000_000; // 10^18 base units

    pub const fn from_u128(val: u128) -> Self {
        Self(val)
    }

    pub fn as_u128(&self) -> u128 {
        self.0
    }

    /// Construct amount from whole TOLA coins (multiplied by 10^18).
    pub fn from_tola(coins: u64) -> Option<Self> {
        (coins as u128).checked_mul(Self::ONE_TOLA).map(Amount)
    }

    pub fn checked_add(&self, other: Amount) -> Option<Amount> {
        self.0.checked_add(other.0).map(Amount)
    }

    pub fn checked_sub(&self, other: Amount) -> Option<Amount> {
        self.0.checked_sub(other.0).map(Amount)
    }

    pub fn checked_mul(&self, factor: u64) -> Option<Amount> {
        self.0.checked_mul(factor as u128).map(Amount)
    }

    pub fn saturating_add(&self, other: Amount) -> Amount {
        Amount(self.0.saturating_add(other.0))
    }

    pub fn saturating_sub(&self, other: Amount) -> Amount {
        Amount(self.0.saturating_sub(other.0))
    }
}

impl Add for Amount {
    type Output = Amount;
    fn add(self, rhs: Self) -> Self::Output {
        self.checked_add(rhs).expect("Amount overflow")
    }
}

impl Sub for Amount {
    type Output = Amount;
    fn sub(self, rhs: Self) -> Self::Output {
        self.checked_sub(rhs).expect("Amount underflow")
    }
}

impl fmt::Debug for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Amount({} raw)", self.0)
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0 / Self::ONE_TOLA;
        let frac = self.0 % Self::ONE_TOLA;
        if frac == 0 {
            write!(f, "{} TOLA", whole)
        } else {
            write!(f, "{}.{:018} TOLA", whole, frac)
        }
    }
}

// ============================================================================
// 3. Nonce (64-bit strictly monotonic account counter)
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Nonce(pub u64);

impl Nonce {
    pub const ZERO: Self = Self(0);

    pub const fn new(val: u64) -> Self {
        Self(val)
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }

    pub fn next(&self) -> Self {
        Self(self.0.checked_add(1).expect("Nonce overflow"))
    }

    pub fn increment(&mut self) {
        self.0 = self.0.checked_add(1).expect("Nonce overflow");
    }

    pub fn is_next_of(&self, previous: Nonce) -> bool {
        self.0 == previous.0.saturating_add(1)
    }
}

impl fmt::Debug for Nonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Nonce({})", self.0)
    }
}

impl fmt::Display for Nonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ============================================================================
// 4. Gas (64-bit compute / storage gas metering)
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Gas(pub u64);

impl Gas {
    pub const ZERO: Self = Self(0);
    pub const MIN_TRANSACTION_GAS: Gas = Gas(21_000);
    pub const MAX_BLOCK_GAS: Gas = Gas(30_000_000);

    pub const fn new(val: u64) -> Self {
        Self(val)
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }

    pub fn checked_add(&self, other: Gas) -> Option<Gas> {
        self.0.checked_add(other.0).map(Gas)
    }

    pub fn checked_sub(&self, other: Gas) -> Option<Gas> {
        self.0.checked_sub(other.0).map(Gas)
    }
}

impl fmt::Debug for Gas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Gas({})", self.0)
    }
}

impl fmt::Display for Gas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ============================================================================
// 5. Blockchain Core Identifiers & Metrics
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ChainId(pub u64);

impl ChainId {
    pub const MAINNET: Self = Self(9999);
    pub const TESTNET: Self = Self(102);
    pub const DEVNET: Self = Self(1337);
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct BlockHeight(pub u64);

impl BlockHeight {
    pub const fn new(val: u64) -> Self {
        Self(val)
    }
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct Epoch(pub u64);

impl Epoch {
    pub const fn new(val: u64) -> Self {
        Self(val)
    }
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct Round(pub u32);

impl Round {
    pub const fn new(val: u32) -> Self {
        Self(val)
    }
    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
pub struct Timestamp(pub u64);

impl Timestamp {
    pub const fn new(val: u64) -> Self {
        Self(val)
    }
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

// ============================================================================
// 6. Cryptographic Keys & Signatures
// ============================================================================

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PublicKey {
    Ed25519([u8; 32]),
    Secp256k1([u8; 33]),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Signature {
    Ed25519([u8; 64]),
    Secp256k1 {
        r: [u8; 32],
        s: [u8; 32],
        v: u8,
    },
    Secp256k1Eth {
        r: [u8; 32],
        s: [u8; 32],
        y_parity: u8,
        chain_id: u64,
    },
}
