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
}

/// One stored Dogecoin header plus its derived chain metadata.
#[derive(Debug, Clone)]
pub struct DogeHeaderEntry {
    pub header: DogeHeader,
    pub height: u64,
    pub total_work: BigUint,
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
    if headers.is_empty() {
        return Err("apply_doge_header_batch: empty batch".to_string());
    }
    if headers.len() > MAX_DOGE_HEADERS_PER_BATCH as usize {
        return Err(format!(
            "apply_doge_header_batch: {} headers exceeds max {}",
            headers.len(),
            MAX_DOGE_HEADERS_PER_BATCH
        ));
    }
    if anchor.is_zero() {
        return Err("apply_doge_header_batch: anchor not configured".to_string());
    }

    // Parallel PoW pre-check. scrypt costs ~10 ms / header; running them
    // sequentially for a full batch is >1 s wall-clock. par_iter fans
    // the work across the rayon thread pool. We collect indices of any
    // failures and report the lowest one, matching the sequential
    // caller contract on the BTC and LTC sides.
    let mut pow_failures: Vec<usize> = headers
        .par_iter()
        .enumerate()
        .filter_map(|(i, h)| if h.meets_pow() { None } else { Some(i) })
        .collect();
    if !pow_failures.is_empty() {
        pow_failures.sort_unstable();
        return Err(format!(
            "apply_doge_header_batch: header {} fails PoW",
            pow_failures[0]
        ));
    }

    let first = &headers[0];
    let (start_prev_height, start_prev_work) = if first.prev_hash == anchor.hash {
        (anchor.height, work_for_bits(anchor.bits))
    } else {
        let parent = doge_headers.get(&first.prev_hash).ok_or_else(|| {
            "apply_doge_header_batch: first header does not connect to known chain".to_string()
        })?;
        (parent.height, parent.total_work.clone())
    };

    let mut prev_hash = first.prev_hash;
    let mut prev_height = start_prev_height;
    let mut prev_work = start_prev_work;
    let mut staged: Vec<([u8; 32], DogeHeaderEntry)> = Vec::with_capacity(headers.len());

    for (i, header) in headers.iter().enumerate() {
        if header.prev_hash != prev_hash {
            return Err(format!(
                "apply_doge_header_batch: header {} does not link to previous",
                i
            ));
        }
        let height = prev_height + 1;
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
}
