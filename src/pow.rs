use num_bigint::BigUint;
use num_traits::Zero;
use sha2::{Digest, Sha256};

/// Compact proof-of-work target, mirroring Python `Target`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub bits: u32,
}

impl Target {
    /// Convert compact bits to full target integer (Bitcoin-style).
    pub fn to_target(self) -> BigUint {
        let exponent = (self.bits >> 24) as u32;
        let mantissa = (self.bits & 0x00ff_ffff) as u32;
        let mut value = BigUint::from(mantissa);
        if exponent <= 3 {
            let shift = 8 * (3 - exponent);
            value >>= shift;
        } else {
            let shift = 8 * (exponent - 3);
            value <<= shift;
        }
        value
    }

    /// Construct a compact target from a full integer, mirroring Python `Target.from_target`.
    pub fn from_target(value: &BigUint) -> Target {
        if value.is_zero() {
            return Target { bits: 0 };
        }

        // Number of bytes required to represent the value (big-endian, no leading zeros).
        let value_bytes = value.to_bytes_be();
        let mut exponent = value_bytes.len() as u32;

        // Normalize so that mantissa fits in 3 bytes, like Bitcoin-style compact format.
        let mantissa_big = if exponent <= 3 {
            let shift_bytes = 3 - exponent;
            value << (8 * shift_bytes)
        } else {
            let shift_bytes = exponent - 3;
            value >> (8 * shift_bytes)
        };

        let mut mantissa_bytes = mantissa_big.to_bytes_be();
        if mantissa_bytes.len() > 3 {
            // Keep the most significant 3 bytes.
            let start = mantissa_bytes.len() - 3;
            mantissa_bytes = mantissa_bytes[start..].to_vec();
        } else if mantissa_bytes.len() < 3 {
            // Left-pad with zeros to exactly 3 bytes.
            let mut padded = vec![0u8; 3 - mantissa_bytes.len()];
            padded.extend_from_slice(&mantissa_bytes);
            mantissa_bytes = padded;
        }

        let mut mantissa = ((mantissa_bytes[0] as u32) << 16)
            | ((mantissa_bytes[1] as u32) << 8)
            | (mantissa_bytes[2] as u32);

        // If mantissa's top bit is set, shift down and bump exponent, matching Python's logic.
        if mantissa & 0x0080_0000 != 0 {
            mantissa >>= 8;
            exponent += 1;
        }

        let bits = (exponent << 24) | (mantissa & 0x00ff_ffff);
        Target { bits }
    }
}

pub fn sha256d(data: &[u8]) -> [u8; 32] {
    let first = Sha256::digest(data);
    let second = Sha256::digest(&first);
    let mut out = [0u8; 32];
    out.copy_from_slice(&second);
    out
}

pub fn header_hash(parts: &[&[u8]]) -> [u8; 32] {
    let mut buf = Vec::new();
    for p in parts {
        buf.extend_from_slice(p);
    }
    sha256d(&buf)
}

pub fn meets_target(hash: &[u8; 32], target: Target) -> bool {
    let value = BigUint::from_bytes_be(hash);
    value <= target.to_target()
}
