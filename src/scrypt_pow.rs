//! Scrypt Proof-of-Work primitive for the LTC SPV header relay.
//!
//! Litecoin uses scrypt(N=1024, r=1, p=1) over the 80-byte block header,
//! with the SAME header bytes serving as both password AND salt
//! (Litecoin's specific binding — RFC 7914 scrypt accepts independent
//! password and salt but the LTC PoW reuses the header for both inputs).
//! Output is 32 bytes interpreted as a little-endian 256-bit integer,
//! compared against the target in the same byte order as Bitcoin's PoW.
//!
//! Chain linkage on Litecoin is still sha256d — `header.prev_hash`
//! references the sha256d of the parent header, just like Bitcoin.
//! Scrypt is consulted ONLY for the PoW target check.
//!
//! Cost: ~10 ms per call on commodity hardware vs ~1 µs for sha256d.
//! Downstream batch verification will need rayon-parallel evaluation
//! across cores to keep per-block validation budget reasonable; this
//! module is intentionally the single-threaded primitive.
//!
//! No consensus wiring lives here. The LTC SPV header validator hasn't
//! landed yet — this primitive is staged ahead of time so the future
//! activation gate doesn't depend on this code first having to compile.

use crate::pow::{meets_target_btc, Target};
use scrypt::{scrypt as scrypt_kdf, Params};

/// log2(N) for Litecoin's scrypt cost parameter (N = 1024).
const LTC_SCRYPT_LOG_N: u8 = 10;
/// Litecoin scrypt block-size mixing parameter.
const LTC_SCRYPT_R: u32 = 1;
/// Litecoin scrypt parallelisation parameter.
const LTC_SCRYPT_P: u32 = 1;
/// Output length in bytes (256-bit hash).
const LTC_SCRYPT_OUT: usize = 32;

/// Compute Litecoin's scrypt PoW hash for an 80-byte block header.
///
/// The same header bytes are passed as BOTH password and salt — this is
/// Litecoin's specific PoW binding, distinct from generic scrypt-as-KDF
/// usage where password and salt are independent. Returns a 32-byte
/// hash in raw byte order; downstream callers feed it directly into
/// `meets_target_btc`, which interprets it as a little-endian 256-bit
/// integer per the BTC/LTC PoW convention.
pub fn scrypt_hash(header_80_bytes: &[u8; 80]) -> [u8; 32] {
    let params = Params::new(LTC_SCRYPT_LOG_N, LTC_SCRYPT_R, LTC_SCRYPT_P, LTC_SCRYPT_OUT)
        .expect("hardcoded LTC scrypt params are valid");
    let mut out = [0u8; 32];
    scrypt_kdf(header_80_bytes, header_80_bytes, &params, &mut out)
        .expect("32-byte output never overflows scrypt's output buffer");
    out
}

/// Whether a Litecoin block header satisfies its difficulty target.
///
/// Byte-order semantics are identical to `meets_target_btc`: the scrypt
/// result is treated as a little-endian 256-bit integer, mirroring how
/// Bitcoin's sha256d PoW is compared. The only difference between BTC
/// and LTC PoW verification, end to end, is the choice of hash function.
pub fn meets_target_ltc(header_bytes: &[u8; 80], target: Target) -> bool {
    let hash = scrypt_hash(header_bytes);
    meets_target_btc(&hash, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Determinism — identical input must always produce identical output.
    /// Foundational property: if this fails, the scrypt crate is broken
    /// or the parameters are being mutated between calls.
    #[test]
    fn scrypt_hash_is_deterministic() {
        let header = [0xAAu8; 80];
        let a = scrypt_hash(&header);
        let b = scrypt_hash(&header);
        assert_eq!(a, b, "scrypt must produce identical output for identical input");
    }

    /// Output shape — LTC scrypt always returns exactly 32 bytes. Locked
    /// here so a future typo in the params constant (e.g. dkLen=16) is
    /// caught before it can ship.
    #[test]
    fn scrypt_hash_returns_32_bytes() {
        let header = [0u8; 80];
        let h = scrypt_hash(&header);
        assert_eq!(h.len(), 32);
    }

    /// Different inputs produce different outputs — sanity guard against
    /// a degenerate implementation that returns a constant value for
    /// every header.
    #[test]
    fn scrypt_hash_distinguishes_inputs() {
        let a = scrypt_hash(&[0u8; 80]);
        let mut header_b = [0u8; 80];
        header_b[0] = 1;
        let b = scrypt_hash(&header_b);
        assert_ne!(a, b);
    }

    /// Cross-implementation reference vector.
    ///
    /// The expected output should be produced by running Litecoin Core
    /// (or any conformant reference implementation) on a known LTC
    /// mainnet header and recording the scrypt result. The test is
    /// `#[ignore]` until the reference value is recorded — running it
    /// without the value would silently assert the wrong thing.
    ///
    /// Target header: LTC mainnet block 3,106,656 (anchor candidate
    /// for the SPV relay).
    ///   - hash (display):  8a89d2e52329aabe63fabeb9d4cf734d8a44de158598afb6560f20f8c947be64
    ///   - timestamp:       1778676649  (2026-05-13 04:50:49 UTC)
    ///   - bits:            0x1929b619
    ///   - version:         0x20000000
    ///   - prev_hash (disp):d025f09b02adbaaeee722173a89f654577e0f5fba7301e992f0da57b6ee3dc1a
    ///   - merkle (disp):   9fee8a55c04cb632aa424d867e7eec7a23e3e20b7ed04aa113fc1da6b72a40e6
    ///   - nonce:           TODO (litecoinspace.org block-summary endpoint omits nonce;
    ///                      fetch via /api/block/{hash}/header to recover the raw 80-byte hex)
    ///
    /// Run with: `cargo test --release scrypt_hash_matches_litecoin_core_reference -- --ignored`
    #[test]
    #[ignore = "needs Litecoin Core reference scrypt value + complete 80-byte header (nonce missing from summary fetch)"]
    fn scrypt_hash_matches_litecoin_core_reference() {
        // Placeholder — fill in once raw 80-byte header + reference scrypt
        // output are recorded. Until then, the assertion is meaningless
        // (the ignored attribute prevents accidental green-on-empty runs).
        let header = [0u8; 80];
        let _result = scrypt_hash(&header);
        // assert_eq!(_result, hex_lit::hex!("…32 bytes of reference output…"));
    }
}
