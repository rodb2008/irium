//! Dogecoin SPV header relay primitive (Phase A1).
//!
//! Tracks a chain of Dogecoin block headers in iriumd consensus state.
//! Dogecoin shares Bitcoin/Litecoin's 80-byte header wire format and
//! sha256d chain-linkage hash. Its scrypt proof-of-work uses the same
//! `scrypt(N=1024, r=1, p=1)` parameters as Litecoin, so the existing
//! `scrypt_pow::meets_target_ltc` helper is the byte-identical PoW
//! verifier here as well — no DOGE-specific scrypt parameterisation
//! is needed.
//!
//! Phase A1 scope (this file):
//!   - 80-byte header type with sha256d block-hash and scrypt PoW check.
//!   - Anchor type (with an extra `prev_time` slot needed by Digishield).
//!   - Digishield-v3 retarget primitive (`DigishieldParams`,
//!     `expected_bits_digishield`).
//!   - `apply_doge_header_batch` / `undo_doge_relay_update` plus their
//!     undo record.
//!   - Header-batch wire encode/parse under tag `0xc9`.
//!   - Tests for serialisation, batch parsing, work, target compaction,
//!     anchor wiring, and Digishield retarget (clamp + damping).
//!
//! Phase A1 explicitly does NOT verify AuxPoW proofs. Roughly 100% of
//! live Dogecoin blocks since height 371,337 are merged-mined with
//! Litecoin and their work proofs live on the LTC parent header, not on
//! their own 80-byte header bytes. Phase A1's PoW check accepts only
//! headers whose own scrypt(80B) satisfies their declared target — i.e.
//! solo-mined DOGE blocks. This is enough to unit-test the relay
//! plumbing and to drive a regtest DOGE devnet. Phase A2 will add the
//! AuxPoW proof structure (coinbase tx + coinbase merkle branch + chain
//! merkle branch + parent LTC header) and verify the parent's scrypt
//! work directly, before mainnet activation can even be considered.
//!
//! Differences from `ltc_spv` worth calling out:
//!   - Retarget happens EVERY block (post-Digishield-v3 activation at
//!     DOGE block 145,000). No sliding-window walk — the algorithm uses
//!     only the parent and grandparent times.
//!   - `DigishieldParams` is intentionally a separate type from
//!     `RetargetParams`: the LTC docstring explicitly forecasted this
//!     split and declined a shared trait. See `ltc_spv::RetargetParams`.
//!   - `DogeAnchor` carries an extra `prev_time` field (the time of the
//!     block one height below the anchor) so the first relayed header's
//!     Digishield retarget has its grandparent's timestamp.
//!
//! Shipping disabled on mainnet (activation height = `None`). Devnet and
//! testnet enable via an `IRIUM_DOGE_SPV_RELAY_*` env override path
//! mirroring the LTC and BTC resolvers. No consensus dispatch path yet
//! references `DOGE_HEADER_BATCH_TAG`; this module is staged so a future
//! activation commit doesn't have to introduce the primitive and the
//! wiring in a single PR.

use std::collections::HashMap;
use std::env;

use num_bigint::BigUint;
use num_traits::Zero;
use rayon::prelude::*;

use crate::activation::{resolved_doge_spv_relay_activation_height, NetworkKind};
use crate::pow::{sha256d, Target};
use crate::scrypt_pow::meets_target_ltc;

// Re-export the activation-side DOGE anchor constants so downstream
// modules (and the tests in this module) can keep referring to them via
// `crate::doge_spv::MAINNET_DOGE_*` exactly the way LTC does in
// `ltc_spv`.
pub use crate::activation::{
    MAINNET_DOGE_ANCHOR_BITS, MAINNET_DOGE_ANCHOR_HASH_DISPLAY, MAINNET_DOGE_ANCHOR_HEIGHT,
    MAINNET_DOGE_ANCHOR_PREV_TIME, MAINNET_DOGE_ANCHOR_TIME,
};
#[cfg(test)]
use crate::activation::MAINNET_DOGE_SPV_RELAY_ACTIVATION_HEIGHT;

/// Output script tag reserved for a Dogecoin header batch. Consensus
/// dispatch is wired in a later phase once governance assigns an
/// activation height; chosen as `0xc9` to avoid collision with existing
/// tags `0xc0` (HTLCv1), `0xc1` (MPSOv1), `0xc3` (HtlcBtcSwapV1),
/// `0xc4` (BtcHeaderBatch), `0xc5` (SwapOrder), `0xc6` (LtcHeaderBatch),
/// `0xc7` (HtlcLtcSwapV1), and `0xc8` (LtcSwapOrder).
pub const DOGE_HEADER_BATCH_TAG: u8 = 0xc9;
pub const DOGE_HEADER_BATCH_VERSION: u8 = 0x01;
/// PR-4 of issue #68: v0x02 batches carry optional per-header AuxPoW
/// bytes. The chain consensus path (`parse_doge_header_batch`) still
/// rejects v0x02 via its "unknown version" check — activation in
/// iriumd blocks is gated by a future chain.rs PR. The format is
/// available today via `parse_doge_header_batch_with_auxpow` for
/// tooling and the post-activation chain code that will land later.
pub const DOGE_HEADER_BATCH_VERSION_V2: u8 = 0x02;
/// Per-header AuxPoW byte cap. Real AuxPoW is ~300-500 bytes (parent
/// header 80 + coinbase ~200 + 2 merkle branches of ~9 hashes × 32
/// bytes). 10 KB is well above any historical or projected value;
/// the cap exists to prevent memory-exhaustion via malicious batches.
pub const MAX_DOGE_AUXPOW_BYTES: usize = 10_000;
/// Upper bound on the v0x02 batch payload size, derived from the
/// 144-headers-per-batch cap × (80 header + 1 flag + 3 varint + 10 KB
/// auxpow). ~1.5 MB worst case. iriumd MAX_BLOCK_SIZE is 4 MB (per
/// `src/protocol.rs:9`), so a v0x02 batch fits comfortably inside a
/// single block. Typical real-world batches are ~70 KB (~500 B
/// AuxPoW × 144 headers).
pub const MAX_DOGE_HEADER_BATCH_V2_BYTES: usize =
    4 + (DOGE_HEADER_BYTES + 1 + 3 + MAX_DOGE_AUXPOW_BYTES)
        * (MAX_DOGE_HEADERS_PER_BATCH as usize);
pub const DOGE_HEADER_BYTES: usize = 80;

/// Regtest proof-of-work limit (`bnProofOfWorkLimit` in dogecoind/regtest).
/// When the anchor uses this value the Digishield retarget is
/// short-circuited and all headers must use this same `bits`. Mainnet
/// anchors carry real DOGE difficulty (e.g. 0x1a0097af) so this branch
/// never fires there.
pub const DOGE_REGTEST_POW_LIMIT_BITS: u32 = 0x207fffff;

/// Dogecoin scrypt PoW costs ~10 ms per header on commodity hardware —
/// same cost as Litecoin since the parameters are identical. A 144-cap
/// batch verified across 8 cores via rayon completes in well under
/// 250 ms — within the per-block validation budget the iriumd consensus
/// loop assumes.
pub const MAX_DOGE_HEADERS_PER_BATCH: u16 = 144;
pub const MAX_DOGE_HEADER_BATCH_BYTES: usize =
    4 + DOGE_HEADER_BYTES * (MAX_DOGE_HEADERS_PER_BATCH as usize);

/// Median-time-past window: 11 ancestor headers, matching the
/// Bitcoin/Litecoin/Dogecoin Core convention.
pub const DOGE_MTP_WINDOW: usize = 11;
/// Maximum allowed gap between a Dogecoin header time and the iriumd
/// block time that carried it: 2 hours, matching Dogecoin Core's
/// `nMaxFutureBlockTime`.
pub const DOGE_MAX_FUTURE_TIME_SECS: u32 = 2 * 60 * 60;

/// Difficulty-retarget parameter bundle for chains using a
/// Digishield-v3-style per-block damped retarget. Dogecoin (post-block
/// 145,000) uses this algorithm.
///
/// Intentionally NOT shared with `ltc_spv::RetargetParams`. The LTC
/// module docstring explicitly forecasted this split:
///
/// > "chains with fundamentally different algorithms (DOGE DigiShield,
/// > BCH EDA) get their own retarget function, NOT new fields on
/// > `RetargetParams`."
///
/// The two algorithms differ in lookback (window vs single parent
/// step), retarget cadence (per-window vs per-block), and clamp
/// shape (symmetric multiplier/divisor vs asymmetric numerator/
/// denominator). Forcing them through one struct would lose more
/// clarity than it saves.
///
/// Clamps are encoded as numerator/denominator over `target_spacing`:
///   - min_ts = target_spacing * min_clamp_numerator / min_clamp_denominator
///   - max_ts = target_spacing * max_clamp_numerator / max_clamp_denominator
///
/// For Dogecoin this gives min_ts = 60 * 3/4 = 45 s and max_ts =
/// 60 * 3/2 = 90 s, matching Dogecoin Core's own arithmetic
/// (`target - target/4`, `target + target/2`) exactly.
#[derive(Debug, Clone, Copy)]
pub struct DigishieldParams {
    /// Target spacing between consecutive blocks, in seconds.
    pub target_spacing_secs: u32,
    /// Damping divisor applied to the deviation `(actual - target)`.
    /// Dogecoin uses 8 (pulls the timespan 7/8 of the way back toward
    /// the target each block).
    pub damping_divisor: u32,
    /// Lower clamp on the damped timespan, expressed as a numerator
    /// over `min_clamp_denominator`. Dogecoin: 3/4.
    pub min_clamp_numerator: u32,
    pub min_clamp_denominator: u32,
    /// Upper clamp on the damped timespan, expressed as a numerator
    /// over `max_clamp_denominator`. Dogecoin: 3/2.
    pub max_clamp_numerator: u32,
    pub max_clamp_denominator: u32,
    /// Compact-form maximum target. Newly-retargeted targets clamp here.
    pub max_target_bits: u32,
}

impl DigishieldParams {
    /// Dogecoin mainnet Digishield-v3 (post-block 145,000): 60 s target
    /// spacing, damping divisor 8, asymmetric clamp [3/4, 3/2] of
    /// target, max target `0x1e0fffff` (same as Litecoin's pow_limit).
    pub const DOGECOIN: Self = Self {
        target_spacing_secs: 60,
        damping_divisor: 8,
        min_clamp_numerator: 3,
        min_clamp_denominator: 4,
        max_clamp_numerator: 3,
        max_clamp_denominator: 2,
        max_target_bits: 0x1e0f_ffff,
    };
}

/// 80-byte Dogecoin block header in little-endian wire format. The
/// field layout matches Bitcoin and Litecoin byte-for-byte; the
/// chain-level differences live in the retarget algorithm and (for
/// AuxPoW blocks, Phase A2) in the proof of work proof material that
/// follows this header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DogeHeader {
    pub version: i32,
    pub prev_hash: [u8; 32],
    pub merkle_root: [u8; 32],
    pub time: u32,
    pub bits: u32,
    pub nonce: u32,
}

impl DogeHeader {
    pub fn serialize(&self) -> [u8; DOGE_HEADER_BYTES] {
        let mut out = [0u8; DOGE_HEADER_BYTES];
        out[0..4].copy_from_slice(&self.version.to_le_bytes());
        out[4..36].copy_from_slice(&self.prev_hash);
        out[36..68].copy_from_slice(&self.merkle_root);
        out[68..72].copy_from_slice(&self.time.to_le_bytes());
        out[72..76].copy_from_slice(&self.bits.to_le_bytes());
        out[76..80].copy_from_slice(&self.nonce.to_le_bytes());
        out
    }

    pub fn deserialize(bytes: &[u8]) -> Result<DogeHeader, String> {
        if bytes.len() != DOGE_HEADER_BYTES {
            return Err(format!(
                "doge header must be exactly {} bytes (got {})",
                DOGE_HEADER_BYTES,
                bytes.len()
            ));
        }
        let mut prev_hash = [0u8; 32];
        prev_hash.copy_from_slice(&bytes[4..36]);
        let mut merkle_root = [0u8; 32];
        merkle_root.copy_from_slice(&bytes[36..68]);
        Ok(DogeHeader {
            version: i32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            prev_hash,
            merkle_root,
            time: u32::from_le_bytes(bytes[68..72].try_into().unwrap()),
            bits: u32::from_le_bytes(bytes[72..76].try_into().unwrap()),
            nonce: u32::from_le_bytes(bytes[76..80].try_into().unwrap()),
        })
    }

    /// Natural-order sha256d of the serialized header — used for chain
    /// linkage (`prev_hash` references this value). Display-order DOGE
    /// hashes are this value reversed. PoW uses scrypt, not this hash
    /// — see [`DogeHeader::meets_pow`].
    pub fn block_hash(&self) -> [u8; 32] {
        sha256d(&self.serialize())
    }

    /// True iff this header's own scrypt PoW satisfies its declared
    /// target. Phase A1 trusts this check unconditionally. Phase A2
    /// will add an AuxPoW path so a header whose own scrypt fails can
    /// still be accepted when an attached merge-mining proof shows the
    /// required work on the parent Litecoin chain. Same Litecoin
    /// scrypt parameters (`N=1024, r=1, p=1`), so the existing
    /// `meets_target_ltc` helper is the byte-identical verifier here.
    pub fn meets_pow(&self) -> bool {
        meets_target_ltc(&self.serialize(), Target { bits: self.bits })
    }

    /// PR-5 of issue #68: validate this header's AuxPoW via the LTC
    /// parent's Scrypt PoW and the merge-mining merkle proofs.
    ///
    /// Returns true iff `auxpow::validate_with_parent_hash` accepts
    /// the AuxPoW with the LTC Scrypt parent-PoW closure against this
    /// DOGE block's target (derived from `self.bits`).
    ///
    /// Used by `apply_doge_header_batch_with_auxpow` for headers at
    /// DOGE height ≥ 371,337 with the AuxPoW bit (0x100) set in the
    /// version field. Pre-AuxPoW headers and headers without the bit
    /// continue to use `meets_pow` (standalone Scrypt).
    pub fn meets_pow_auxpow(&self, auxpow: &crate::auxpow::AuxPoW) -> bool {
        let aux_header = self.serialize();
        let target = Target { bits: self.bits };
        crate::auxpow::validate_with_parent_hash(
            auxpow,
            &aux_header,
            target,
            |parent_header| meets_target_ltc(parent_header, target),
        )
        .is_ok()
    }
}

/// One stored Dogecoin header plus its derived chain metadata.
#[derive(Debug, Clone)]
pub struct DogeHeaderEntry {
    pub header: DogeHeader,
    pub height: u64,
    pub total_work: BigUint,
}

/// PR-4 of issue #68: a DOGE header paired with optional raw AuxPoW
/// bytes. Returned by `parse_doge_header_batch_with_auxpow` when
/// parsing v0x02 batches. The bytes are deliberately not deserialized
/// at parse time — the AuxPoW validator (later PR) consumes them via
/// `auxpow::deserialize`. Using `Option<Vec<u8>>` instead of an empty
/// `Vec` makes the "no auxpow attached" case explicit.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // wired into doge mining / post-activation chain code in later PRs
pub struct ParsedDogeHeader {
    pub header: DogeHeader,
    pub auxpow: Option<Vec<u8>>,
}

/// The first Dogecoin header known to this iriumd relay. No header
/// before `height` is ever submittable. Zero-valued anchor disables
/// the relay.
///
/// Unlike `LtcAnchor`, `DogeAnchor` carries an extra `prev_time` field:
/// Digishield's retarget needs the grandparent's timestamp, and for the
/// first relayed header (at `height + 1`) the grandparent IS the block
/// one step below the anchor. We pre-record its time at anchor pick
/// time so the relay can validate the very first submitted header.
#[derive(Debug, Clone, Copy)]
pub struct DogeAnchor {
    pub hash: [u8; 32],
    pub height: u64,
    pub bits: u32,
    pub time: u32,
    pub prev_time: u32,
}

impl DogeAnchor {
    pub const fn zero() -> DogeAnchor {
        DogeAnchor {
            hash: [0u8; 32],
            height: 0,
            bits: 0,
            time: 0,
            prev_time: 0,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.hash == [0u8; 32]
            && self.height == 0
            && self.bits == 0
            && self.time == 0
            && self.prev_time == 0
    }

    /// Construct the mainnet anchor from the hardcoded display-order
    /// hash, reversing it into natural storage order. Returns the zero
    /// anchor if every constant happens to be zero (defensive against
    /// a future edit that accidentally clears them).
    pub fn mainnet() -> DogeAnchor {
        let mut hash = MAINNET_DOGE_ANCHOR_HASH_DISPLAY;
        hash.reverse();
        let candidate = DogeAnchor {
            hash,
            height: MAINNET_DOGE_ANCHOR_HEIGHT,
            bits: MAINNET_DOGE_ANCHOR_BITS,
            time: MAINNET_DOGE_ANCHOR_TIME,
            prev_time: MAINNET_DOGE_ANCHOR_PREV_TIME,
        };
        if candidate.hash == [0u8; 32]
            && candidate.bits == 0
            && candidate.time == 0
            && candidate.prev_time == 0
        {
            DogeAnchor::zero()
        } else {
            candidate
        }
    }
}

/// Configuration bundle for the DOGE SPV relay. `None` in
/// `ChainParams.doge_spv` (Phase B wiring) keeps the relay disabled.
#[derive(Debug, Clone)]
pub struct DogeSpvParams {
    pub activation_height: u64,
    pub anchor: DogeAnchor,
    pub retarget: DigishieldParams,
}

/// Resolve the DOGE SPV relay configuration for a given network. Returns
/// `Some` only when an activation height AND a valid anchor are both
/// present. Mirrors `ltc_spv::resolve_ltc_spv_params` and
/// `btc_spv::resolve_btc_spv_params`.
///
/// Mainnet uses the code-defined `MAINNET_DOGE_*` constants from
/// `activation.rs` (currently `None` placeholders until governance flips
/// them in a dedicated activation commit per the workflow in
/// `docs/htlcv1_activation_commit_workflow.md`).
///
/// Testnet and devnet read the anchor from the five
/// `IRIUM_DOGE_ANCHOR_{HEIGHT,HASH,BITS,TIME,PREV_TIME}` environment
/// variables alongside the existing
/// `IRIUM_DOGE_SPV_RELAY_ACTIVATION_HEIGHT`. All six must be present
/// for the relay to enable. Hash is accepted in display order
/// (Dogecoin RPC convention) and canonicalised to natural order for
/// internal storage. `BITS` accepts either `0x1a0097af` or decimal.
#[allow(dead_code)]
pub fn resolve_doge_spv_params(network: NetworkKind) -> Option<DogeSpvParams> {
    let activation_height = resolved_doge_spv_relay_activation_height(network)?;
    let anchor = match network {
        NetworkKind::Mainnet => {
            let candidate = DogeAnchor::mainnet();
            if candidate.is_zero() {
                return None;
            }
            candidate
        }
        NetworkKind::Testnet | NetworkKind::Devnet => {
            let height = env::var("IRIUM_DOGE_ANCHOR_HEIGHT")
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())?;
            let hash_str = env::var("IRIUM_DOGE_ANCHOR_HASH").ok()?;
            let hash_bytes = hex::decode(hash_str.trim()).ok()?;
            if hash_bytes.len() != 32 {
                return None;
            }
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&hash_bytes);
            hash.reverse();
            let bits_str = env::var("IRIUM_DOGE_ANCHOR_BITS").ok()?;
            let bits_trim = bits_str.trim();
            let bits = if let Some(stripped) = bits_trim.strip_prefix("0x") {
                u32::from_str_radix(stripped, 16).ok()?
            } else {
                bits_trim.parse::<u32>().ok()?
            };
            let time = env::var("IRIUM_DOGE_ANCHOR_TIME")
                .ok()
                .and_then(|v| v.trim().parse::<u32>().ok())?;
            let prev_time = env::var("IRIUM_DOGE_ANCHOR_PREV_TIME")
                .ok()
                .and_then(|v| v.trim().parse::<u32>().ok())?;
            DogeAnchor {
                hash,
                height,
                bits,
                time,
                prev_time,
            }
        }
    };
    Some(DogeSpvParams {
        activation_height,
        anchor,
        retarget: DigishieldParams::DOGECOIN,
    })
}

/// Undo record produced by one successful header batch apply. Stored
/// inside the per-block undo log and consumed by `undo_doge_relay_update`
/// on iriumd block disconnect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DogeRelayUpdate {
    pub tip_before: Option<[u8; 32]>,
    pub tip_height_before: u64,
    pub headers_added: Vec<[u8; 32]>,
}

/// Encode a sequence of headers as a `DogeHeaderBatch` output script
/// payload. Same wire shape as `ltc_spv::encode_ltc_header_batch`
/// (tag, version, u16-LE count, N * 80-byte headers) so RPC handlers
/// can be templated across chains.
#[allow(dead_code)]
pub fn encode_doge_header_batch(headers: &[DogeHeader]) -> Result<Vec<u8>, String> {
    if headers.is_empty() {
        return Err("doge header batch: empty".to_string());
    }
    if headers.len() > MAX_DOGE_HEADERS_PER_BATCH as usize {
        return Err(format!(
            "doge header batch: {} headers exceeds max {}",
            headers.len(),
            MAX_DOGE_HEADERS_PER_BATCH
        ));
    }
    let count = headers.len() as u16;
    let mut out = Vec::with_capacity(4 + DOGE_HEADER_BYTES * headers.len());
    out.push(DOGE_HEADER_BATCH_TAG);
    out.push(DOGE_HEADER_BATCH_VERSION);
    out.extend_from_slice(&count.to_le_bytes());
    for h in headers {
        out.extend_from_slice(&h.serialize());
    }
    Ok(out)
}

// ====================================================================
// PR-4 of issue #68 — v0x02 wire format with optional per-header AuxPoW
// ====================================================================
//
// Per-batch:
//   [1 byte: tag = 0xc9]
//   [1 byte: version = 0x02]
//   [2 bytes LE: count]
//   [per-header payload * count]
//
// Per-header payload:
//   [80 bytes: DOGE header]
//   [1 byte: has_auxpow flag (0x00 or 0x01)]
//   If has_auxpow == 0x01:
//     [varint: auxpow_len]
//     [auxpow_len bytes: serialized AuxPoW (see auxpow::serialize)]
//
// The legacy v0x01 parser (`parse_doge_header_batch`) is the
// consensus path used by chain.rs/mempool.rs. It rejects v0x02 via
// its existing "unknown version" check — activation in iriumd blocks
// lands in a future PR. The new
// `parse_doge_header_batch_with_auxpow` is format-tolerant (accepts
// both v0x01 and v0x02) and is for tooling + post-activation chain
// code. The new `encode_doge_header_batch_with_auxpow` emits v0x02.

/// Parse a `DogeHeaderBatch` script accepting both v0x01 and v0x02.
/// Preserves any per-header AuxPoW bytes (None for v0x01 entries).
///
/// **NOT** called from the chain consensus path — see
/// `parse_doge_header_batch` for that. This function is the
/// format-tolerant variant for tooling and the post-activation chain
/// code that will land in a later PR.
#[allow(dead_code)] // wired into doge mining / post-activation chain code in later PRs
pub fn parse_doge_header_batch_with_auxpow(
    script: &[u8],
) -> Result<Vec<ParsedDogeHeader>, String> {
    if script.len() < 4 {
        return Err("doge header batch: script too short".to_string());
    }
    if script[0] != DOGE_HEADER_BATCH_TAG {
        return Err("doge header batch: wrong tag".to_string());
    }
    let version = script[1];
    let count = u16::from_le_bytes([script[2], script[3]]) as usize;
    if count == 0 || count > MAX_DOGE_HEADERS_PER_BATCH as usize {
        return Err(format!("doge header batch: count {} out of range", count));
    }
    match version {
        DOGE_HEADER_BATCH_VERSION => {
            // v0x01: fixed 80 bytes per header, no auxpow.
            let expected = 4 + DOGE_HEADER_BYTES * count;
            if script.len() != expected {
                return Err(format!(
                    "doge header batch v1: wrong size (got {}, expected {})",
                    script.len(),
                    expected
                ));
            }
            let mut out = Vec::with_capacity(count);
            for i in 0..count {
                let start = 4 + i * DOGE_HEADER_BYTES;
                let h = DogeHeader::deserialize(&script[start..start + DOGE_HEADER_BYTES])?;
                out.push(ParsedDogeHeader { header: h, auxpow: None });
            }
            Ok(out)
        }
        DOGE_HEADER_BATCH_VERSION_V2 => {
            // v0x02: variable size per header (80 + flag + optional
            // varint + auxpow bytes).
            if script.len() > MAX_DOGE_HEADER_BATCH_V2_BYTES {
                return Err(format!(
                    "doge header batch v2: total size {} exceeds cap {}",
                    script.len(),
                    MAX_DOGE_HEADER_BATCH_V2_BYTES
                ));
            }
            let mut out = Vec::with_capacity(count);
            let mut off = 4usize;
            for i in 0..count {
                if off + DOGE_HEADER_BYTES + 1 > script.len() {
                    return Err(format!(
                        "doge header batch v2: truncated at header {}",
                        i
                    ));
                }
                let h = DogeHeader::deserialize(&script[off..off + DOGE_HEADER_BYTES])?;
                off += DOGE_HEADER_BYTES;
                let flag = script[off];
                off += 1;
                let auxpow = match flag {
                    0x00 => None,
                    0x01 => {
                        let auxpow_len = read_varint_doge(script, &mut off)?;
                        if auxpow_len > MAX_DOGE_AUXPOW_BYTES {
                            return Err(format!(
                                "doge header batch v2 header {}: auxpow_len {} exceeds cap {}",
                                i, auxpow_len, MAX_DOGE_AUXPOW_BYTES
                            ));
                        }
                        if off + auxpow_len > script.len() {
                            return Err(format!(
                                "doge header batch v2 header {}: auxpow truncated",
                                i
                            ));
                        }
                        let bytes = script[off..off + auxpow_len].to_vec();
                        off += auxpow_len;
                        Some(bytes)
                    }
                    other => {
                        return Err(format!(
                            "doge header batch v2 header {}: invalid has_auxpow flag 0x{:02x}",
                            i, other
                        ));
                    }
                };
                out.push(ParsedDogeHeader { header: h, auxpow });
            }
            if off != script.len() {
                return Err(format!(
                    "doge header batch v2: trailing {} bytes after last header",
                    script.len() - off
                ));
            }
            Ok(out)
        }
        v => Err(format!("doge header batch: unknown version 0x{:02x}", v)),
    }
}

/// Encode a v0x02 `DogeHeaderBatch` script with optional per-header
/// AuxPoW bytes. Used by future tooling / mining code (gated by
/// activation in a later PR). Pre-activation callers should continue
/// using `encode_doge_header_batch` (v0x01).
pub fn encode_doge_header_batch_with_auxpow(
    items: &[ParsedDogeHeader],
) -> Result<Vec<u8>, String> {
    if items.is_empty() {
        return Err("doge header batch v2: empty".to_string());
    }
    if items.len() > MAX_DOGE_HEADERS_PER_BATCH as usize {
        return Err(format!(
            "doge header batch v2: {} headers exceeds max {}",
            items.len(),
            MAX_DOGE_HEADERS_PER_BATCH
        ));
    }
    let count = items.len() as u16;
    let mut out = Vec::with_capacity(4 + (DOGE_HEADER_BYTES + 1) * items.len());
    out.push(DOGE_HEADER_BATCH_TAG);
    out.push(DOGE_HEADER_BATCH_VERSION_V2);
    out.extend_from_slice(&count.to_le_bytes());
    for item in items {
        out.extend_from_slice(&item.header.serialize());
        match &item.auxpow {
            None => out.push(0x00),
            Some(bytes) => {
                if bytes.len() > MAX_DOGE_AUXPOW_BYTES {
                    return Err(format!(
                        "doge header batch v2: auxpow {} bytes exceeds cap {}",
                        bytes.len(),
                        MAX_DOGE_AUXPOW_BYTES
                    ));
                }
                out.push(0x01);
                write_varint_doge(&mut out, bytes.len());
                out.extend_from_slice(bytes);
            }
        }
    }
    if out.len() > MAX_DOGE_HEADER_BATCH_V2_BYTES {
        return Err(format!(
            "doge header batch v2: total {} bytes exceeds cap {}",
            out.len(),
            MAX_DOGE_HEADER_BATCH_V2_BYTES
        ));
    }
    Ok(out)
}

/// Private varint reader for v0x02 batch parsing. Bitcoin CompactSize
/// (1, 3, 5, or 9 bytes). Kept private to doge_spv.rs to avoid
/// surfacing a duplicate of auxpow::read_varint as a pub API.
fn read_varint_doge(data: &[u8], off: &mut usize) -> Result<usize, String> {
    if *off >= data.len() {
        return Err("varint: EOF".to_string());
    }
    let first = data[*off];
    *off += 1;
    match first {
        0xff => {
            if *off + 8 > data.len() {
                return Err("varint: EOF (8b)".to_string());
            }
            let mut b = [0u8; 8];
            b.copy_from_slice(&data[*off..*off + 8]);
            *off += 8;
            Ok(u64::from_le_bytes(b) as usize)
        }
        0xfe => {
            if *off + 4 > data.len() {
                return Err("varint: EOF (4b)".to_string());
            }
            let mut b = [0u8; 4];
            b.copy_from_slice(&data[*off..*off + 4]);
            *off += 4;
            Ok(u32::from_le_bytes(b) as usize)
        }
        0xfd => {
            if *off + 2 > data.len() {
                return Err("varint: EOF (2b)".to_string());
            }
            let mut b = [0u8; 2];
            b.copy_from_slice(&data[*off..*off + 2]);
            *off += 2;
            Ok(u16::from_le_bytes(b) as usize)
        }
        n => Ok(n as usize),
    }
}

/// Private varint writer for v0x02 batch serialization. Mirror of
/// `read_varint_doge`.
fn write_varint_doge(out: &mut Vec<u8>, n: usize) {
    if n < 0xfd {
        out.push(n as u8);
    } else if n <= 0xffff {
        out.push(0xfd);
        out.extend_from_slice(&(n as u16).to_le_bytes());
    } else if n <= 0xffff_ffff {
        out.push(0xfe);
        out.extend_from_slice(&(n as u32).to_le_bytes());
    } else {
        out.push(0xff);
        out.extend_from_slice(&(n as u64).to_le_bytes());
    }
}

pub fn parse_doge_header_batch(script: &[u8]) -> Result<Vec<DogeHeader>, String> {
    if script.len() < 4 {
        return Err("doge header batch: script too short".to_string());
    }
    if script[0] != DOGE_HEADER_BATCH_TAG {
        return Err("doge header batch: wrong tag".to_string());
    }
    if script[1] != DOGE_HEADER_BATCH_VERSION {
        return Err("doge header batch: unknown version".to_string());
    }
    let count = u16::from_le_bytes([script[2], script[3]]) as usize;
    if count == 0 || count > MAX_DOGE_HEADERS_PER_BATCH as usize {
        return Err(format!("doge header batch: count {} out of range", count));
    }
    let expected = 4 + DOGE_HEADER_BYTES * count;
    if script.len() != expected {
        return Err(format!(
            "doge header batch: wrong size (got {}, expected {})",
            script.len(),
            expected
        ));
    }
    let mut headers = Vec::with_capacity(count);
    for i in 0..count {
        let start = 4 + i * DOGE_HEADER_BYTES;
        let h = DogeHeader::deserialize(&script[start..start + DOGE_HEADER_BYTES])?;
        headers.push(h);
    }
    Ok(headers)
}

/// Cumulative work for a header with compact `bits`. Standard
/// Bitcoin/Litecoin/Dogecoin formula: work = 2^256 / (target + 1).
/// Copied (not shared) from `ltc_spv::work_for_bits` per the same
/// design intent — keep each chain's PoW math independently editable.
pub fn work_for_bits(bits: u32) -> BigUint {
    let target = Target { bits }.to_target();
    let two_pow_256 = BigUint::from(1u8) << 256;
    let denom = target + BigUint::from(1u8);
    two_pow_256 / denom
}

pub fn doge_max_target(params: &DigishieldParams) -> BigUint {
    Target {
        bits: params.max_target_bits,
    }
    .to_target()
}

/// Encode a target BigUint to compact `bits` form. Inverse of
/// `Target::to_target`. Canonical (high-bit clamped) output. Copied
/// from `ltc_spv::target_to_compact_bits` so future DOGE maintenance
/// can't inadvertently change LTC/BTC behaviour via a shared helper.
pub fn target_to_compact_bits(target: &BigUint) -> u32 {
    if target.is_zero() {
        return 0;
    }
    let bytes = target.to_bytes_be();
    let mut size = bytes.len();
    let mut compact: u32 = if size <= 3 {
        let mut v: u32 = 0;
        for b in &bytes {
            v = (v << 8) | (*b as u32);
        }
        v << (8 * (3 - size))
    } else {
        ((bytes[0] as u32) << 16) | ((bytes[1] as u32) << 8) | (bytes[2] as u32)
    };
    if compact & 0x0080_0000 != 0 {
        compact >>= 8;
        size += 1;
    }
    compact |= (size as u32) << 24;
    compact
}

/// Combined read-only view over committed headers + headers staged
/// earlier in the same batch. Lets a single batch's later headers
/// reference its earlier headers' MTP and Digishield grandparent
/// lookup before any state has been written.
struct LookupView<'a> {
    committed: &'a HashMap<[u8; 32], DogeHeaderEntry>,
    staged: &'a [([u8; 32], DogeHeaderEntry)],
}

impl<'a> LookupView<'a> {
    fn get(&self, hash: &[u8; 32]) -> Option<&DogeHeaderEntry> {
        if let Some(e) = self.committed.get(hash) {
            return Some(e);
        }
        self.staged
            .iter()
            .rev()
            .find(|(h, _)| h == hash)
            .map(|(_, e)| e)
    }
}

fn median_time_past_v(parent_hash: &[u8; 32], view: &LookupView, anchor: &DogeAnchor) -> u32 {
    let mut times: Vec<u32> = Vec::with_capacity(DOGE_MTP_WINDOW);
    let mut cur = *parent_hash;
    while times.len() < DOGE_MTP_WINDOW {
        if cur == anchor.hash {
            times.push(anchor.time);
            break;
        }
        match view.get(&cur) {
            Some(e) => {
                times.push(e.header.time);
                cur = e.header.prev_hash;
            }
            None => break,
        }
    }
    if times.is_empty() {
        return 0;
    }
    times.sort();
    times[times.len() / 2]
}

/// Pure Digishield-v3 retarget. Inputs are the parent header's bits and
/// time and the grandparent header's time; output is the expected
/// compact-bits for the next (child) header.
///
/// Algorithm (matches Dogecoin Core `CalculateDogecoinNextWorkRequired`
/// on the post-block-145,000 path):
///
/// ```text
/// actual_ts  = parent.time - grandparent.time
/// modulated  = target_ts + (actual_ts - target_ts) / damping_divisor
/// min_ts     = target_ts * min_clamp_numerator / min_clamp_denominator
/// max_ts     = target_ts * max_clamp_numerator / max_clamp_denominator
/// clamped    = modulated.clamp(min_ts, max_ts)
/// new_target = parent.target * clamped / target_ts
/// new_target = min(new_target, pow_limit)
/// new_bits   = compact(new_target)
/// ```
///
/// All intermediate arithmetic is in i64 to handle the rare case where
/// `parent.time < grandparent.time` (DOGE timestamps need only be above
/// MTP, not strictly monotonic). The clamp values for Dogecoin are
/// 45 s lower and 90 s upper, so after `.clamp()` the result is always
/// positive — we still defensively `.max(0)` before the BigUint cast.
pub fn expected_bits_digishield(
    parent_bits: u32,
    parent_time: u32,
    grandparent_time: u32,
    params: &DigishieldParams,
) -> u32 {
    let target_ts = params.target_spacing_secs as i64;
    let actual_ts = (parent_time as i64) - (grandparent_time as i64);

    let damping = params.damping_divisor.max(1) as i64;
    let modulated = target_ts + (actual_ts - target_ts) / damping;

    let min_ts = target_ts * (params.min_clamp_numerator as i64)
        / (params.min_clamp_denominator.max(1) as i64);
    let max_ts = target_ts * (params.max_clamp_numerator as i64)
        / (params.max_clamp_denominator.max(1) as i64);
    let clamped = modulated.clamp(min_ts, max_ts).max(0) as u64;

    let parent_target = Target { bits: parent_bits }.to_target();
    let new_target =
        parent_target * BigUint::from(clamped) / BigUint::from(target_ts.max(1) as u64);
    let max_target = doge_max_target(params);
    let final_target = if new_target > max_target {
        max_target
    } else {
        new_target
    };
    target_to_compact_bits(&final_target)
}

/// Expected compact-bits for a header whose parent is `parent_hash`.
/// Resolves the parent's bits + time and the grandparent's time from
/// the relay state (anchor + committed + same-batch staged) and feeds
/// them into `expected_bits_digishield`.
///
/// Grandparent resolution rules:
///   - If `parent_hash == anchor.hash`, the grandparent is one block
///     below the anchor and its time is `anchor.prev_time`.
///   - If the parent's `prev_hash` equals `anchor.hash`, the
///     grandparent IS the anchor and its time is `anchor.time`.
///   - Otherwise the grandparent must already be in committed state or
///     staged earlier in the current batch.
fn expected_bits_for_v(
    parent_hash: &[u8; 32],
    view: &LookupView,
    anchor: &DogeAnchor,
    params: &DigishieldParams,
) -> Result<u32, String> {
    // Regtest carve-out: dogecoind regtest hardcodes bits to its
    // pow_limit (0x207fffff) and never retargets. Mirror that:
    // when the anchor itself is regtest, every subsequent header
    // must also use the regtest limit. `expected_bits_digishield`
    // is otherwise correct for mainnet, which is why we short-circuit
    // here instead of inside that function.
    if anchor.bits == DOGE_REGTEST_POW_LIMIT_BITS {
        let _ = (view, params);
        return Ok(DOGE_REGTEST_POW_LIMIT_BITS);
    }

    let (parent_bits, parent_time, parent_prev_hash) = if *parent_hash == anchor.hash {
        (anchor.bits, anchor.time, [0u8; 32])
    } else {
        let p = view
            .get(parent_hash)
            .ok_or_else(|| "doge expected_bits: parent unknown".to_string())?;
        (p.header.bits, p.header.time, p.header.prev_hash)
    };

    let grandparent_time = if *parent_hash == anchor.hash {
        anchor.prev_time
    } else if parent_prev_hash == anchor.hash {
        anchor.time
    } else {
        view.get(&parent_prev_hash)
            .map(|g| g.header.time)
            .ok_or_else(|| "doge expected_bits: grandparent unknown".to_string())?
    };

    Ok(expected_bits_digishield(
        parent_bits,
        parent_time,
        grandparent_time,
        params,
    ))
}

/// Validate a header batch against current relay state and apply it.
/// On success returns an undo record; on any error no state is mutated.
///
/// Rules mirror `ltc_spv::apply_ltc_header_batch` except:
///   - The expected-bits check uses Digishield (per-block, grandparent
///     time only) instead of a sliding-window retarget.
///   - PoW is verified by `meets_target_ltc` exactly like LTC (DOGE
///     scrypt params are identical to LTC's). Phase A1 rejects headers
///     whose own scrypt fails. Phase A2 will route those through an
///     AuxPoW proof verifier instead.
///
/// Tip switches to the batch's final header iff that header's
/// cumulative work strictly exceeds the prior tip's cumulative work.
/// Lower-work batches are still recorded so they can become canonical
/// later if extended.
/// PR-5 of issue #68: apply a batch of DOGE headers, each optionally
/// carrying AuxPoW data. Post-371,337 DOGE headers with the AuxPoW
/// version bit (0x100) set must carry AuxPoW bytes and are validated
/// via the parent LTC Scrypt PoW + merge-mining merkle proofs. All
/// other headers (pre-activation or post-activation without the bit)
/// fall back to standalone Scrypt (`meets_pow`) — some pools continue
/// emitting legacy-style headers after the boundary; DOGE Core accepts
/// these the same way.
///
/// The DOGE AuxPoW activation height (371,337) comes from
/// `crate::activation::doge_auxpow_activation_height()`, which honors
/// the `IRIUM_DOGE_AUXPOW_HEIGHT` env override on devnet/testnet
/// (mainnet is fixed).
///
/// NOTE: this function does NOT use the parallel PoW pre-check from
/// the legacy `apply_doge_header_batch` because per-header validator
/// dispatch depends on the DOGE height assigned during sequential
/// chain linkage. A later optimization can split into two passes
/// (linkage first to assign heights, then par_iter for PoW) if the
/// performance regression matters.
pub fn apply_doge_header_batch_with_auxpow(
    items: Vec<ParsedDogeHeader>,
    iriumd_block_time: u32,
    doge_headers: &mut HashMap<[u8; 32], DogeHeaderEntry>,
    doge_heights: &mut HashMap<[u8; 32], u64>,
    doge_tip: &mut Option<[u8; 32]>,
    doge_tip_height: &mut u64,
    anchor: &DogeAnchor,
    retarget: &DigishieldParams,
) -> Result<DogeRelayUpdate, String> {
    if items.is_empty() {
        return Err("apply_doge_header_batch: empty batch".to_string());
    }
    if items.len() > MAX_DOGE_HEADERS_PER_BATCH as usize {
        return Err(format!(
            "apply_doge_header_batch: {} headers exceeds max {}",
            items.len(),
            MAX_DOGE_HEADERS_PER_BATCH
        ));
    }
    if anchor.is_zero() {
        return Err("apply_doge_header_batch: anchor not configured".to_string());
    }

    let first = &items[0].header;
    let (start_prev_height, start_prev_work) = if first.prev_hash == anchor.hash {
        (anchor.height, work_for_bits(anchor.bits))
    } else {
        let parent = doge_headers.get(&first.prev_hash).ok_or_else(|| {
            "apply_doge_header_batch: first header does not connect to known chain".to_string()
        })?;
        (parent.height, parent.total_work.clone())
    };

    let auxpow_activation = crate::activation::doge_auxpow_activation_height();
    let mut known_prefix = 0usize;
    let mut prefix_prev_hash = first.prev_hash;
    let mut prefix_prev_height = start_prev_height;
    let mut prefix_prev_work = start_prev_work.clone();
    for parsed in items.iter() {
        let header = &parsed.header;
        if header.prev_hash != prefix_prev_hash {
            break;
        }
        let expected_height = prefix_prev_height + 1;
        let hash = header.block_hash();
        let Some(entry) = doge_headers.get(&hash) else {
            break;
        };
        if entry.height != expected_height || entry.header != *header {
            break;
        }
        known_prefix += 1;
        prefix_prev_hash = hash;
        prefix_prev_height = expected_height;
        prefix_prev_work = entry.total_work.clone();
    }
    if known_prefix == items.len() {
        return Ok(DogeRelayUpdate {
            tip_before: *doge_tip,
            tip_height_before: *doge_tip_height,
            headers_added: Vec::new(),
        });
    }

    let mut prev_hash = prefix_prev_hash;
    let mut prev_height = prefix_prev_height;
    let mut prev_work = prefix_prev_work;
    let items_to_apply = &items[known_prefix..];
    let mut staged: Vec<([u8; 32], DogeHeaderEntry)> =
        Vec::with_capacity(items_to_apply.len());

    for (offset, parsed) in items_to_apply.iter().enumerate() {
        let i = known_prefix + offset;
        let header = &parsed.header;
        if header.prev_hash != prev_hash {
            return Err(format!(
                "apply_doge_header_batch: header {} does not link to previous",
                i
            ));
        }
        let height = prev_height + 1;

        // PR-5: per-header PoW dispatch by DOGE height + AuxPoW bit.
        let auxpow_active_at_this_height = height >= auxpow_activation;
        let has_auxpow_bit =
            (header.version as u32) & crate::auxpow::AUXPOW_VERSION_BIT != 0;
        let pow_ok = if auxpow_active_at_this_height && has_auxpow_bit {
            let bytes = parsed.auxpow.as_ref().ok_or_else(|| {
                format!(
                    "apply_doge_header_batch: header {} at DOGE height {} has AuxPoW bit set but no AuxPoW data attached",
                    i, height
                )
            })?;
            let mut off = 0;
            let auxpow = crate::auxpow::deserialize(bytes, &mut off).map_err(|e| {
                format!(
                    "apply_doge_header_batch: header {} auxpow parse: {}",
                    i, e
                )
            })?;
            if off != bytes.len() {
                return Err(format!(
                    "apply_doge_header_batch: header {} auxpow has {} trailing bytes",
                    i,
                    bytes.len() - off
                ));
            }
            header.meets_pow_auxpow(&auxpow)
        } else {
            // Pre-activation OR post-activation without AuxPoW bit.
            header.meets_pow()
        };
        if !pow_ok {
            return Err(format!(
                "apply_doge_header_batch: header {} fails PoW",
                i
            ));
        }

        let hash = header.block_hash();
        if doge_headers.contains_key(&hash) {
            return Err(format!(
                "apply_doge_header_batch: header {} already known in chain state",
                i
            ));
        }
        if staged.iter().any(|(h, _)| h == &hash) {
            return Err(format!(
                "apply_doge_header_batch: header {} duplicated within batch",
                i
            ));
        }

        let (expected_bits, mtp) = {
            let view = LookupView {
                committed: doge_headers,
                staged: &staged,
            };
            let bits = expected_bits_for_v(&prev_hash, &view, anchor, retarget)?;
            let mtp = median_time_past_v(&prev_hash, &view, anchor);
            (bits, mtp)
        };
        let expected_target = Target { bits: expected_bits }.to_target();
        let header_target = Target { bits: header.bits }.to_target();
        if header_target != expected_target {
            return Err(format!(
                "apply_doge_header_batch: header {} bits mismatch \
                 (expected {:#010x}, got {:#010x})",
                i, expected_bits, header.bits
            ));
        }
        if header.time <= mtp {
            return Err(format!(
                "apply_doge_header_batch: header {} time {} not above MTP {}",
                i, header.time, mtp
            ));
        }
        if header.time > iriumd_block_time.saturating_add(DOGE_MAX_FUTURE_TIME_SECS) {
            return Err(format!(
                "apply_doge_header_batch: header {} time {} more than 2h ahead \
                 of iriumd block time {}",
                i, header.time, iriumd_block_time
            ));
        }

        let work = prev_work.clone() + work_for_bits(header.bits);
        staged.push((
            hash,
            DogeHeaderEntry {
                header: header.clone(),
                height,
                total_work: work.clone(),
            },
        ));
        prev_hash = hash;
        prev_height = height;
        prev_work = work;
    }

    let tip_before = *doge_tip;
    let tip_height_before = *doge_tip_height;

    let final_hash = staged.last().unwrap().0;
    let final_height = staged.last().unwrap().1.height;
    let final_work = staged.last().unwrap().1.total_work.clone();

    let mut headers_added: Vec<[u8; 32]> = Vec::with_capacity(staged.len());
    for (hash, entry) in staged {
        headers_added.push(hash);
        doge_heights.insert(hash, entry.height);
        doge_headers.insert(hash, entry);
    }

    let current_tip_work = match tip_before {
        Some(h) => doge_headers
            .get(&h)
            .map(|e| e.total_work.clone())
            .unwrap_or_else(BigUint::zero),
        None => work_for_bits(anchor.bits),
    };
    if final_work > current_tip_work {
        *doge_tip = Some(final_hash);
        *doge_tip_height = final_height;
    }

    Ok(DogeRelayUpdate {
        tip_before,
        tip_height_before,
        headers_added,
    })
}

/// PR-5 of issue #68: legacy entry point for tests / callers that
/// don't construct `ParsedDogeHeader` themselves. Wraps each `DogeHeader`
/// as `ParsedDogeHeader { header, auxpow: None }` and delegates to
/// `apply_doge_header_batch_with_auxpow`. Behavior is identical to the
/// pre-PR-5 implementation for pre-371,337 DOGE headers (standalone
/// Scrypt). Post-371,337 headers with the AuxPoW bit set would fail
/// the "has AuxPoW bit set but no AuxPoW data attached" error here —
/// callers wanting AuxPoW validation must use the new function
/// directly.
pub fn apply_doge_header_batch(
    headers: Vec<DogeHeader>,
    iriumd_block_time: u32,
    doge_headers: &mut HashMap<[u8; 32], DogeHeaderEntry>,
    doge_heights: &mut HashMap<[u8; 32], u64>,
    doge_tip: &mut Option<[u8; 32]>,
    doge_tip_height: &mut u64,
    anchor: &DogeAnchor,
    retarget: &DigishieldParams,
) -> Result<DogeRelayUpdate, String> {
    let items: Vec<ParsedDogeHeader> = headers
        .into_iter()
        .map(|h| ParsedDogeHeader { header: h, auxpow: None })
        .collect();
    apply_doge_header_batch_with_auxpow(
        items,
        iriumd_block_time,
        doge_headers,
        doge_heights,
        doge_tip,
        doge_tip_height,
        anchor,
        retarget,
    )
}

/// Reverse a previously-applied `DogeRelayUpdate`. Removes inserted
/// headers from the committed maps and restores tip pointers. Called
/// from the iriumd block-disconnect path during a reorg, symmetric
/// to `undo_btc_relay_update` and `undo_ltc_relay_update`.
pub fn undo_doge_relay_update(
    update: &DogeRelayUpdate,
    doge_headers: &mut HashMap<[u8; 32], DogeHeaderEntry>,
    doge_heights: &mut HashMap<[u8; 32], u64>,
    doge_tip: &mut Option<[u8; 32]>,
    doge_tip_height: &mut u64,
) {
    for hash in &update.headers_added {
        doge_headers.remove(hash);
        doge_heights.remove(hash);
    }
    *doge_tip = update.tip_before;
    *doge_tip_height = update.tip_height_before;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trivially-low difficulty target used by the mining helper below.
    /// At `0x207fffff` almost every scrypt attempt at nonce 0 succeeds,
    /// keeping unit tests fast despite scrypt's ~10 ms-per-attempt cost.
    fn regtest_bits() -> u32 {
        0x207f_ffff
    }

    fn mine_doge_header(prev_hash: [u8; 32], time: u32, bits: u32) -> DogeHeader {
        let mut nonce: u32 = 0;
        loop {
            let header = DogeHeader {
                version: 1,
                prev_hash,
                merkle_root: [0u8; 32],
                time,
                bits,
                nonce,
            };
            if header.meets_pow() {
                return header;
            }
            nonce = nonce.wrapping_add(1);
        }
    }

    /// Build a fresh anchor for unit tests. `prev_time` is set 60 s
    /// before `time` so the Digishield retarget of the first relayed
    /// header gets a clean 60-second actual timespan (matches target,
    /// so the algorithm returns the parent's bits unchanged).
    fn fresh_anchor() -> (DogeAnchor, DogeHeader) {
        let bits = regtest_bits();
        let anchor_header = mine_doge_header([0u8; 32], 1_700_000_000, bits);
        let anchor = DogeAnchor {
            hash: anchor_header.block_hash(),
            height: 6_224_800,
            bits,
            time: anchor_header.time,
            prev_time: anchor_header.time - 60,
        };
        (anchor, anchor_header)
    }

    #[test]
    fn header_serialize_roundtrip() {
        let h = DogeHeader {
            version: 0x2000_0001,
            prev_hash: [0xaa; 32],
            merkle_root: [0xbb; 32],
            time: 1_700_000_000,
            bits: 0x1a00_97af,
            nonce: 0xdead_beef,
        };
        let bytes = h.serialize();
        assert_eq!(bytes.len(), 80);
        let decoded = DogeHeader::deserialize(&bytes).expect("decode");
        assert_eq!(decoded, h);
    }

    #[test]
    fn header_deserialize_rejects_wrong_size() {
        assert!(DogeHeader::deserialize(&[0u8; 79]).is_err());
        assert!(DogeHeader::deserialize(&[0u8; 81]).is_err());
    }

    #[test]
    fn batch_encode_parse_roundtrip() {
        let bits = regtest_bits();
        let h1 = mine_doge_header([0u8; 32], 1000, bits);
        let h2 = mine_doge_header(h1.block_hash(), 1001, bits);
        let batch = encode_doge_header_batch(&[h1.clone(), h2.clone()]).expect("encode");
        assert_eq!(batch[0], DOGE_HEADER_BATCH_TAG);
        assert_eq!(batch[1], DOGE_HEADER_BATCH_VERSION);
        assert_eq!(u16::from_le_bytes([batch[2], batch[3]]), 2);
        let parsed = parse_doge_header_batch(&batch).expect("parse");
        assert_eq!(parsed, vec![h1, h2]);
    }

    #[test]
    fn batch_parse_rejects_wrong_tag() {
        let mut script = vec![0xc6, 0x01, 0x01, 0x00];
        script.extend_from_slice(&[0u8; DOGE_HEADER_BYTES]);
        assert!(parse_doge_header_batch(&script).is_err());
    }

    #[test]
    fn batch_parse_rejects_zero_count() {
        let script = vec![DOGE_HEADER_BATCH_TAG, DOGE_HEADER_BATCH_VERSION, 0, 0];
        assert!(parse_doge_header_batch(&script).is_err());
    }

    #[test]
    fn batch_parse_rejects_oversize_count() {
        let mut script = vec![DOGE_HEADER_BATCH_TAG, DOGE_HEADER_BATCH_VERSION];
        let oversize: u16 = MAX_DOGE_HEADERS_PER_BATCH + 1;
        script.extend_from_slice(&oversize.to_le_bytes());
        assert!(parse_doge_header_batch(&script).is_err());
    }

    #[test]
    fn batch_parse_rejects_count_mismatch() {
        // Count says 2 but payload only has 1 header's worth of bytes.
        let mut script = vec![DOGE_HEADER_BATCH_TAG, DOGE_HEADER_BATCH_VERSION, 2, 0];
        script.extend_from_slice(&[0u8; DOGE_HEADER_BYTES]);
        assert!(parse_doge_header_batch(&script).is_err());
    }

    #[test]
    fn target_to_compact_bits_roundtrip_dogecoin_min() {
        let target = Target {
            bits: DigishieldParams::DOGECOIN.max_target_bits,
        }
        .to_target();
        let bits = target_to_compact_bits(&target);
        assert_eq!(
            bits, DigishieldParams::DOGECOIN.max_target_bits,
            "round-trip must preserve canonical DOGE mainnet-min bits"
        );
    }

    #[test]
    fn target_to_compact_bits_roundtrip_real_difficulty() {
        let bits_in = MAINNET_DOGE_ANCHOR_BITS;
        let target = Target { bits: bits_in }.to_target();
        let bits_out = target_to_compact_bits(&target);
        assert_eq!(bits_out, bits_in);
    }

    #[test]
    fn work_for_bits_harder_target_is_more_work() {
        let easy = work_for_bits(DigishieldParams::DOGECOIN.max_target_bits);
        let hard = work_for_bits(MAINNET_DOGE_ANCHOR_BITS);
        assert!(hard > easy);
    }

    #[test]
    fn mainnet_activation_set_to_24800() {
        assert_eq!(
            MAINNET_DOGE_SPV_RELAY_ACTIVATION_HEIGHT,
            Some(24_800),
            "DOGE SPV mainnet activation height is set to 24_800"
        );
    }

    #[test]
    fn mainnet_anchor_mainnet_constructor_reverses_to_natural_order() {
        let anchor = DogeAnchor::mainnet();
        assert_eq!(anchor.height, MAINNET_DOGE_ANCHOR_HEIGHT);
        assert_eq!(anchor.bits, MAINNET_DOGE_ANCHOR_BITS);
        assert_eq!(anchor.time, MAINNET_DOGE_ANCHOR_TIME);
        assert_eq!(anchor.prev_time, MAINNET_DOGE_ANCHOR_PREV_TIME);
        let mut expected_natural = MAINNET_DOGE_ANCHOR_HASH_DISPLAY;
        expected_natural.reverse();
        assert_eq!(anchor.hash, expected_natural);
    }

    #[test]
    fn digishield_dogecoin_constants_are_self_consistent() {
        let p = DigishieldParams::DOGECOIN;
        assert_eq!(p.target_spacing_secs, 60);
        assert_eq!(p.damping_divisor, 8);
        assert_eq!(p.min_clamp_numerator, 3);
        assert_eq!(p.min_clamp_denominator, 4);
        assert_eq!(p.max_clamp_numerator, 3);
        assert_eq!(p.max_clamp_denominator, 2);
        assert_eq!(p.max_target_bits, 0x1e0f_ffff);
        // Derived clamp bounds: 45 s lower, 90 s upper.
        let target_ts = p.target_spacing_secs as i64;
        let min_ts = target_ts * (p.min_clamp_numerator as i64)
            / (p.min_clamp_denominator as i64);
        let max_ts = target_ts * (p.max_clamp_numerator as i64)
            / (p.max_clamp_denominator as i64);
        assert_eq!(min_ts, 45);
        assert_eq!(max_ts, 90);
    }

    #[test]
    fn digishield_on_target_returns_parent_bits() {
        // actual_ts = target_ts → modulated = target_ts, clamp passthrough,
        // new_target = parent_target * 1 → bits unchanged (modulo
        // canonical compaction).
        let params = DigishieldParams::DOGECOIN;
        let parent_bits = 0x1a00_97af;
        let parent_time: u32 = 1_779_953_962;
        let grandparent_time: u32 = parent_time - params.target_spacing_secs;
        let out = expected_bits_digishield(parent_bits, parent_time, grandparent_time, &params);
        // Canonical re-encoding can differ from arbitrary input bits;
        // assert by comparing decoded targets, like the apply path does.
        let want = Target { bits: parent_bits }.to_target();
        let got = Target { bits: out }.to_target();
        assert_eq!(got, want);
    }

    /// Helper for Digishield asserts: replays the function's own
    /// arithmetic to derive the expected canonical bits, then compares
    /// bits-to-bits. Decoding `out` back to a BigUint would lose the
    /// mantissa truncation that compact-bits encoding applies, so the
    /// raw target form is not directly comparable.
    fn expected_digishield_bits(
        parent_bits: u32,
        clamped_timespan: i64,
        params: &DigishieldParams,
    ) -> u32 {
        let target_ts = params.target_spacing_secs as i64;
        let raw_target = Target { bits: parent_bits }.to_target()
            * BigUint::from(clamped_timespan.max(0) as u64)
            / BigUint::from(target_ts.max(1) as u64);
        let max_target = doge_max_target(params);
        let final_target = if raw_target > max_target {
            max_target
        } else {
            raw_target
        };
        target_to_compact_bits(&final_target)
    }

    #[test]
    fn digishield_clamp_lower_bound_at_extreme_fast_blocks() {
        // actual_ts = 0 (every block landed at the same second). Damped
        // modulated = 60 + (0 - 60)/8 = 60 - 7 = 53 (int div). 53 > 45
        // so the clamp itself doesn't fire here — verify the
        // post-damping value 53 is what the function uses, and
        // separately verify the clamp formula at its true lower
        // boundary.
        let params = DigishieldParams::DOGECOIN;
        let parent_bits = 0x1a00_97af;
        let parent_time: u32 = 1_779_953_962;
        let grandparent_time = parent_time; // actual_ts = 0

        let out = expected_bits_digishield(parent_bits, parent_time, grandparent_time, &params);
        // 60 + (0 - 60)/8 == 60 + (-60/8) == 60 + (-7) == 53.
        let expected = expected_digishield_bits(parent_bits, 53, &params);
        assert_eq!(out, expected, "damping (no clamp) result");

        // Direct clamp-formula verification at the true lower edge.
        let target_ts = params.target_spacing_secs as i64;
        let min_ts = target_ts * (params.min_clamp_numerator as i64)
            / (params.min_clamp_denominator as i64);
        assert_eq!(min_ts, 45);
        let way_below: i64 = 0;
        assert_eq!(way_below.clamp(min_ts, target_ts * 10), 45);
    }

    #[test]
    fn digishield_clamp_upper_bound_at_extreme_slow_blocks() {
        // 10-minute parent/grandparent gap: actual = 600s,
        // modulated = 60 + (600 - 60)/8 = 60 + 67 = 127. Clamps to 90.
        let params = DigishieldParams::DOGECOIN;
        let parent_bits = 0x1a00_97af;
        let parent_time: u32 = 1_779_953_962;
        let grandparent_time = parent_time - 600;

        let out = expected_bits_digishield(parent_bits, parent_time, grandparent_time, &params);
        // After clamp the effective timespan must be 90 (the max).
        let expected = expected_digishield_bits(parent_bits, 90, &params);
        assert_eq!(out, expected, "upper-clamp result");

        // Sanity: the produced target is strictly easier than the
        // parent (slower blocks → easier next target).
        let parent_target = Target { bits: parent_bits }.to_target();
        let new_target = Target { bits: out }.to_target();
        assert!(new_target > parent_target, "slower blocks → easier target");
    }

    #[test]
    fn digishield_damping_halves_deviation_amplitude() {
        // actual_ts = 120 (double target). modulated = 60 + (120-60)/8
        // = 60 + 7 = 67 (int div). Without damping a 2x-spacing block
        // would land on a 2x target; with damping it lands on a
        // 67/60 ≈ 1.117x target. Verify the magnitude.
        let params = DigishieldParams::DOGECOIN;
        let parent_bits = 0x1a00_97af;
        let parent_time: u32 = 1_779_953_962;
        let grandparent_time = parent_time - 120;

        let out = expected_bits_digishield(parent_bits, parent_time, grandparent_time, &params);
        let expected = expected_digishield_bits(parent_bits, 67, &params);
        assert_eq!(out, expected, "damping result");

        // Sanity: new target is between parent target and 2x parent —
        // i.e. damping prevented a 2x swing.
        let parent_target = Target { bits: parent_bits }.to_target();
        let new_target = Target { bits: out }.to_target();
        let doubled = parent_target.clone() * BigUint::from(2u8);
        assert!(new_target > parent_target, "slower blocks → easier target");
        assert!(new_target < doubled, "damping kept the target below 2x parent");
    }

    #[test]
    fn apply_rejects_when_anchor_not_configured() {
        let bits = regtest_bits();
        let h = mine_doge_header([0u8; 32], 1000, bits);
        let mut headers_db: HashMap<[u8; 32], DogeHeaderEntry> = HashMap::new();
        let mut heights_db: HashMap<[u8; 32], u64> = HashMap::new();
        let mut tip: Option<[u8; 32]> = None;
        let mut tip_height: u64 = 0;
        let zero_anchor = DogeAnchor::zero();
        let res = apply_doge_header_batch(
            vec![h],
            1_000_000,
            &mut headers_db,
            &mut heights_db,
            &mut tip,
            &mut tip_height,
            &zero_anchor,
            &DigishieldParams::DOGECOIN,
        );
        assert!(res.is_err());
    }

    #[test]
    fn apply_rejects_empty_batch() {
        let (anchor, _) = fresh_anchor();
        let mut headers_db: HashMap<[u8; 32], DogeHeaderEntry> = HashMap::new();
        let mut heights_db: HashMap<[u8; 32], u64> = HashMap::new();
        let mut tip: Option<[u8; 32]> = None;
        let mut tip_height: u64 = 0;
        let res = apply_doge_header_batch(
            vec![],
            1_000_000,
            &mut headers_db,
            &mut heights_db,
            &mut tip,
            &mut tip_height,
            &anchor,
            &DigishieldParams::DOGECOIN,
        );
        assert!(res.is_err());
    }

    /// Test-only Digishield params used in apply-path tests. The
    /// production `DOGECOIN` constants would require the regtest mined
    /// headers to also carry expected_bits matching the retarget for
    /// their inputs; using a passthrough params (target_spacing equal
    /// to the actual timespan we feed in, damping high enough that
    /// modulated == target_ts) keeps the apply test focused on the
    /// linkage/MTP/PoW logic without retargeting noise.
    fn passthrough_digishield(bits: u32) -> DigishieldParams {
        DigishieldParams {
            target_spacing_secs: 60,
            damping_divisor: 1,
            min_clamp_numerator: 1,
            min_clamp_denominator: 1,
            max_clamp_numerator: 1,
            max_clamp_denominator: 1,
            max_target_bits: bits,
        }
    }

    #[test]
    fn apply_extends_anchor_and_sets_tip() {
        let (anchor, anchor_header) = fresh_anchor();
        // With passthrough Digishield (clamp at 60/60), any timespan
        // collapses to 60 s and new_target == parent_target. The mined
        // header therefore must carry parent.bits.
        let h1 = mine_doge_header(anchor.hash, anchor_header.time + 60, anchor.bits);
        let mut headers_db: HashMap<[u8; 32], DogeHeaderEntry> = HashMap::new();
        let mut heights_db: HashMap<[u8; 32], u64> = HashMap::new();
        let mut tip: Option<[u8; 32]> = None;
        let mut tip_height: u64 = 0;

        let params = passthrough_digishield(anchor.bits);
        let update = apply_doge_header_batch(
            vec![h1.clone()],
            anchor_header.time + 60,
            &mut headers_db,
            &mut heights_db,
            &mut tip,
            &mut tip_height,
            &anchor,
            &params,
        )
        .expect("apply");

        assert_eq!(update.tip_before, None);
        assert_eq!(update.tip_height_before, 0);
        assert_eq!(update.headers_added, vec![h1.block_hash()]);
        assert_eq!(tip, Some(h1.block_hash()));
        assert_eq!(tip_height, anchor.height + 1);
        assert!(headers_db.contains_key(&h1.block_hash()));
        assert_eq!(*heights_db.get(&h1.block_hash()).unwrap(), anchor.height + 1);
    }

    #[test]
    fn apply_then_undo_restores_state() {
        let (anchor, anchor_header) = fresh_anchor();
        let params = passthrough_digishield(anchor.bits);
        let h1 = mine_doge_header(anchor.hash, anchor_header.time + 60, anchor.bits);
        let h2 = mine_doge_header(h1.block_hash(), anchor_header.time + 120, anchor.bits);
        let mut headers_db: HashMap<[u8; 32], DogeHeaderEntry> = HashMap::new();
        let mut heights_db: HashMap<[u8; 32], u64> = HashMap::new();
        let mut tip: Option<[u8; 32]> = None;
        let mut tip_height: u64 = 0;

        let update = apply_doge_header_batch(
            vec![h1.clone(), h2.clone()],
            anchor_header.time + 120,
            &mut headers_db,
            &mut heights_db,
            &mut tip,
            &mut tip_height,
            &anchor,
            &params,
        )
        .expect("apply");

        assert_eq!(headers_db.len(), 2);
        assert_eq!(tip, Some(h2.block_hash()));
        assert_eq!(tip_height, anchor.height + 2);

        undo_doge_relay_update(
            &update,
            &mut headers_db,
            &mut heights_db,
            &mut tip,
            &mut tip_height,
        );

        assert!(headers_db.is_empty());
        assert!(heights_db.is_empty());
        assert_eq!(tip, None);
        assert_eq!(tip_height, 0);
    }

    #[test]
    fn apply_rejects_bad_linkage() {
        let (anchor, anchor_header) = fresh_anchor();
        let params = passthrough_digishield(anchor.bits);
        let h1 = mine_doge_header(anchor.hash, anchor_header.time + 60, anchor.bits);
        let bad = mine_doge_header([0xee; 32], anchor_header.time + 120, anchor.bits);
        let mut headers_db = HashMap::new();
        let mut heights_db = HashMap::new();
        let mut tip = None;
        let mut tip_height = 0;

        let res = apply_doge_header_batch(
            vec![h1, bad],
            anchor_header.time + 120,
            &mut headers_db,
            &mut heights_db,
            &mut tip,
            &mut tip_height,
            &anchor,
            &params,
        );
        assert!(res.is_err());
        assert!(
            headers_db.is_empty(),
            "no state should leak when batch is rejected"
        );
        assert!(tip.is_none());
    }

    #[test]
    fn apply_rejects_oversize_batch() {
        // Synthesise a 145-header vec of placeholder headers; we expect
        // the size-cap check to fire before any expensive PoW work.
        let (anchor, _anchor_header) = fresh_anchor();
        let dummy = DogeHeader {
            version: 0,
            prev_hash: [0u8; 32],
            merkle_root: [0u8; 32],
            time: 0,
            bits: 0,
            nonce: 0,
        };
        let oversize: Vec<DogeHeader> = std::iter::repeat_n(dummy, MAX_DOGE_HEADERS_PER_BATCH as usize + 1)
            .collect();
        let mut headers_db = HashMap::new();
        let mut heights_db = HashMap::new();
        let mut tip = None;
        let mut tip_height = 0;
        let res = apply_doge_header_batch(
            oversize,
            1_000_000,
            &mut headers_db,
            &mut heights_db,
            &mut tip,
            &mut tip_height,
            &anchor,
            &DigishieldParams::DOGECOIN,
        );
        assert!(res.is_err());
    }

    // ====================================================================
    // PR-4 of issue #68 — v0x02 wire format tests
    // ====================================================================
    // Real-mainnet-fixture-based test deferred to PR-5 (where the AuxPoW
    // validator wiring lands); these 6 tests cover the format-level
    // round-trip + rejection paths synthetically.

    #[test]
    fn pr4_parse_v01_still_works_after_dispatcher_added() {
        // Regression: existing v0x01 batches must still parse via the
        // chain-consensus path (parse_doge_header_batch, v0x01-only).
        let h1 = DogeHeader {
            version: 1, prev_hash: [0u8; 32], merkle_root: [1u8; 32],
            time: 100, bits: 0x1d00ffff, nonce: 0,
        };
        let batch = encode_doge_header_batch(&[h1.clone()]).unwrap();
        let parsed = parse_doge_header_batch(&batch).unwrap();
        assert_eq!(parsed, vec![h1]);
    }

    #[test]
    fn pr4_parse_v01_via_with_auxpow_returns_none_per_header() {
        // v0x01 batches parsed through the v0x02-capable function yield
        // None for the auxpow field on every header.
        let h = DogeHeader {
            version: 1, prev_hash: [0u8; 32], merkle_root: [2u8; 32],
            time: 200, bits: 0x1d00ffff, nonce: 0,
        };
        let batch = encode_doge_header_batch(&[h.clone()]).unwrap();
        let parsed = parse_doge_header_batch_with_auxpow(&batch).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].header, h);
        assert_eq!(parsed[0].auxpow, None);
    }

    #[test]
    fn pr4_parse_v02_rejected_by_consensus_path() {
        // SAFETY: parse_doge_header_batch (used by chain.rs) must
        // reject v0x02 batches. Without an activation gate in chain.rs,
        // accepting them would let a malicious miner pre-activate
        // AuxPoW behavior. Issue #68 PR-4 keeps this gate.
        let h = DogeHeader {
            version: 1, prev_hash: [0u8; 32], merkle_root: [3u8; 32],
            time: 300, bits: 0x1d00ffff, nonce: 0,
        };
        let batch = encode_doge_header_batch_with_auxpow(&[
            ParsedDogeHeader { header: h, auxpow: None },
        ]).unwrap();
        assert_eq!(batch[1], DOGE_HEADER_BATCH_VERSION_V2);
        let err = parse_doge_header_batch(&batch).unwrap_err();
        assert!(
            err.contains("unknown version"),
            "expected 'unknown version' rejection, got: {}", err,
        );
    }

    #[test]
    fn pr4_parse_v02_round_trip_no_auxpow() {
        // Encode v0x02 with all headers flagged has_auxpow=0; verify
        // round-trip via the v0x02-capable parser.
        let h1 = DogeHeader {
            version: 1, prev_hash: [0u8; 32], merkle_root: [4u8; 32],
            time: 400, bits: 0x1d00ffff, nonce: 0,
        };
        let h2 = DogeHeader {
            version: 1, prev_hash: h1.block_hash(), merkle_root: [5u8; 32],
            time: 410, bits: 0x1d00ffff, nonce: 0,
        };
        let items = vec![
            ParsedDogeHeader { header: h1, auxpow: None },
            ParsedDogeHeader { header: h2, auxpow: None },
        ];
        let bytes = encode_doge_header_batch_with_auxpow(&items).unwrap();
        let parsed = parse_doge_header_batch_with_auxpow(&bytes).unwrap();
        assert_eq!(parsed, items);
    }

    #[test]
    fn pr4_parse_v02_round_trip_with_auxpow() {
        // Round-trip a v0x02 batch where headers carry synthetic
        // (non-empty) auxpow bytes. Verifies the varint length encoding
        // and reading works for typical sizes.
        let h = DogeHeader {
            version: 0x00620102_u32 as i32, prev_hash: [0u8; 32],
            merkle_root: [6u8; 32], time: 500, bits: 0x1d00ffff, nonce: 0,
        };
        let auxpow_bytes: Vec<u8> = (0..512u16).map(|i| (i & 0xff) as u8).collect();
        let items = vec![
            ParsedDogeHeader { header: h.clone(), auxpow: Some(auxpow_bytes.clone()) },
        ];
        let bytes = encode_doge_header_batch_with_auxpow(&items).unwrap();
        assert_eq!(bytes[1], DOGE_HEADER_BATCH_VERSION_V2);
        let parsed = parse_doge_header_batch_with_auxpow(&bytes).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].header, h);
        assert_eq!(parsed[0].auxpow.as_deref(), Some(auxpow_bytes.as_slice()));
    }

    #[test]
    fn pr4_parse_v02_rejects_unknown_flag() {
        // The has_auxpow flag byte must be 0x00 or 0x01. Anything else
        // is rejected, preventing malformed batches from sneaking past.
        let mut batch = Vec::new();
        batch.push(DOGE_HEADER_BATCH_TAG);
        batch.push(DOGE_HEADER_BATCH_VERSION_V2);
        batch.extend_from_slice(&1u16.to_le_bytes());
        batch.extend_from_slice(&[0u8; DOGE_HEADER_BYTES]);
        batch.push(0x42); // invalid flag
        let err = parse_doge_header_batch_with_auxpow(&batch).unwrap_err();
        assert!(
            err.contains("invalid has_auxpow flag"),
            "expected 'invalid has_auxpow flag' error, got: {}", err,
        );
    }

    #[test]
    fn pr4_parse_v02_rejects_oversized_auxpow() {
        // auxpow_len > MAX_DOGE_AUXPOW_BYTES must be rejected at the
        // length-prefix step BEFORE allocating / consuming the bytes.
        // Defense against memory-exhaustion attacks via malformed
        // batches.
        let mut batch = Vec::new();
        batch.push(DOGE_HEADER_BATCH_TAG);
        batch.push(DOGE_HEADER_BATCH_VERSION_V2);
        batch.extend_from_slice(&1u16.to_le_bytes());
        batch.extend_from_slice(&[0u8; DOGE_HEADER_BYTES]);
        batch.push(0x01); // has_auxpow
        // 4-byte varint header (0xfe) + a length larger than the cap
        batch.push(0xfe);
        batch.extend_from_slice(&(MAX_DOGE_AUXPOW_BYTES as u32 + 1).to_le_bytes());
        let err = parse_doge_header_batch_with_auxpow(&batch).unwrap_err();
        assert!(
            err.contains("exceeds cap"),
            "expected 'exceeds cap' error, got: {}", err,
        );
    }

    // ====================================================================
    // PR-5 of issue #68 — AuxPoW validator wiring tests
    // ====================================================================

    /// Pre-activation DOGE headers (DOGE height < 371,337) in a v0x02
    /// batch with `auxpow: None` must use the standalone Scrypt path
    /// via the legacy wrapper. This is the most common path for
    /// historical batches.
    #[test]
    fn pr5_apply_pre_activation_path_uses_standalone_scrypt() {
        // Use the legacy wrapper, which delegates to
        // apply_doge_header_batch_with_auxpow with auxpow: None for
        // every header. Any header that doesn't pass standalone Scrypt
        // (which is true for synthetic headers) should produce the
        // "fails PoW" error — proving the standalone-Scrypt path is
        // exercised even via the new function.
        let (anchor, anchor_header) = fresh_anchor();
        let synthetic = DogeHeader {
            version: 1, // No AuxPoW bit
            prev_hash: anchor.hash,
            merkle_root: [0u8; 32],
            time: anchor_header.time + 60,
            bits: anchor.bits,
            nonce: 0,
        };
        let mut headers_db = HashMap::new();
        let mut heights_db = HashMap::new();
        let mut tip = None;
        let mut tip_height = 0;
        let res = apply_doge_header_batch(
            vec![synthetic],
            anchor_header.time + 120,
            &mut headers_db,
            &mut heights_db,
            &mut tip,
            &mut tip_height,
            &anchor,
            &DigishieldParams::DOGECOIN,
        );
        let err = res.unwrap_err();
        assert!(
            err.contains("fails PoW") || err.contains("bits mismatch") || err.contains("MTP"),
            "expected standalone-Scrypt path error, got: {}", err,
        );
    }

    /// Post-activation DOGE header with the AuxPoW bit set but no
    /// AuxPoW data attached must error with the specific "has AuxPoW
    /// bit set but no AuxPoW data" message. This proves the
    /// per-header dispatch is reading version & 0x100 correctly.
    #[test]
    fn pr5_apply_post_activation_auxpow_bit_requires_auxpow_data() {
        // Set up an anchor at DOGE height 371_336 so the next header
        // is at 371_337 (the AuxPoW activation boundary).
        let anchor = DogeAnchor {
            hash: [0xaau8; 32],
            height: 371_336,
            bits: 0x1d00ffff,
            time: 1_410_464_500,
            prev_time: 1_410_464_440,
        };
        let header_at_371_337 = DogeHeader {
            // 0x100 bit set → AuxPoW required at this height
            version: 0x00010102_i32,
            prev_hash: anchor.hash,
            merkle_root: [0u8; 32],
            time: anchor.time + 60,
            bits: anchor.bits,
            nonce: 0,
        };
        let items = vec![ParsedDogeHeader {
            header: header_at_371_337,
            auxpow: None, // missing the required AuxPoW data
        }];
        let mut headers_db = HashMap::new();
        let mut heights_db = HashMap::new();
        let mut tip = None;
        let mut tip_height = 0;
        let res = apply_doge_header_batch_with_auxpow(
            items,
            anchor.time + 120,
            &mut headers_db,
            &mut heights_db,
            &mut tip,
            &mut tip_height,
            &anchor,
            &DigishieldParams::DOGECOIN,
        );
        let err = res.unwrap_err();
        assert!(
            err.contains("has AuxPoW bit set but no AuxPoW data attached"),
            "expected missing-auxpow error, got: {}", err,
        );
    }

    /// Post-activation DOGE header WITHOUT the AuxPoW bit set should
    /// still attempt standalone Scrypt (per DOGE Core's behavior —
    /// pools may emit either type after the boundary). The expected
    /// failure here is from standalone Scrypt failing on the synthetic
    /// header, NOT from AuxPoW being mandatory.
    #[test]
    fn pr5_apply_post_activation_no_auxpow_bit_uses_standalone_scrypt() {
        let anchor = DogeAnchor {
            hash: [0xbbu8; 32],
            height: 371_336,
            bits: 0x1d00ffff,
            time: 1_410_464_500,
            prev_time: 1_410_464_440,
        };
        let header_at_371_337 = DogeHeader {
            // AuxPoW bit NOT set → standalone Scrypt path
            version: 0x00010002_i32,
            prev_hash: anchor.hash,
            merkle_root: [0u8; 32],
            time: anchor.time + 60,
            bits: anchor.bits,
            nonce: 0,
        };
        let items = vec![ParsedDogeHeader {
            header: header_at_371_337,
            auxpow: None,
        }];
        let mut headers_db = HashMap::new();
        let mut heights_db = HashMap::new();
        let mut tip = None;
        let mut tip_height = 0;
        let res = apply_doge_header_batch_with_auxpow(
            items,
            anchor.time + 120,
            &mut headers_db,
            &mut heights_db,
            &mut tip,
            &mut tip_height,
            &anchor,
            &DigishieldParams::DOGECOIN,
        );
        let err = res.unwrap_err();
        assert!(
            err.contains("fails PoW") || err.contains("bits mismatch") || err.contains("MTP"),
            "expected standalone-Scrypt path error, got: {}", err,
        );
        assert!(
            !err.contains("has AuxPoW bit set"),
            "should NOT trigger missing-auxpow error when bit is clear, got: {}", err,
        );
    }

    /// Real mainnet DOGE block 371,337 AuxPoW validation. If this
    /// breaks, the AuxPoW validator no longer correctly handles the
    /// historic merge-mining transition data — a consensus-breaking
    /// regression. Fixture source:
    /// tests/fixtures/doge_auxpow/block_371337.json (PR-3).
    ///
    /// Byte-order note: blockchair's JSON returns merkle branch hashes
    /// as 32-byte hex strings. PR-5 first attempt uses NATURAL byte
    /// order (no reversal). If validation fails, try the reversed
    /// orientation — Bitcoin RPC outputs are sometimes display-order.
    #[test]
    fn pr5_validate_block_371337_real_mainnet_auxpow() {
        // Helper: hex → [u8; 32], optionally byte-reversed (for the
        // prev_hash and merkle_root fields which are display-order in
        // explorers but on-wire little-endian).
        fn h32(s: &str, reverse: bool) -> [u8; 32] {
            let mut v = hex::decode(s).unwrap();
            if reverse {
                v.reverse();
            }
            v.try_into().unwrap()
        }

        let coinbase_txn = hex::decode("01000000010000000000000000000000000000000000000000000000000000000000000000ffffffff380345bf09fabe6d6d980ba42120410de0554d42a5b5ee58167bcd86bf7591f429005f24da45fb51cf0800000000000000cdb1f1ff0e000000ffffffff01800c0c2a010000001976a914aa3750aa18b8a0f3f0590731e1fab934856680cf88ac00000000").unwrap();
        let mut parent_header = [0u8; 80];
        parent_header.copy_from_slice(&hex::decode("02000000d2ec7dfeb7e8f43fe77aba3368df95ac2088034420402730ee0492a2084217083411b3fc91033bfdeea339bc11b9efc986e161c703e07a9045338c165673f09940fb11548b54021b58cc9ae5").unwrap());

        // Use natural byte order for branches (no reversal). If this
        // fails empirically, flip to true.
        let branch_reverse = true;
        let coinbase_branch = vec![
            h32("cd3947cd5a0c26fde01b05a3aa3d7a38717be6ae11d27239365024db36a679a9", branch_reverse),
            h32("48f9e8fef3411944e27f49ec804462c9e124dca0954c71c8560e8a9dd218a452", branch_reverse),
            h32("d11293660392e7c51f69477a6130237c72ecee2d0c1d3dc815841734c370331a", branch_reverse),
        ];
        let blockchain_branch = vec![
            h32("b541c848bc001d07d2bdf8643abab61d2c6ae50d5b2495815339a4b30703a46f", branch_reverse),
            h32("78d6abe48cee514cf3496f4042039acb7e27616dcfc5de926ff0d6c7e5987be7", branch_reverse),
            h32("a0469413ce64d67c43902d54ee3a380eff12ded22ca11cbd3842e15d48298103", branch_reverse),
        ];

        let auxpow = crate::auxpow::AuxPoW {
            coinbase_txn,
            parent_hash: crate::pow::sha256d(&parent_header),
            coinbase_branch,
            coinbase_branch_index: 0,
            blockchain_branch,
            blockchain_branch_index: 0,
            parent_header,
        };

        // DOGE block 371,337 header. version 0x00620102 has the AuxPoW
        // bit (0x100). prev_hash and merkle_root are display-order in
        // explorers; reverse to get on-wire little-endian bytes.
        let doge_header = DogeHeader {
            version: 0x00620102_i32,
            prev_hash: h32(
                "46a8b109fb016fa41abd17a19186ca78d39c60c020c71fcd2690320d47036f0d",
                true,
            ),
            merkle_root: h32(
                "ee27b8fb782a5bfb99c975f0d4686440b9af9e16846603e5f2830e0b6fbf158a",
                true,
            ),
            time: 1_410_464_577,
            bits: 0x1b364184,
            nonce: 0,
        };

                let result = doge_header.meets_pow_auxpow(&auxpow);
        assert!(
            result,
            "Real mainnet DOGE block 371,337 AuxPoW validation FAILED              after the PR-2 byte-order fix. If this fires, either the fix              regressed or the fixture data is wrong.",
        );
    }
}
