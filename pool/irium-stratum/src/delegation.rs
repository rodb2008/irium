//! Phase 18B step-2: non-custodial one-time PoAW-X delegation registration.
//!
//! This module is a STRATUM-LOCAL mirror of the consensus `Delegation` wire
//! format (defined in `irium_node_rs::poawx`). The stratum intentionally does
//! NOT depend on the full node crate in production (see `block.rs`, which
//! likewise mirrors the receipt-root algorithm); `irium-node-rs` is a
//! dev-dependency used only by the parity tests in this file, which assert the
//! mirror stays byte-identical to the canonical type. Any drift fails tests.
//!
//! The pool delegate key here is a signer-only identity: it signs delegated
//! receipt challenges (step 4). It is NEVER a payout identity and never appears
//! in a coinbase. The delegation registry stores public material only — no
//! private keys.
//!
//! Some items (e.g. `DelegateKey::secret`, `DelegationStore::all_active` /
//! `prune_expired`, `Delegation::digest`) are foundation consumed by the
//! step-4 receipt producer and are intentionally not yet called in production.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{info, warn};

/// Domain separator — MUST equal `irium_node_rs::poawx::DOMAIN_DELEG`.
pub const DELEG_DOMAIN: &[u8] = b"irium.poawx.delegation.v1";

// ── Stratum-local mirror of the consensus Delegation (226-byte wire) ─────────

#[derive(Debug, Clone, PartialEq)]
pub struct Delegation {
    pub deleg_version: u8,
    pub network_id: u8,
    pub miner_pubkey: [u8; 33],
    pub pool_pubkey: [u8; 33],
    pub worker_tag: [u8; 32],
    pub expiry_height: u64,
    pub fee_bps: u16,
    pub fee_pkh: [u8; 20],
    pub deleg_nonce: [u8; 32],
    pub delegation_sig: [u8; 64],
}

impl Delegation {
    pub const VERSION: u8 = 1;
    pub const WIRE_SIZE: usize = 1 + 1 + 33 + 33 + 32 + 8 + 2 + 20 + 32 + 64; // 226

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_SIZE);
        out.push(self.deleg_version);
        out.push(self.network_id);
        out.extend_from_slice(&self.miner_pubkey);
        out.extend_from_slice(&self.pool_pubkey);
        out.extend_from_slice(&self.worker_tag);
        out.extend_from_slice(&self.expiry_height.to_le_bytes());
        out.extend_from_slice(&self.fee_bps.to_le_bytes());
        out.extend_from_slice(&self.fee_pkh);
        out.extend_from_slice(&self.deleg_nonce);
        out.extend_from_slice(&self.delegation_sig);
        out
    }

    pub fn deserialize(raw: &[u8]) -> Result<Self, String> {
        if raw.len() < Self::WIRE_SIZE {
            return Err(format!(
                "delegation too short: {} < {}",
                raw.len(),
                Self::WIRE_SIZE
            ));
        }
        let mut off = 0usize;
        let deleg_version = raw[off];
        off += 1;
        let network_id = raw[off];
        off += 1;
        let mut miner_pubkey = [0u8; 33];
        miner_pubkey.copy_from_slice(&raw[off..off + 33]);
        off += 33;
        let mut pool_pubkey = [0u8; 33];
        pool_pubkey.copy_from_slice(&raw[off..off + 33]);
        off += 33;
        let mut worker_tag = [0u8; 32];
        worker_tag.copy_from_slice(&raw[off..off + 32]);
        off += 32;
        let expiry_height =
            u64::from_le_bytes(raw[off..off + 8].try_into().expect("slice len checked"));
        off += 8;
        let fee_bps = u16::from_le_bytes(raw[off..off + 2].try_into().expect("slice len checked"));
        off += 2;
        let mut fee_pkh = [0u8; 20];
        fee_pkh.copy_from_slice(&raw[off..off + 20]);
        off += 20;
        let mut deleg_nonce = [0u8; 32];
        deleg_nonce.copy_from_slice(&raw[off..off + 32]);
        off += 32;
        let mut delegation_sig = [0u8; 64];
        delegation_sig.copy_from_slice(&raw[off..off + 64]);
        Ok(Self {
            deleg_version,
            network_id,
            miner_pubkey,
            pool_pubkey,
            worker_tag,
            expiry_height,
            fee_bps,
            fee_pkh,
            deleg_nonce,
            delegation_sig,
        })
    }

    /// SHA256 over the domain + all fields EXCEPT the signature.
    pub fn message_hash(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(DELEG_DOMAIN);
        h.update([self.deleg_version]);
        h.update([self.network_id]);
        h.update(self.miner_pubkey);
        h.update(self.pool_pubkey);
        h.update(self.worker_tag);
        h.update(self.expiry_height.to_le_bytes());
        h.update(self.fee_bps.to_le_bytes());
        h.update(self.fee_pkh);
        h.update(self.deleg_nonce);
        h.finalize().into()
    }

    /// SHA256 over the full 226-byte serialization (including the signature).
    pub fn digest(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.serialize());
        h.finalize().into()
    }

    /// HASH160(miner_pubkey) — the miner pkh / payout identity.
    pub fn miner_pkh(&self) -> [u8; 20] {
        let sha = Sha256::digest(self.miner_pubkey);
        let rip = ripemd::Ripemd160::digest(sha);
        let mut pkh = [0u8; 20];
        pkh.copy_from_slice(&rip);
        pkh
    }

    /// Verify the miner's delegation signature over `message_hash()`.
    pub fn verify_signature(&self) -> Result<(), &'static str> {
        use k256::ecdsa::signature::hazmat::PrehashVerifier;
        use k256::ecdsa::{Signature, VerifyingKey};
        let vk = VerifyingKey::from_sec1_bytes(&self.miner_pubkey)
            .map_err(|_| "delegation: invalid miner_pubkey")?;
        let sig = Signature::from_slice(&self.delegation_sig)
            .map_err(|_| "delegation: malformed delegation_sig")?;
        vk.verify_prehash(&self.message_hash(), &sig)
            .map_err(|_| "delegation: signature verification failed")
    }
}

/// worker_tag = SHA256(worker) for a named worker, or all-zero when unscoped.
pub fn worker_tag(worker: &str) -> [u8; 32] {
    if worker.is_empty() {
        return [0u8; 32];
    }
    let mut h = Sha256::new();
    h.update(worker.as_bytes());
    h.finalize().into()
}

/// network_id mirror of `irium_node_rs::activation::NetworkKind::id_byte`:
/// Mainnet=0, Testnet=1, Devnet=2. Read from `IRIUM_NETWORK`.
pub fn network_id_from_env() -> u8 {
    match env::var("IRIUM_NETWORK")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "testnet" => 1,
        "devnet" | "regtest" | "trial" => 2,
        _ => 0, // mainnet / unset
    }
}

// ── Pool delegate key (signer-only; NOT a payout identity) ───────────────────

pub struct DelegateKey {
    secret: [u8; 32],
    pubkey: [u8; 33],
}

impl DelegateKey {
    /// Load the delegate key from `path`, or generate one if absent and
    /// `allow_generate` is true. Generation is only permitted in non-mainnet
    /// contexts (caller passes `allow_generate=false` on mainnet). The private
    /// key is written with 0600 permissions where supported and never logged.
    pub fn load_or_generate(path: &Path, allow_generate: bool) -> Result<Self, String> {
        if path.exists() {
            let hex_str = std::fs::read_to_string(path)
                .map_err(|e| format!("read delegate key {}: {e}", path.display()))?;
            let bytes = hex::decode(hex_str.trim())
                .map_err(|_| "delegate key file: invalid hex".to_string())?;
            if bytes.len() != 32 {
                return Err("delegate key file: expected 32 bytes".to_string());
            }
            let mut secret = [0u8; 32];
            secret.copy_from_slice(&bytes);
            let pubkey = pubkey_from_secret(&secret)?;
            Ok(Self { secret, pubkey })
        } else {
            if !allow_generate {
                return Err("delegate key missing and generation not allowed".to_string());
            }
            let secret = random_valid_secret()?;
            let pubkey = pubkey_from_secret(&secret)?;
            write_secret_file(path, &secret)?;
            Ok(Self { secret, pubkey })
        }
    }

    pub fn pubkey(&self) -> [u8; 33] {
        self.pubkey
    }
    pub fn pubkey_hex(&self) -> String {
        hex::encode(self.pubkey)
    }
    /// Signer-only secret, used to sign delegated receipt challenges in step 4.
    /// Never serialized into the delegation registry, never logged.
    pub fn secret(&self) -> &[u8; 32] {
        &self.secret
    }

    /// Sign the per-height receipt challenge `SHA256(solution‖commitment_nonce‖
    /// height_le8)` with the delegate key. This is the signer signature embedded
    /// in a mode-1 receipt (`worker_sig`); the consensus verifies it against the
    /// delegate pubkey, which the miner authorized via the delegation.
    pub fn sign_challenge(
        &self,
        solution: &[u8],
        commitment_nonce: &[u8; 32],
        height: u64,
    ) -> [u8; 64] {
        use k256::ecdsa::signature::hazmat::PrehashSigner;
        use k256::ecdsa::{Signature, SigningKey};
        let sk = SigningKey::from_slice(&self.secret).expect("delegate secret validated at load");
        let challenge: [u8; 32] = {
            let mut h = Sha256::new();
            h.update(solution);
            h.update(commitment_nonce);
            h.update(height.to_le_bytes());
            h.finalize().into()
        };
        let sig: Signature = sk.sign_prehash(&challenge).expect("sign challenge");
        let mut out = [0u8; 64];
        out.copy_from_slice(&sig.to_bytes());
        out
    }
}

fn pubkey_from_secret(secret: &[u8; 32]) -> Result<[u8; 33], String> {
    use k256::ecdsa::{SigningKey, VerifyingKey};
    let sk = SigningKey::from_slice(secret).map_err(|e| format!("invalid delegate secret: {e}"))?;
    let vk = VerifyingKey::from(&sk);
    let enc = vk.to_encoded_point(true);
    let mut pk = [0u8; 33];
    pk.copy_from_slice(enc.as_bytes());
    Ok(pk)
}

fn random_valid_secret() -> Result<[u8; 32], String> {
    use k256::ecdsa::SigningKey;
    use rand_core::{OsRng, RngCore};
    for _ in 0..64 {
        let mut b = [0u8; 32];
        OsRng.fill_bytes(&mut b);
        if SigningKey::from_slice(&b).is_ok() {
            return Ok(b);
        }
    }
    Err("failed to generate a valid delegate secret".to_string())
}

fn write_secret_file(path: &Path, secret: &[u8; 32]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create dir {}: {e}", parent.display()))?;
    }
    std::fs::write(path, hex::encode(secret))
        .map_err(|e| format!("write delegate key {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

// ── Delegation registry (no private keys) ────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredDelegation {
    /// Canonical 226-byte `Delegation::serialize()` hex — public material only.
    pub delegation_hex: String,
    pub miner_pkh: String,
    pub worker: String,
    pub network_id: u8,
    pub expiry_height: u64,
    pub fee_bps: u16,
    pub status: String,
    pub received_at_unix: u64,
}

pub fn deleg_key(miner_pkh_hex: &str, worker: &str) -> String {
    format!("{}.{}", miner_pkh_hex, worker)
}

pub trait DelegationStore: Send + Sync {
    fn get(&self, miner_pkh_hex: &str, worker: &str) -> Option<StoredDelegation>;
    fn put(&self, rec: StoredDelegation) -> Result<(), String>;
    fn all_active(&self, tip_height: u64) -> Vec<StoredDelegation>;
    fn prune_expired(&self, tip_height: u64) -> usize;
}

#[derive(Serialize, Deserialize, Default)]
struct DelegFile {
    version: u32,
    delegations: BTreeMap<String, StoredDelegation>,
}

pub struct JsonDelegationStore {
    path: PathBuf,
    inner: Mutex<BTreeMap<String, StoredDelegation>>,
}

impl JsonDelegationStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let map = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| format!("read delegations {}: {e}", path.display()))?;
            let file: DelegFile = serde_json::from_str(&raw).unwrap_or_default();
            file.delegations
        } else {
            BTreeMap::new()
        };
        Ok(Self {
            path,
            inner: Mutex::new(map),
        })
    }

    fn flush(&self, map: &BTreeMap<String, StoredDelegation>) -> Result<(), String> {
        let file = DelegFile {
            version: 1,
            delegations: map.clone(),
        };
        let json = serde_json::to_string_pretty(&file)
            .map_err(|e| format!("serialize delegations: {e}"))?;
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| format!("write delegations tmp: {e}"))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("rename delegations: {e}"))?;
        Ok(())
    }
}

impl DelegationStore for JsonDelegationStore {
    fn get(&self, miner_pkh_hex: &str, worker: &str) -> Option<StoredDelegation> {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.get(&deleg_key(miner_pkh_hex, worker)).cloned()
    }

    fn put(&self, rec: StoredDelegation) -> Result<(), String> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(deleg_key(&rec.miner_pkh, &rec.worker), rec);
        self.flush(&map)
    }

    fn all_active(&self, tip_height: u64) -> Vec<StoredDelegation> {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.values()
            .filter(|r| r.status == "active" && r.expiry_height > tip_height)
            .cloned()
            .collect()
    }

    fn prune_expired(&self, tip_height: u64) -> usize {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let before = map.len();
        map.retain(|_, r| r.expiry_height > tip_height);
        let removed = before - map.len();
        if removed > 0 {
            let _ = self.flush(&map);
        }
        removed
    }
}

// ── Pool-identity + verify-and-store core (pure, unit-tested) ─────────────────

pub fn pool_identity_json(pool_pubkey_hex: &str, network_id: u8) -> serde_json::Value {
    serde_json::json!({
        "pool_pubkey": pool_pubkey_hex,
        "network_id": network_id,
        "fee_bps": 0,
        "deleg_version": Delegation::VERSION,
        "domain": String::from_utf8_lossy(DELEG_DOMAIN),
    })
}

#[derive(Debug, PartialEq)]
pub enum DelegError {
    Mainnet,
    BadHex,
    BadFormat,
    BadSignature,
    MinerPkhMismatch,
    WorkerTagMismatch,
    NetworkMismatch,
    PoolPubkeyMismatch,
    NonZeroFee,
    Expired,
    Storage(String),
}

impl DelegError {
    pub fn http_status(&self) -> (u16, &'static str) {
        match self {
            DelegError::Mainnet => (503, "Service Unavailable"),
            DelegError::Expired => (409, "Conflict"),
            DelegError::Storage(_) => (500, "Internal Server Error"),
            _ => (400, "Bad Request"),
        }
    }
    pub fn reason(&self) -> String {
        match self {
            DelegError::Mainnet => "delegation registration unavailable on mainnet".into(),
            DelegError::BadHex => "delegation: invalid hex".into(),
            DelegError::BadFormat => "delegation: malformed wire format".into(),
            DelegError::BadSignature => "delegation: signature verification failed".into(),
            DelegError::MinerPkhMismatch => {
                "delegation: miner_pkh does not match miner_pubkey".into()
            }
            DelegError::WorkerTagMismatch => "delegation: worker_tag does not match worker".into(),
            DelegError::NetworkMismatch => "delegation: network_id mismatch".into(),
            DelegError::PoolPubkeyMismatch => "delegation: pool_pubkey is not this pool".into(),
            DelegError::NonZeroFee => "delegation: fee_bps must be 0 (official pool is 0%)".into(),
            DelegError::Expired => "delegation: expiry_height must be in the future".into(),
            DelegError::Storage(e) => format!("delegation: storage error: {e}"),
        }
    }
}

/// Verify a submitted delegation and persist it. Pure (tip + now passed in) so
/// it is fully unit-testable without HTTP or RPC.
#[allow(clippy::too_many_arguments)]
pub fn verify_and_store(
    store: &dyn DelegationStore,
    delegation_hex: &str,
    worker: &str,
    expected_miner_pkh_hex: &str,
    pool_pubkey: &[u8; 33],
    network_id: u8,
    tip_height: u64,
    now_unix: u64,
) -> Result<StoredDelegation, DelegError> {
    if network_id == 0 {
        return Err(DelegError::Mainnet);
    }
    let bytes = hex::decode(delegation_hex).map_err(|_| DelegError::BadHex)?;
    let d = Delegation::deserialize(&bytes).map_err(|_| DelegError::BadFormat)?;

    if d.network_id != network_id {
        return Err(DelegError::NetworkMismatch);
    }
    if &d.pool_pubkey != pool_pubkey {
        return Err(DelegError::PoolPubkeyMismatch);
    }
    if d.fee_bps != 0 {
        return Err(DelegError::NonZeroFee);
    }
    if d.worker_tag != worker_tag(worker) {
        return Err(DelegError::WorkerTagMismatch);
    }
    d.verify_signature().map_err(|_| DelegError::BadSignature)?;

    let miner_pkh = d.miner_pkh();
    let miner_pkh_hex = hex::encode(miner_pkh);
    if !expected_miner_pkh_hex.is_empty()
        && expected_miner_pkh_hex.to_ascii_lowercase() != miner_pkh_hex
    {
        return Err(DelegError::MinerPkhMismatch);
    }
    if d.expiry_height <= tip_height {
        return Err(DelegError::Expired);
    }

    let rec = StoredDelegation {
        delegation_hex: delegation_hex.to_ascii_lowercase(),
        miner_pkh: miner_pkh_hex,
        worker: worker.to_string(),
        network_id: d.network_id,
        expiry_height: d.expiry_height,
        fee_bps: d.fee_bps,
        status: "active".to_string(),
        received_at_unix: now_unix,
    };
    store.put(rec.clone()).map_err(DelegError::Storage)?;
    Ok(rec)
}

// ── Mode-1 receipt production (pure; pool-side) ──────────────────────────────

/// Expected CPU lane first byte (assignment lane is "cpu").
pub const EXPECTED_LANE_FIRST: u8 = b'c';

/// Decoded, validated per-block assignment context the pool uses to produce a
/// mode-1 receipt for `block_height` (= tip + 1).
#[derive(Debug, Clone)]
pub struct AssignmentContext {
    pub block_height: u64,
    pub seed: [u8; 32],
    pub commitment_nonce: [u8; 32],
    pub difficulty: u32,
    pub lane: String,
}

fn decode32(s: &str) -> Option<[u8; 32]> {
    let b = hex::decode(s).ok()?;
    if b.len() != 32 {
        return None;
    }
    let mut a = [0u8; 32];
    a.copy_from_slice(&b);
    Some(a)
}

/// Convert the node assignment DTO into an `AssignmentContext` for `job_height`,
/// applying the fail-closed checks: the assignment must be for the tip the job
/// builds on (`assignment.height + 1 == job_height`) and the CPU lane.
pub fn assignment_context_from_dto(
    dto: &crate::template::PoawxAssignment,
    job_height: u64,
) -> Option<AssignmentContext> {
    if dto.height.checked_add(1) != Some(job_height) {
        return None; // assignment not for this job's parent tip
    }
    let lane_first = dto.lane.bytes().next()?;
    if lane_first != EXPECTED_LANE_FIRST {
        return None; // unexpected lane
    }
    Some(AssignmentContext {
        block_height: job_height,
        seed: decode32(&dto.seed)?,
        commitment_nonce: decode32(&dto.commitment_nonce)?,
        difficulty: dto.puzzle_difficulty,
        lane: dto.lane.clone(),
    })
}

fn count_leading_zero_bits(hash: &[u8; 32]) -> u32 {
    let mut bits = 0u32;
    for &b in hash.iter() {
        let z = b.leading_zeros();
        bits += z;
        if z < 8 {
            break;
        }
    }
    bits
}

/// Grind an 8-byte solution so `sha256d(seed‖nonce‖solution)` has at least
/// `difficulty` leading zero bits. Bounded; returns None if not found (fail-closed).
fn grind_solution(seed: &[u8; 32], nonce: &[u8; 32], difficulty: u32) -> Option<[u8; 8]> {
    let mut input = [0u8; 72];
    input[..32].copy_from_slice(seed);
    input[32..64].copy_from_slice(nonce);
    for n in 0u64..50_000_000 {
        let sol = n.to_le_bytes();
        input[64..].copy_from_slice(&sol);
        let hash = crate::pow::sha256d(&input);
        if count_leading_zero_bits(&hash) >= difficulty {
            return Some(sol);
        }
    }
    None
}

/// Phase 18C: gated producer trace. Logs reason codes + short prefixes only;
/// never secrets/tokens/full delegation hex/full signatures.
pub fn producer_trace_enabled() -> bool {
    env::var("IRIUM_POAWX_PRODUCER_TRACE")
        .map(|v| v.trim() == "1")
        .unwrap_or(false)
}

fn pfx(bytes: &[u8]) -> String {
    hex::encode(&bytes[..bytes.len().min(4)])
}

/// Produce a mode-1 (delegated) pending receipt for `miner_pkh`/`worker`, or
/// `None` if no valid delegation applies (fail-closed: missing/expired/wrong
/// network/non-zero fee/wrong pool key/wrong worker tag/grind failure). Pure:
/// the assignment is supplied by the caller (fetched from `/poawx/assignment`).
pub fn build_mode1_pending_receipt(
    store: &dyn DelegationStore,
    key: &DelegateKey,
    network_id: u8,
    miner_pkh: [u8; 20],
    worker: &str,
    ctx: &AssignmentContext,
) -> Option<crate::template::PoawxPendingReceipt> {
    let trace = producer_trace_enabled();
    let miner_pkh_hex = hex::encode(miner_pkh);
    macro_rules! deny {
        ($reason:expr) => {{
            if trace {
                info!(
                    "[poawx-trace] build_mode1 deny reason={} miner_pkh={} worker={} key={} net={} block_h={}",
                    $reason, pfx(&miner_pkh), worker, deleg_key(&miner_pkh_hex, worker), network_id, ctx.block_height
                );
            }
            return None;
        }};
    }
    let rec = match store.get(&miner_pkh_hex, worker) {
        Some(r) => r,
        None => deny!("delegation_missing"),
    };
    if trace {
        info!(
            "[poawx-trace] build_mode1 found rec net={} fee_bps={} expiry={}",
            rec.network_id, rec.fee_bps, rec.expiry_height
        );
    }
    if rec.network_id != network_id {
        deny!("rec_network_mismatch");
    }
    if rec.fee_bps != 0 {
        deny!("rec_nonzero_fee");
    }
    if rec.expiry_height < ctx.block_height {
        deny!("expired");
    }
    let dbytes = match hex::decode(&rec.delegation_hex) {
        Ok(b) => b,
        Err(_) => deny!("delegation_hex_bad"),
    };
    let d = match Delegation::deserialize(&dbytes) {
        Ok(d) => d,
        Err(_) => deny!("delegation_decode_bad"),
    };
    if d.pool_pubkey != key.pubkey() {
        if trace {
            info!(
                "[poawx-trace] pool_pubkey mismatch deleg={} key={}",
                pfx(&d.pool_pubkey),
                pfx(&key.pubkey())
            );
        }
        deny!("pool_pubkey_mismatch");
    }
    if d.miner_pkh() != miner_pkh {
        deny!("miner_pkh_mismatch");
    }
    if d.network_id != network_id {
        deny!("deleg_network_mismatch");
    }
    if d.fee_bps != 0 {
        deny!("deleg_nonzero_fee");
    }
    if d.worker_tag != worker_tag(worker) {
        deny!("worker_tag_mismatch");
    }
    let solution = match grind_solution(&ctx.seed, &ctx.commitment_nonce, ctx.difficulty) {
        Some(s) => s,
        None => deny!("grind_failed"),
    };
    let signer_sig = key.sign_challenge(&solution, &ctx.commitment_nonce, ctx.block_height);
    let receipt = crate::template::PoawxPendingReceipt {
        height: ctx.block_height,
        lane: ctx.lane.clone(),
        worker_pkh: miner_pkh_hex,
        solution: hex::encode(solution),
        commitment_nonce: hex::encode(ctx.commitment_nonce),
        worker_pubkey: key.pubkey_hex(),
        worker_sig: hex::encode(signer_sig),
        delegation: rec.delegation_hex.clone(),
        // Phase 20: base mode-1 receipt carries no extension here; the pool's
        // production path (stratum) attaches a synthetic Phase20ReceiptExt after
        // activation. Empty => legacy/pre-activation (byte-identical submit JSON).
        phase20_ext: String::new(),
    };
    if trace {
        info!(
            "[poawx-trace] build_mode1 OK block_h={} lane={} sol_grinded=true delegation_present=true",
            ctx.block_height, ctx.lane
        );
    }
    Some(receipt)
}

/// Fetch the node's `/poawx/assignment` (single source of truth for the
/// deterministic puzzle context). Returns None on any error (fail-closed).
pub async fn fetch_assignment(
    rpc_base: &str,
    rpc_token: &str,
) -> Option<crate::template::PoawxAssignment> {
    let client = reqwest::Client::builder().build().ok()?;
    let url = format!("{}/poawx/assignment", rpc_base.trim_end_matches('/'));
    let resp = client.get(&url).bearer_auth(rpc_token).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<crate::template::PoawxAssignment>().await.ok()
}

// ── Phase 20: production primitives (testnet/devnet mirror of node consensus) ─
//
// STRATUM-LOCAL mirror of the Phase 20 consensus primitives in
// `irium_node_rs::poawx` (multi-role reward split + fairness role claims +
// production receipt extension). Mirrored byte-for-byte; the parity tests below
// assert equality against the dev-dep node lib so any drift fails. Production
// here is OFFICIAL fee-0 only (fee_bps=0 / fee_pkh=0). Mainnet is hard-off.
// The role-claim *source* used by pool production is the gated SYNTHETIC builder
// (testnet/devnet-only, for production-wiring validation) — NOT a live
// hidden-precommit protocol (which remains pending; see design-gap docs).

pub const MULTI_ROLE_PRIMARY_BPS: u64 = 5500;
pub const MULTI_ROLE_COMPUTE_BPS: u64 = 2200;
pub const MULTI_ROLE_VERIFY_BPS: u64 = 1300;
pub const MULTI_ROLE_SUPPORT_BPS: u64 = 1000;

pub const ROLE_COMPUTE_CONTRIBUTOR: u8 = 1;
pub const ROLE_VERIFY_CONTRIBUTOR: u8 = 2;
pub const ROLE_SUPPORT_CONTRIBUTOR: u8 = 3;

pub const LANE_CPU_FRIENDLY: u8 = 0;
pub const LANE_GPU_PARALLEL: u8 = 1;
pub const LANE_ASIC_STREAMING: u8 = 2;

pub const FAIRNESS_DOMAIN_V1: &[u8] = b"IRIUM_POAWX_FAIRNESS_V1";
pub const ROLE_CLAIM_DOMAIN_V1: &[u8] = b"IRIUM_POAWX_ROLE_CLAIM_V1";
pub const FAIRNESS_CPU_UPPER: u32 = 3400;
pub const FAIRNESS_GPU_UPPER: u32 = 6700;
pub const PHASE20_EXT_VERSION: u8 = 1;

/// Mirror of `irium_node_rs::poawx::multi_role_amounts`: split `total` into
/// `[primary, compute, verify, support]` (floor; remainder → PRIMARY; exact sum).
pub fn multi_role_amounts(total: u64) -> [u64; 4] {
    let bps = |b: u64| -> u64 { ((total as u128 * b as u128) / 10_000u128) as u64 };
    let compute = bps(MULTI_ROLE_COMPUTE_BPS);
    let verify = bps(MULTI_ROLE_VERIFY_BPS);
    let support = bps(MULTI_ROLE_SUPPORT_BPS);
    let primary_floor = bps(MULTI_ROLE_PRIMARY_BPS);
    let remainder = total - primary_floor - compute - verify - support;
    [primary_floor + remainder, compute, verify, support]
}

/// Mirror of `irium_node_rs::poawx::fairness_assignment_digest`.
pub fn fairness_assignment_digest(
    network_id: u8,
    height: u64,
    prev_hash: &[u8; 32],
    role_id: u8,
    slot_index: u32,
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(FAIRNESS_DOMAIN_V1);
    h.update([network_id]);
    h.update(height.to_le_bytes());
    h.update(prev_hash);
    h.update([role_id]);
    h.update(slot_index.to_le_bytes());
    h.finalize().into()
}

/// Mirror of `irium_node_rs::poawx::assign_lane` returning the lane *id* byte
/// (production lanes only; never the dev/test fallback).
pub fn assign_lane_id(
    network_id: u8,
    height: u64,
    prev_hash: &[u8; 32],
    role_id: u8,
    slot_index: u32,
) -> u8 {
    let d = fairness_assignment_digest(network_id, height, prev_hash, role_id, slot_index);
    let v = (u64::from_le_bytes(d[0..8].try_into().expect("len 8")) % 10_000) as u32;
    if v < FAIRNESS_CPU_UPPER {
        LANE_CPU_FRIENDLY
    } else if v < FAIRNESS_GPU_UPPER {
        LANE_GPU_PARALLEL
    } else {
        LANE_ASIC_STREAMING
    }
}

/// Mirror of `irium_node_rs::poawx::role_claim_digest`.
#[allow(clippy::too_many_arguments)]
pub fn role_claim_digest(
    network_id: u8,
    height: u64,
    prev_hash: &[u8; 32],
    role_id: u8,
    lane_id: u8,
    solver_pkh: &[u8; 20],
    nonce: &[u8; 32],
    secret: &[u8; 32],
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(ROLE_CLAIM_DOMAIN_V1);
    h.update([network_id]);
    h.update(height.to_le_bytes());
    h.update(prev_hash);
    h.update([role_id]);
    h.update([lane_id]);
    h.update(solver_pkh);
    h.update(nonce);
    h.update(secret);
    h.finalize().into()
}

/// Mirror of `irium_node_rs::poawx::RoleReward` (60-byte wire).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRewardMirror {
    pub compute_contributor_pkh: [u8; 20],
    pub verify_contributor_pkh: [u8; 20],
    pub support_contributor_pkh: [u8; 20],
}

impl RoleRewardMirror {
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(60);
        out.extend_from_slice(&self.compute_contributor_pkh);
        out.extend_from_slice(&self.verify_contributor_pkh);
        out.extend_from_slice(&self.support_contributor_pkh);
        out
    }
}

/// Mirror of `irium_node_rs::poawx::PoawxRoleClaim`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoawxRoleClaimMirror {
    pub role_id: u8,
    pub lane_id: u8,
    pub solver_pkh: [u8; 20],
    pub nonce: [u8; 32],
    pub secret: [u8; 32],
    pub claim_digest: [u8; 32],
    pub commitment_hash: Option<[u8; 32]>,
}

impl PoawxRoleClaimMirror {
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(118 + 1 + 32);
        out.push(self.role_id);
        out.push(self.lane_id);
        out.extend_from_slice(&self.solver_pkh);
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.secret);
        out.extend_from_slice(&self.claim_digest);
        match &self.commitment_hash {
            Some(c) => {
                out.push(1);
                out.extend_from_slice(c);
            }
            None => out.push(0),
        }
        out
    }
}

/// Mirror of `irium_node_rs::poawx::Phase20ReceiptExt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase20ReceiptExtMirror {
    pub role_reward: RoleRewardMirror,
    pub compute_claim: PoawxRoleClaimMirror,
    pub verify_claim: PoawxRoleClaimMirror,
    pub support_claim: PoawxRoleClaimMirror,
    pub fee_bps: u16,
    pub fee_pkh: [u8; 20],
}

impl Phase20ReceiptExtMirror {
    /// Wire: version(1) || role_reward(60) || (len_u16 || claim)×3 || fee_bps(2) || fee_pkh(20).
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(PHASE20_EXT_VERSION);
        out.extend_from_slice(&self.role_reward.serialize());
        for claim in [&self.compute_claim, &self.verify_claim, &self.support_claim] {
            let b = claim.serialize();
            out.extend_from_slice(&(b.len() as u16).to_le_bytes());
            out.extend_from_slice(&b);
        }
        out.extend_from_slice(&self.fee_bps.to_le_bytes());
        out.extend_from_slice(&self.fee_pkh);
        out
    }

    pub fn digest(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.serialize());
        h.finalize().into()
    }
}

/// Parse an env activation height (`>= h`), mainnet-off handled by the caller.
fn activation_height_reached(var: &str, height: u64) -> bool {
    match env::var(var)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
    {
        Some(h) => height >= h,
        None => false,
    }
}

/// Mirror of `irium_node_rs::chain::phase20_production_active`: both multi-role
/// reward AND fairness matrix active for `height`. **Mainnet hard-off**
/// (`network_id_from_env() == 0`). Used to gate pool production + the gated root.
pub fn phase20_production_active(height: u64) -> bool {
    if network_id_from_env() == 0 {
        return false; // mainnet
    }
    activation_height_reached("IRIUM_POAWX_MULTI_ROLE_REWARD_ACTIVATION_HEIGHT", height)
        && activation_height_reached("IRIUM_POAWX_FAIRNESS_MATRIX_ACTIVATION_HEIGHT", height)
}

/// Whether the gated SYNTHETIC role-claim builder is enabled. Testnet/devnet-only
/// (`IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS=1`), mainnet hard-off, disabled by default.
/// This is for production-wiring validation; it is NOT the live hidden-precommit
/// role-claim protocol (pending).
pub fn synthetic_role_claims_enabled() -> bool {
    if network_id_from_env() == 0 {
        return false; // mainnet
    }
    env::var("IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS")
        .map(|v| v.trim() == "1")
        .unwrap_or(false)
}

/// Deterministic per-(role) 32-byte field for reproducible synthetic claims.
fn synth_field(tag: &[u8], network_id: u8, height: u64, prev_hash: &[u8; 32], role_id: u8) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"IRIUM_POAWX_SYNTHETIC_V1");
    h.update(tag);
    h.update([network_id]);
    h.update(height.to_le_bytes());
    h.update(prev_hash);
    h.update([role_id]);
    h.finalize().into()
}

/// Build a synthetic OFFICIAL (fee-0) `Phase20ReceiptExtMirror` for pool
/// production on testnet/devnet. For each role: deterministic nonce/secret, the
/// assigned lane via `assign_lane_id`, a verifying `role_claim_digest`, and a
/// solver pkh chosen deterministically from `workers` (if any) else `primary_pkh`
/// (the MVP single-miner case). `RoleReward` mirrors the validated solver pkhs.
/// Returns None on mainnet or when synthetic claims are disabled (never fakes).
pub fn build_synthetic_phase20_ext(
    network_id: u8,
    height: u64,
    prev_hash: &[u8; 32],
    primary_pkh: &[u8; 20],
    workers: &[[u8; 20]],
) -> Option<Phase20ReceiptExtMirror> {
    if network_id == 0 || !synthetic_role_claims_enabled() {
        return None;
    }
    let mk = |role_id: u8, idx: usize| -> PoawxRoleClaimMirror {
        let lane_id = assign_lane_id(network_id, height, prev_hash, role_id, 0);
        let nonce = synth_field(b"nonce", network_id, height, prev_hash, role_id);
        let secret = synth_field(b"secret", network_id, height, prev_hash, role_id);
        let solver = if workers.is_empty() {
            *primary_pkh
        } else {
            workers[idx % workers.len()]
        };
        let claim_digest = role_claim_digest(
            network_id, height, prev_hash, role_id, lane_id, &solver, &nonce, &secret,
        );
        PoawxRoleClaimMirror {
            role_id,
            lane_id,
            solver_pkh: solver,
            nonce,
            secret,
            claim_digest,
            commitment_hash: None,
        }
    };
    let compute_claim = mk(ROLE_COMPUTE_CONTRIBUTOR, 0);
    let verify_claim = mk(ROLE_VERIFY_CONTRIBUTOR, 1);
    let support_claim = mk(ROLE_SUPPORT_CONTRIBUTOR, 2);
    Some(Phase20ReceiptExtMirror {
        role_reward: RoleRewardMirror {
            compute_contributor_pkh: compute_claim.solver_pkh,
            verify_contributor_pkh: verify_claim.solver_pkh,
            support_contributor_pkh: support_claim.solver_pkh,
        },
        compute_claim,
        verify_claim,
        support_claim,
        fee_bps: 0,
        fee_pkh: [0u8; 20],
    })
}

/// Extract the three RoleReward pkhs from a hex-encoded `Phase20ReceiptExt`
/// (`version(1) || role_reward(60) || …`). Used by the coinbase builder without
/// a full deserialize. Returns None on malformed/short input (fail-closed).
pub fn role_reward_pkhs_from_ext_hex(ext_hex: &str) -> Option<([u8; 20], [u8; 20], [u8; 20])> {
    let b = hex::decode(ext_hex).ok()?;
    if b.len() < 1 + 60 || b[0] != PHASE20_EXT_VERSION {
        return None;
    }
    let mut c = [0u8; 20];
    let mut v = [0u8; 20];
    let mut s = [0u8; 20];
    c.copy_from_slice(&b[1..21]);
    v.copy_from_slice(&b[21..41]);
    s.copy_from_slice(&b[41..61]);
    Some((c, v, s))
}

// ── HTTP server (raw-TCP, loopback-only by config; opt-in) ───────────────────

fn poawx_state_dir() -> PathBuf {
    env::var("IRIUM_POAWX_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/opt/irium-pool/data"))
}

pub fn delegate_key_path() -> PathBuf {
    env::var("IRIUM_POAWX_DELEGATE_KEY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| poawx_state_dir().join("poawx_delegate_key.hex"))
}

pub fn delegations_path() -> PathBuf {
    env::var("IRIUM_POAWX_DELEGATIONS_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| poawx_state_dir().join("poawx_delegations.json"))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_loopback_bind(bind: &str) -> bool {
    let t = bind.trim();
    t.starts_with("127.0.0.1:") || t.starts_with("localhost:") || t.starts_with("[::1]:")
}

/// Shared PoAW-X producer context: the delegate key + delegation registry +
/// network id, shared via a single `Arc` between the HTTP registration server
/// and the live receipt-producer path so a freshly registered delegation is
/// immediately visible to the mining path. None on mainnet (mode-1 hard-off).
pub struct PoawxProducer {
    pub store: Arc<dyn DelegationStore>,
    pub key: Arc<DelegateKey>,
    pub network_id: u8,
}

/// Load the producer (delegate key + delegation store). Returns None on mainnet
/// (no key generated, delegation refused) or if the key/store cannot be loaded.
pub fn load_producer() -> Option<PoawxProducer> {
    let network_id = network_id_from_env();
    if network_id == 0 {
        warn!("[poawx-deleg] mainnet context: PoAW-X delegation disabled (mode-1 hard-off)");
        return None;
    }
    let key = match DelegateKey::load_or_generate(&delegate_key_path(), true) {
        Ok(k) => k,
        Err(e) => {
            warn!("[poawx-deleg] delegate key unavailable: {e}");
            return None;
        }
    };
    let store = match JsonDelegationStore::open(delegations_path()) {
        Ok(s) => Arc::new(s) as Arc<dyn DelegationStore>,
        Err(e) => {
            warn!("[poawx-deleg] delegation store unavailable: {e}");
            return None;
        }
    };
    info!(
        "[poawx-deleg] pool_pubkey={} network_id={network_id}",
        key.pubkey_hex()
    );
    Some(PoawxProducer {
        store,
        key: Arc::new(key),
        network_id,
    })
}

/// Spawn the delegation HTTP server if `IRIUM_POAWX_DELEGATION_BIND` is set.
/// Default = DISABLED (no bind). Refuses any non-loopback bind. The server shares
/// `producer` (store+key) with the receipt-producer path. On mainnet (`producer`
/// None) the endpoints return 503.
pub fn maybe_spawn(producer: Option<Arc<PoawxProducer>>, rpc_base: String, rpc_token: String) {
    let bind = match env::var("IRIUM_POAWX_DELEGATION_BIND")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        Some(b) => b,
        None => return, // disabled by default
    };
    if !is_loopback_bind(&bind) {
        warn!(
            "[poawx-deleg] refusing non-loopback IRIUM_POAWX_DELEGATION_BIND={bind}; server NOT started"
        );
        return;
    }
    let network_id = network_id_from_env();
    tokio::spawn(async move {
        if let Err(e) = serve(bind, producer, network_id, rpc_base, rpc_token).await {
            warn!("[poawx-deleg] server stopped: {e}");
        }
    });
}

struct ServerCtx {
    network_id: u8,
    /// None on mainnet (delegation disabled → 503).
    producer: Option<Arc<PoawxProducer>>,
    rpc_base: String,
    rpc_token: String,
}

async fn serve(
    bind: String,
    producer: Option<Arc<PoawxProducer>>,
    network_id: u8,
    rpc_base: String,
    rpc_token: String,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&bind).await?;
    info!("[poawx-deleg] listening on http://{bind} (loopback-only, network_id={network_id})");

    let ctx = Arc::new(ServerCtx {
        network_id,
        producer,
        rpc_base,
        rpc_token,
    });

    loop {
        let (stream, _addr) = listener.accept().await?;
        let ctx = Arc::clone(&ctx);
        tokio::spawn(async move {
            let _ = handle_conn(stream, ctx).await;
        });
    }
}

async fn read_request(stream: &mut TcpStream) -> Option<(String, String, Vec<u8>)> {
    let mut buf: Vec<u8> = Vec::with_capacity(2048);
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
        if buf.len() > 16 * 1024 {
            return None;
        }
    };
    let header_str = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut header_lines = header_str.lines();
    let first = header_lines.next()?;
    let mut parts = first.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();

    let mut content_len = 0usize;
    for l in header_str.lines() {
        let ll = l.to_ascii_lowercase();
        if let Some(v) = ll.strip_prefix("content-length:") {
            content_len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = buf[header_end..].to_vec();
    while body.len() < content_len {
        let n = stream.read(&mut tmp).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
        if body.len() > 64 * 1024 {
            break;
        }
    }
    body.truncate(content_len.min(body.len()));
    Some((method, path, body))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

async fn respond(stream: &mut TcpStream, status: u16, reason: &str, body: &serde_json::Value) {
    let body_str = serde_json::to_string(body).unwrap_or_else(|_| "{}".to_string());
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body_str.len(),
        body_str
    );
    let _ = stream.write_all(resp.as_bytes()).await;
    let _ = stream.flush().await;
}

async fn fetch_tip_height(rpc_base: &str, rpc_token: &str) -> Option<u64> {
    let tpl =
        crate::template::TemplateClient::new(rpc_base.to_string(), rpc_token.to_string()).ok()?;
    let template = tpl.fetch_template().await.ok()?;
    Some(template.height.saturating_sub(1))
}

async fn handle_conn(mut stream: TcpStream, ctx: Arc<ServerCtx>) {
    let (method, path, body) = match read_request(&mut stream).await {
        Some(t) => t,
        None => return,
    };
    let path_only = path.split('?').next().unwrap_or(&path);

    match (method.as_str(), path_only) {
        ("GET", "/poawx/pool-identity") => match &ctx.producer {
            None => {
                respond(
                    &mut stream,
                    503,
                    "Service Unavailable",
                    &serde_json::json!({"error":"delegation unavailable on mainnet"}),
                )
                .await
            }
            Some(p) => {
                let v = pool_identity_json(&p.key.pubkey_hex(), p.network_id);
                respond(&mut stream, 200, "OK", &v).await
            }
        },
        ("POST", "/poawx/delegation") => {
            let (key, store) = match &ctx.producer {
                Some(p) => (&p.key, &p.store),
                None => {
                    respond(
                        &mut stream,
                        503,
                        "Service Unavailable",
                        &serde_json::json!({"error":"delegation unavailable on mainnet"}),
                    )
                    .await;
                    return;
                }
            };
            let parsed: serde_json::Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(_) => {
                    respond(
                        &mut stream,
                        400,
                        "Bad Request",
                        &serde_json::json!({"error":"invalid JSON body"}),
                    )
                    .await;
                    return;
                }
            };
            let delegation_hex = parsed
                .get("delegation")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let worker = parsed.get("worker").and_then(|v| v.as_str()).unwrap_or("");
            let miner_pkh = parsed
                .get("miner_pkh")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let tip = match fetch_tip_height(&ctx.rpc_base, &ctx.rpc_token).await {
                Some(t) => t,
                None => {
                    respond(
                        &mut stream,
                        503,
                        "Service Unavailable",
                        &serde_json::json!({"error":"could not determine chain tip"}),
                    )
                    .await;
                    return;
                }
            };
            match verify_and_store(
                store.as_ref(),
                delegation_hex,
                worker,
                miner_pkh,
                &key.pubkey(),
                ctx.network_id,
                tip,
                unix_now(),
            ) {
                Ok(rec) => {
                    respond(
                        &mut stream,
                        200,
                        "OK",
                        &serde_json::json!({
                            "status": rec.status,
                            "miner_pkh": rec.miner_pkh,
                            "worker": rec.worker,
                            "expiry_height": rec.expiry_height,
                            "network_id": rec.network_id,
                        }),
                    )
                    .await
                }
                Err(e) => {
                    let (status, reason) = e.http_status();
                    respond(
                        &mut stream,
                        status,
                        reason,
                        &serde_json::json!({"error": e.reason()}),
                    )
                    .await
                }
            }
        }
        _ => {
            respond(
                &mut stream,
                404,
                "Not Found",
                &serde_json::json!({"error":"not found"}),
            )
            .await
        }
    }
}

/// Shared process-wide lock for tests that mutate `IRIUM_NETWORK` / the Phase 20
/// activation env vars (env is global). Exposed `pub(crate)` so the stratum test
/// module can serialize against the same lock.
#[cfg(test)]
pub(crate) fn p20_env_lock() -> &'static std::sync::Mutex<()> {
    static L: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    L.get_or_init(|| std::sync::Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::ecdsa::signature::hazmat::PrehashSigner;
    use k256::ecdsa::{Signature, SigningKey, VerifyingKey};

    fn pk33(sk: &SigningKey) -> [u8; 33] {
        let vk = VerifyingKey::from(sk);
        let enc = vk.to_encoded_point(true);
        let mut p = [0u8; 33];
        p.copy_from_slice(enc.as_bytes());
        p
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("irium-deleg-{tag}-{stamp}"))
    }

    /// Build a mirror Delegation signed by `miner` (matches what the wallet
    /// produces via the canonical type — the parity tests below prove this).
    fn mirror_signed(
        miner: &SigningKey,
        pool_pubkey: [u8; 33],
        network_id: u8,
        worker: &str,
        expiry: u64,
        fee_bps: u16,
    ) -> Delegation {
        let mut d = Delegation {
            deleg_version: Delegation::VERSION,
            network_id,
            miner_pubkey: pk33(miner),
            pool_pubkey,
            worker_tag: worker_tag(worker),
            expiry_height: expiry,
            fee_bps,
            fee_pkh: [0u8; 20],
            deleg_nonce: [7u8; 32],
            delegation_sig: [0u8; 64],
        };
        let sig: Signature = miner.sign_prehash(&d.message_hash()).unwrap();
        d.delegation_sig.copy_from_slice(&sig.to_bytes());
        d
    }

    // ── Mandatory parity tests vs canonical irium_node_rs::poawx::Delegation ──

    #[test]
    fn parity_serialize_message_hash_digest_and_size() {
        let fields = (
            1u8,
            1u8,
            [0x11u8; 33],
            [0x22u8; 33],
            [0x33u8; 32],
            999u64,
            0u16,
            [0u8; 20],
            [0x44u8; 32],
            [0x55u8; 64],
        );
        let canon = irium_node_rs::poawx::Delegation {
            deleg_version: fields.0,
            network_id: fields.1,
            miner_pubkey: fields.2,
            pool_pubkey: fields.3,
            worker_tag: fields.4,
            expiry_height: fields.5,
            fee_bps: fields.6,
            fee_pkh: fields.7,
            deleg_nonce: fields.8,
            delegation_sig: fields.9,
        };
        let mir = Delegation {
            deleg_version: fields.0,
            network_id: fields.1,
            miner_pubkey: fields.2,
            pool_pubkey: fields.3,
            worker_tag: fields.4,
            expiry_height: fields.5,
            fee_bps: fields.6,
            fee_pkh: fields.7,
            deleg_nonce: fields.8,
            delegation_sig: fields.9,
        };
        assert_eq!(Delegation::WIRE_SIZE, 226);
        assert_eq!(irium_node_rs::poawx::Delegation::WIRE_SIZE, 226);
        assert_eq!(canon.serialize(), mir.serialize(), "serialize parity");
        assert_eq!(canon.serialize().len(), 226);
        assert_eq!(
            canon.message_hash(),
            mir.message_hash(),
            "message_hash parity"
        );
        assert_eq!(canon.digest(), mir.digest(), "digest parity");
        assert_eq!(
            Delegation::VERSION,
            irium_node_rs::poawx::Delegation::VERSION
        );
        assert_eq!(DELEG_DOMAIN, irium_node_rs::poawx::DOMAIN_DELEG);
    }

    #[test]
    fn parity_mirror_verifies_canonical_signed_and_rejects_tamper() {
        let miner = SigningKey::from_slice(&[7u8; 32]).unwrap();
        let mp = pk33(&miner);
        let mut canon = irium_node_rs::poawx::Delegation {
            deleg_version: 1,
            network_id: 1,
            miner_pubkey: mp,
            pool_pubkey: [0x02u8; 33],
            worker_tag: [0u8; 32],
            expiry_height: 999,
            fee_bps: 0,
            fee_pkh: [0u8; 20],
            deleg_nonce: [4u8; 32],
            delegation_sig: [0u8; 64],
        };
        let sig: Signature = miner.sign_prehash(&canon.message_hash()).unwrap();
        canon.delegation_sig.copy_from_slice(&sig.to_bytes());

        // Mirror deserializes the canonical bytes and verifies the signature.
        let mir = Delegation::deserialize(&canon.serialize()).unwrap();
        assert!(
            mir.verify_signature().is_ok(),
            "mirror verifies canonical-signed"
        );
        assert_eq!(mir.miner_pkh(), canon.miner_pkh(), "miner_pkh parity");

        // Tampered canonical delegation must be rejected by the mirror.
        let mut tampered = canon.clone();
        tampered.delegation_sig[0] ^= 0xff;
        let mir_bad = Delegation::deserialize(&tampered.serialize()).unwrap();
        assert!(
            mir_bad.verify_signature().is_err(),
            "mirror rejects tampered"
        );
    }

    #[test]
    fn parity_miner_pkh_matches_hash160() {
        let miner = SigningKey::from_slice(&[9u8; 32]).unwrap();
        let mp = pk33(&miner);
        let canon = irium_node_rs::poawx::Delegation {
            deleg_version: 1,
            network_id: 1,
            miner_pubkey: mp,
            pool_pubkey: [0u8; 33],
            worker_tag: [0u8; 32],
            expiry_height: 1,
            fee_bps: 0,
            fee_pkh: [0u8; 20],
            deleg_nonce: [0u8; 32],
            delegation_sig: [0u8; 64],
        };
        let expected = {
            let sha = Sha256::digest(mp);
            let rip = ripemd::Ripemd160::digest(sha);
            let mut p = [0u8; 20];
            p.copy_from_slice(&rip);
            p
        };
        let mir = Delegation::deserialize(&canon.serialize()).unwrap();
        assert_eq!(mir.miner_pkh(), expected);
        assert_eq!(canon.miner_pkh(), expected);
    }

    // ── Store, key, verify, identity ──

    #[test]
    fn store_put_get_persist_across_reload() {
        let dir = temp_dir("store");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("poawx_delegations.json");
        let rec = StoredDelegation {
            delegation_hex: "ab".repeat(226),
            miner_pkh: "aa".repeat(20),
            worker: "rig1".into(),
            network_id: 1,
            expiry_height: 100,
            fee_bps: 0,
            status: "active".into(),
            received_at_unix: 1,
        };
        {
            let s = JsonDelegationStore::open(&path).unwrap();
            s.put(rec.clone()).unwrap();
            assert_eq!(s.get(&rec.miner_pkh, "rig1").unwrap(), rec);
        }
        let s2 = JsonDelegationStore::open(&path).unwrap();
        assert_eq!(
            s2.get(&rec.miner_pkh, "rig1").unwrap(),
            rec,
            "persists across reload"
        );
        // Registry stores no private keys.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("privkey") && !raw.contains("secret") && !raw.contains("private"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn multi_worker_registry_isolation_and_reload() {
        // Phase 20 (Part D, registry layer): the delegation registry holds many
        // (miner_pkh, worker) entries without cross-contamination; reload preserves
        // all; all_active() honors expiry; no worker can resolve to another's record.
        let dir = temp_dir("multiworker");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("poawx_delegations.json");
        let mk = |pkh: &str, worker: &str, expiry: u64| StoredDelegation {
            delegation_hex: "cd".repeat(226),
            miner_pkh: pkh.to_string(),
            worker: worker.to_string(),
            network_id: 1,
            expiry_height: expiry,
            fee_bps: 0,
            status: "active".into(),
            received_at_unix: 1,
        };
        let a = mk(&"aa".repeat(20), "rig1", 100); // miner A / worker rig1
        let b = mk(&"bb".repeat(20), "rig2", 100); // miner B / worker rig2 (different miner+worker)
        let a2 = mk(&"aa".repeat(20), "rig3", 5); // miner A / worker rig3, expires early
        {
            let s = JsonDelegationStore::open(&path).unwrap();
            s.put(a.clone()).unwrap();
            s.put(b.clone()).unwrap();
            s.put(a2.clone()).unwrap();
            // exact (pkh,worker) resolution — no cross-pay collisions:
            assert_eq!(s.get(&a.miner_pkh, "rig1").unwrap(), a);
            assert_eq!(s.get(&b.miner_pkh, "rig2").unwrap(), b);
            assert_eq!(s.get(&a.miner_pkh, "rig3").unwrap(), a2);
            assert!(
                s.get(&b.miner_pkh, "rig1").is_none(),
                "worker rig1 belongs to miner A only"
            );
            assert!(
                s.get(&a.miner_pkh, "rig2").is_none(),
                "worker rig2 belongs to miner B only"
            );
            assert!(
                s.get(&"cc".repeat(20), "rig1").is_none(),
                "unknown miner has no record"
            );
            // all_active(tip=10): a and b active; a2 expired (expiry 5 <= 10).
            let active = s.all_active(10);
            assert_eq!(active.len(), 2, "two active delegations at tip 10");
        }
        // reload preserves every (pkh,worker) entry, including the expired-but-stored a2.
        let s2 = JsonDelegationStore::open(&path).unwrap();
        assert_eq!(s2.get(&a.miner_pkh, "rig1").unwrap(), a);
        assert_eq!(s2.get(&b.miner_pkh, "rig2").unwrap(), b);
        assert_eq!(s2.get(&a.miner_pkh, "rig3").unwrap(), a2);
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("privkey") && !raw.contains("secret") && !raw.contains("private"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_and_store_accepts_valid_and_no_privkey_in_registry() {
        let dir = temp_dir("ok");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("d.json");
        let store = JsonDelegationStore::open(&path).unwrap();
        let pool = SigningKey::from_slice(&[3u8; 32]).unwrap();
        let pool_pub = pk33(&pool);
        let miner = SigningKey::from_slice(&[5u8; 32]).unwrap();
        let d = mirror_signed(&miner, pool_pub, 1, "rig1", 100, 0);
        let miner_pkh_hex = hex::encode(d.miner_pkh());
        let rec = verify_and_store(
            &store,
            &hex::encode(d.serialize()),
            "rig1",
            &miner_pkh_hex,
            &pool_pub,
            1,
            10,
            42,
        )
        .unwrap();
        assert_eq!(rec.miner_pkh, miner_pkh_hex);
        assert!(store.get(&miner_pkh_hex, "rig1").is_some());
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("privkey") && !raw.contains("secret"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_and_store_rejections() {
        let dir = temp_dir("rej");
        std::fs::create_dir_all(&dir).unwrap();
        let store = JsonDelegationStore::open(dir.join("d.json")).unwrap();
        let pool = SigningKey::from_slice(&[3u8; 32]).unwrap();
        let pool_pub = pk33(&pool);
        let other_pool = pk33(&SigningKey::from_slice(&[4u8; 32]).unwrap());
        let miner = SigningKey::from_slice(&[5u8; 32]).unwrap();
        let hexd = |d: &Delegation| hex::encode(d.serialize());

        // mainnet (network_id 0)
        assert_eq!(
            verify_and_store(
                &store,
                &hexd(&mirror_signed(&miner, pool_pub, 0, "r", 100, 0)),
                "r",
                "",
                &pool_pub,
                0,
                10,
                1
            ),
            Err(DelegError::Mainnet)
        );
        // network mismatch
        assert_eq!(
            verify_and_store(
                &store,
                &hexd(&mirror_signed(&miner, pool_pub, 2, "r", 100, 0)),
                "r",
                "",
                &pool_pub,
                1,
                10,
                1
            ),
            Err(DelegError::NetworkMismatch)
        );
        // pool pubkey mismatch
        assert_eq!(
            verify_and_store(
                &store,
                &hexd(&mirror_signed(&miner, other_pool, 1, "r", 100, 0)),
                "r",
                "",
                &pool_pub,
                1,
                10,
                1
            ),
            Err(DelegError::PoolPubkeyMismatch)
        );
        // fee > 0
        assert_eq!(
            verify_and_store(
                &store,
                &hexd(&mirror_signed(&miner, pool_pub, 1, "r", 100, 100)),
                "r",
                "",
                &pool_pub,
                1,
                10,
                1
            ),
            Err(DelegError::NonZeroFee)
        );
        // worker_tag mismatch (claimed worker differs from signed tag)
        assert_eq!(
            verify_and_store(
                &store,
                &hexd(&mirror_signed(&miner, pool_pub, 1, "rig1", 100, 0)),
                "rig2",
                "",
                &pool_pub,
                1,
                10,
                1
            ),
            Err(DelegError::WorkerTagMismatch)
        );
        // bad signature
        let mut bad = mirror_signed(&miner, pool_pub, 1, "r", 100, 0);
        bad.delegation_sig[0] ^= 0xff;
        assert_eq!(
            verify_and_store(&store, &hexd(&bad), "r", "", &pool_pub, 1, 10, 1),
            Err(DelegError::BadSignature)
        );
        // miner_pkh mismatch (claim a different pkh)
        assert_eq!(
            verify_and_store(
                &store,
                &hexd(&mirror_signed(&miner, pool_pub, 1, "r", 100, 0)),
                "r",
                &"ff".repeat(20),
                &pool_pub,
                1,
                10,
                1
            ),
            Err(DelegError::MinerPkhMismatch)
        );
        // expired (expiry <= tip)
        assert_eq!(
            verify_and_store(
                &store,
                &hexd(&mirror_signed(&miner, pool_pub, 1, "r", 10, 0)),
                "r",
                "",
                &pool_pub,
                1,
                10,
                1
            ),
            Err(DelegError::Expired)
        );
        // bad hex / format
        assert_eq!(
            verify_and_store(&store, "zz", "r", "", &pool_pub, 1, 10, 1),
            Err(DelegError::BadHex)
        );
        assert_eq!(
            verify_and_store(&store, "00", "r", "", &pool_pub, 1, 10, 1),
            Err(DelegError::BadFormat)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn delegate_key_generate_reload_and_no_generate_when_absent() {
        let dir = temp_dir("key");
        std::fs::create_dir_all(&dir).unwrap();
        let kp = dir.join("poawx_delegate_key.hex");
        let k1 = DelegateKey::load_or_generate(&kp, true).unwrap();
        let k2 = DelegateKey::load_or_generate(&kp, true).unwrap();
        assert_eq!(k1.pubkey(), k2.pubkey(), "same key across reload");
        // Stored file is the 32-byte secret hex only (64 chars), no JSON/labels.
        let raw = std::fs::read_to_string(&kp).unwrap();
        assert_eq!(raw.trim().len(), 64);
        // Generation disallowed when absent (mainnet path).
        let missing = dir.join("nope.hex");
        assert!(DelegateKey::load_or_generate(&missing, false).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pool_identity_json_shape() {
        let pk = "02".to_string() + &"ab".repeat(32);
        let v = pool_identity_json(&pk, 1);
        assert_eq!(v["pool_pubkey"], pk);
        assert_eq!(v["network_id"], 1);
        assert_eq!(v["fee_bps"], 0);
        assert_eq!(v["deleg_version"], Delegation::VERSION);
        assert_eq!(v["domain"], "irium.poawx.delegation.v1");
    }

    #[test]
    fn network_id_mapping_and_loopback_guard() {
        assert!(is_loopback_bind("127.0.0.1:39520"));
        assert!(is_loopback_bind("[::1]:39520"));
        assert!(!is_loopback_bind("0.0.0.0:39520"));
        assert!(!is_loopback_bind("10.0.0.5:39520"));
    }

    // ── Phase 18B step-3: mode-1 receipt production ──

    fn p18b3_assignment_dto(
        tip: u64,
        difficulty: u32,
        lane: &str,
    ) -> crate::template::PoawxAssignment {
        crate::template::PoawxAssignment {
            height: tip,
            seed: hex::encode([0xaau8; 32]),
            commitment_nonce: hex::encode([0xbbu8; 32]),
            puzzle_difficulty: difficulty,
            lane: lane.to_string(),
        }
    }

    /// Generate a delegate key in a temp dir + a store holding a valid delegation
    /// for `miner`/`worker`. Returns (key, store, miner_pkh).
    fn p18b3_setup(
        miner: &SigningKey,
        worker: &str,
        expiry: u64,
        fee_bps: u16,
        network_id: u8,
    ) -> (DelegateKey, JsonDelegationStore, [u8; 20]) {
        let dir = temp_dir("p18b3");
        std::fs::create_dir_all(&dir).unwrap();
        let key = DelegateKey::load_or_generate(&dir.join("k.hex"), true).unwrap();
        let store = JsonDelegationStore::open(dir.join("d.json")).unwrap();
        let d = mirror_signed(miner, key.pubkey(), network_id, worker, expiry, fee_bps);
        let miner_pkh = d.miner_pkh();
        // store via the real registration path (tip 0 so expiry>tip holds).
        verify_and_store(
            &store,
            &hex::encode(d.serialize()),
            worker,
            &hex::encode(miner_pkh),
            &key.pubkey(),
            network_id,
            0,
            1,
        )
        .expect("store delegation");
        (key, store, miner_pkh)
    }

    #[test]
    fn phase18b3_build_mode1_receipt_and_root_parity() {
        let miner = SigningKey::from_slice(&[5u8; 32]).unwrap();
        let (key, store, miner_pkh) = p18b3_setup(&miner, "rig1", 1000, 0, 1);
        let dto = p18b3_assignment_dto(0, 4, "cpu"); // tip 0 -> block height 1
        let ctx = assignment_context_from_dto(&dto, 1).expect("ctx");
        let rec =
            build_mode1_pending_receipt(&store, &key, 1, miner_pkh, "rig1", &ctx).expect("receipt");
        // Field checks.
        assert_eq!(rec.worker_pkh, hex::encode(miner_pkh), "pays miner pkh");
        assert_eq!(
            rec.worker_pubkey,
            key.pubkey_hex(),
            "signer = pool delegate"
        );
        assert_eq!(rec.height, 1);
        assert_eq!(rec.lane, "cpu");
        assert!(!rec.delegation.is_empty(), "carries embedded delegation");
        // Root parity: pool root == node irx1_root_from_block_receipts.
        let block_rec = irium_node_rs::poawx::PoawxBlockReceipt {
            height: rec.height,
            lane: rec.lane.bytes().next().unwrap(),
            worker_pkh: {
                let mut a = [0u8; 20];
                a.copy_from_slice(&hex::decode(&rec.worker_pkh).unwrap());
                a
            },
            worker_pubkey: {
                let mut a = [0u8; 33];
                a.copy_from_slice(&hex::decode(&rec.worker_pubkey).unwrap());
                a
            },
            worker_sig: {
                let mut a = [0u8; 64];
                a.copy_from_slice(&hex::decode(&rec.worker_sig).unwrap());
                a
            },
            solution: {
                let mut a = [0u8; 8];
                a.copy_from_slice(&hex::decode(&rec.solution).unwrap());
                a
            },
            commitment_nonce: {
                let mut a = [0u8; 32];
                a.copy_from_slice(&hex::decode(&rec.commitment_nonce).unwrap());
                a
            },
            delegation: Some(
                irium_node_rs::poawx::Delegation::deserialize(
                    &hex::decode(&rec.delegation).unwrap(),
                )
                .unwrap(),
            ),
            phase20_ext: None,
        };
        let node_root =
            irium_node_rs::poawx::irx1_root_from_block_receipts(std::slice::from_ref(&block_rec));
        let pool_root =
            crate::block::compute_receipts_root_from_pending(std::slice::from_ref(&rec));
        assert_eq!(
            pool_root, node_root,
            "pool mode-1 root must equal node irx1_root_from_block_receipts"
        );
        // The produced solution actually meets the puzzle difficulty.
        let mut input = [0u8; 72];
        input[..32].copy_from_slice(&ctx.seed);
        input[32..64].copy_from_slice(&ctx.commitment_nonce);
        input[64..].copy_from_slice(&hex::decode(&rec.solution).unwrap());
        assert!(count_leading_zero_bits(&crate::pow::sha256d(&input)) >= ctx.difficulty);
    }

    #[test]
    fn phase18b3_no_delegation_returns_none() {
        let key_dir = temp_dir("p18b3-none");
        std::fs::create_dir_all(&key_dir).unwrap();
        let key = DelegateKey::load_or_generate(&key_dir.join("k.hex"), true).unwrap();
        let store = JsonDelegationStore::open(key_dir.join("d.json")).unwrap();
        let dto = p18b3_assignment_dto(0, 4, "cpu");
        let ctx = assignment_context_from_dto(&dto, 1).unwrap();
        // empty store -> no mode-1 receipt (mode-0 path preserved upstream).
        assert!(build_mode1_pending_receipt(&store, &key, 1, [0x11u8; 20], "rig1", &ctx).is_none());
    }

    #[test]
    fn phase18b3_assignment_height_and_lane_mismatch_fail_closed() {
        // height mismatch: assignment tip 5 but job height 1 -> None
        assert!(assignment_context_from_dto(&p18b3_assignment_dto(5, 4, "cpu"), 1).is_none());
        // correct: tip 0 -> job height 1
        assert!(assignment_context_from_dto(&p18b3_assignment_dto(0, 4, "cpu"), 1).is_some());
        // wrong lane -> None
        assert!(assignment_context_from_dto(&p18b3_assignment_dto(0, 4, "gpu"), 1).is_none());
    }

    #[test]
    fn phase18b3_expired_wrong_worker_wrong_pkh_fail_closed() {
        let miner = SigningKey::from_slice(&[5u8; 32]).unwrap();
        let (key, store, miner_pkh) = p18b3_setup(&miner, "rig1", 5, 0, 1);
        // expired: delegation expiry 5, block height 10 -> None
        let ctx10 = assignment_context_from_dto(&p18b3_assignment_dto(9, 4, "cpu"), 10).unwrap();
        assert!(build_mode1_pending_receipt(&store, &key, 1, miner_pkh, "rig1", &ctx10).is_none());
        // wrong worker (registered rig1, ask rig2) -> None
        let ctx1 = assignment_context_from_dto(&p18b3_assignment_dto(0, 4, "cpu"), 1).unwrap();
        assert!(build_mode1_pending_receipt(&store, &key, 1, miner_pkh, "rig2", &ctx1).is_none());
        // wrong miner pkh -> None
        assert!(
            build_mode1_pending_receipt(&store, &key, 1, [0xffu8; 20], "rig1", &ctx1).is_none()
        );
        // wrong network id (delegation stored for net 1; ask as net 2) -> None
        assert!(build_mode1_pending_receipt(&store, &key, 2, miner_pkh, "rig1", &ctx1).is_none());
    }

    // ── Phase 20 production primitives: parity, gate, synthetic builder ───────

    #[test]
    fn phase20_mirror_wire_parity_vs_node() {
        // RoleReward 60-byte wire parity.
        let rr = RoleRewardMirror {
            compute_contributor_pkh: [0xC1u8; 20],
            verify_contributor_pkh: [0xC2u8; 20],
            support_contributor_pkh: [0xC3u8; 20],
        };
        let node_rr = irium_node_rs::poawx::RoleReward {
            compute_contributor_pkh: [0xC1u8; 20],
            verify_contributor_pkh: [0xC2u8; 20],
            support_contributor_pkh: [0xC3u8; 20],
        };
        assert_eq!(rr.serialize(), node_rr.serialize(), "RoleReward wire parity");

        // multi_role_amounts parity over a range incl. remainder cases.
        for total in [0u64, 1, 7, 999, 5_000_000_000, 5_000_000_001, u64::MAX / 2] {
            assert_eq!(
                multi_role_amounts(total),
                irium_node_rs::poawx::multi_role_amounts(total),
                "multi_role_amounts parity total={total}"
            );
            // exact-sum invariant.
            assert_eq!(multi_role_amounts(total).iter().sum::<u64>(), total);
        }

        // fairness digest / assign_lane / role_claim_digest parity.
        let prev = [0x44u8; 32];
        for role in [
            ROLE_COMPUTE_CONTRIBUTOR,
            ROLE_VERIFY_CONTRIBUTOR,
            ROLE_SUPPORT_CONTRIBUTOR,
        ] {
            assert_eq!(
                fairness_assignment_digest(1, 100, &prev, role, 0),
                irium_node_rs::poawx::fairness_assignment_digest(1, 100, &prev, role, 0)
            );
            assert_eq!(
                assign_lane_id(1, 100, &prev, role, 0),
                irium_node_rs::poawx::assign_lane(1, 100, &prev, role, 0).id()
            );
            assert_eq!(
                role_claim_digest(1, 100, &prev, role, 0, &[0x07u8; 20], &[1u8; 32], &[2u8; 32]),
                irium_node_rs::poawx::role_claim_digest(
                    1,
                    100,
                    &prev,
                    role,
                    0,
                    &[0x07u8; 20],
                    &[1u8; 32],
                    &[2u8; 32]
                )
            );
        }

        // PoawxRoleClaim + Phase20ReceiptExt wire + digest parity.
        let mk_pool = |role: u8| PoawxRoleClaimMirror {
            role_id: role,
            lane_id: LANE_GPU_PARALLEL,
            solver_pkh: [role; 20],
            nonce: [role; 32],
            secret: [role.wrapping_add(1); 32],
            claim_digest: [role.wrapping_add(2); 32],
            commitment_hash: None,
        };
        let mk_node = |role: u8| irium_node_rs::poawx::PoawxRoleClaim {
            role_id: role,
            lane_id: LANE_GPU_PARALLEL,
            solver_pkh: [role; 20],
            nonce: [role; 32],
            secret: [role.wrapping_add(1); 32],
            claim_digest: [role.wrapping_add(2); 32],
            commitment_hash: None,
        };
        assert_eq!(mk_pool(1).serialize(), mk_node(1).serialize(), "RoleClaim parity");
        let ext = Phase20ReceiptExtMirror {
            role_reward: rr,
            compute_claim: mk_pool(1),
            verify_claim: mk_pool(2),
            support_claim: mk_pool(3),
            fee_bps: 0,
            fee_pkh: [0u8; 20],
        };
        let node_ext = irium_node_rs::poawx::Phase20ReceiptExt {
            role_reward: node_rr,
            compute_claim: mk_node(1),
            verify_claim: mk_node(2),
            support_claim: mk_node(3),
            fee_bps: 0,
            fee_pkh: [0u8; 20],
        };
        assert_eq!(ext.serialize(), node_ext.serialize(), "Phase20ReceiptExt wire parity");
        assert_eq!(ext.digest(), node_ext.digest(), "ext digest parity");
        // The node can deserialize the pool's bytes back to the identical type.
        let round = irium_node_rs::poawx::Phase20ReceiptExt::deserialize(&ext.serialize()).unwrap();
        assert_eq!(round, node_ext);
        // RoleReward pkhs extractable from the hex without full deserialize.
        let (c, v, s) = role_reward_pkhs_from_ext_hex(&hex::encode(ext.serialize())).unwrap();
        assert_eq!((c, v, s), ([0xC1u8; 20], [0xC2u8; 20], [0xC3u8; 20]));
    }

    #[test]
    fn phase20_gate_mainnet_off_and_heights() {
        let _g = p20_env_lock().lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("IRIUM_NETWORK", "testnet");
        std::env::set_var("IRIUM_POAWX_MULTI_ROLE_REWARD_ACTIVATION_HEIGHT", "5");
        std::env::remove_var("IRIUM_POAWX_FAIRNESS_MATRIX_ACTIVATION_HEIGHT");
        assert!(!phase20_production_active(10), "needs fairness too");
        std::env::set_var("IRIUM_POAWX_FAIRNESS_MATRIX_ACTIVATION_HEIGHT", "5");
        assert!(!phase20_production_active(4), "below activation");
        assert!(phase20_production_active(5), "both active at height");
        std::env::set_var("IRIUM_NETWORK", "mainnet");
        assert!(!phase20_production_active(10), "mainnet hard-off");
        std::env::remove_var("IRIUM_NETWORK");
        std::env::remove_var("IRIUM_POAWX_MULTI_ROLE_REWARD_ACTIVATION_HEIGHT");
        std::env::remove_var("IRIUM_POAWX_FAIRNESS_MATRIX_ACTIVATION_HEIGHT");
    }

    #[test]
    fn phase20_synthetic_disabled_or_mainnet_returns_none() {
        let _g = p20_env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let prev = [0x44u8; 32];
        // disabled by default on testnet (no fakes).
        std::env::set_var("IRIUM_NETWORK", "testnet");
        std::env::remove_var("IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS");
        assert!(
            build_synthetic_phase20_ext(1, 10, &prev, &[0x11u8; 20], &[]).is_none(),
            "synthetic disabled by default"
        );
        // enabled flag but mainnet (network_id 0 passed) -> None.
        std::env::set_var("IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS", "1");
        std::env::set_var("IRIUM_NETWORK", "mainnet");
        assert!(
            build_synthetic_phase20_ext(0, 10, &prev, &[0x11u8; 20], &[]).is_none(),
            "mainnet hard-off"
        );
        std::env::remove_var("IRIUM_NETWORK");
        std::env::remove_var("IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS");
    }

    #[test]
    fn phase20_synthetic_builder_valid_and_node_validator_passes() {
        let _g = p20_env_lock().lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("IRIUM_NETWORK", "testnet");
        std::env::set_var("IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS", "1");
        let net = 1u8;
        let height = 500u64;
        let prev = [0x55u8; 32];
        let primary = [0xA1u8; 20];
        let total = 5_000_000_001u64;

        let ext = build_synthetic_phase20_ext(net, height, &prev, &primary, &[]).expect("ext");
        // three role claims, expected role ids, solver == primary (MVP single-miner).
        assert_eq!(ext.compute_claim.role_id, ROLE_COMPUTE_CONTRIBUTOR);
        assert_eq!(ext.verify_claim.role_id, ROLE_VERIFY_CONTRIBUTOR);
        assert_eq!(ext.support_claim.role_id, ROLE_SUPPORT_CONTRIBUTOR);
        assert_eq!(ext.role_reward.compute_contributor_pkh, ext.compute_claim.solver_pkh);
        assert_eq!(ext.role_reward.verify_contributor_pkh, ext.verify_claim.solver_pkh);
        assert_eq!(ext.role_reward.support_contributor_pkh, ext.support_claim.solver_pkh);
        assert_eq!(ext.fee_bps, 0, "official fee-0 only");
        assert_eq!(ext.fee_pkh, [0u8; 20]);
        // deterministic / reproducible.
        let ext2 = build_synthetic_phase20_ext(net, height, &prev, &primary, &[]).unwrap();
        assert_eq!(ext.serialize(), ext2.serialize(), "synthetic builder is deterministic");

        // each claim validates via the node consensus primitive.
        let node_ext =
            irium_node_rs::poawx::Phase20ReceiptExt::deserialize(&ext.serialize()).unwrap();
        for c in [
            &node_ext.compute_claim,
            &node_ext.verify_claim,
            &node_ext.support_claim,
        ] {
            irium_node_rs::poawx::validate_role_claim(c, net, height, &prev, 0)
                .expect("synthetic role claim must validate");
        }

        // Build the canonical pool coinbase outputs and assert the AUTHORITATIVE
        // node validators accept them (item 8). p2pkh = 76 a9 14 <20> 88 ac.
        let amts = multi_role_amounts(total);
        let p2pkh = |pkh: &[u8; 20]| -> Vec<u8> {
            let mut s = vec![0x76u8, 0xa9, 0x14];
            s.extend_from_slice(pkh);
            s.extend_from_slice(&[0x88, 0xac]);
            s
        };
        let irx1 = irium_node_rs::tx::TxOutput {
            value: 0,
            script_pubkey: vec![0x6a, 0x24, b'i', b'r', b'x', b'1'],
        };
        let outs = vec![
            irx1,
            irium_node_rs::tx::TxOutput { value: amts[0], script_pubkey: p2pkh(&primary) },
            irium_node_rs::tx::TxOutput {
                value: amts[1],
                script_pubkey: p2pkh(&node_ext.role_reward.compute_contributor_pkh),
            },
            irium_node_rs::tx::TxOutput {
                value: amts[2],
                script_pubkey: p2pkh(&node_ext.role_reward.verify_contributor_pkh),
            },
            irium_node_rs::tx::TxOutput {
                value: amts[3],
                script_pubkey: p2pkh(&node_ext.role_reward.support_contributor_pkh),
            },
        ];
        irium_node_rs::chain::validate_phase20_production_payout(
            &outs, &primary, total, height, &prev, net, &node_ext, false,
        )
        .expect("node validator must accept the pool-produced fixture");
        irium_node_rs::chain::validate_poawx_coinbase_payout(
            &outs,
            &primary,
            total,
            Some(&node_ext.role_reward),
            None,
        )
        .expect("node coinbase payout must accept the pool-produced fixture");

        // Tamper cases rejected by the node validator.
        let mut bad = outs.clone();
        bad[1].value += 1;
        assert!(irium_node_rs::chain::validate_phase20_production_payout(
            &bad, &primary, total, height, &prev, net, &node_ext, false
        )
        .is_err(), "wrong amount must reject");
        let mut bad = outs.clone();
        bad.swap(1, 2);
        assert!(irium_node_rs::chain::validate_phase20_production_payout(
            &bad, &primary, total, height, &prev, net, &node_ext, false
        )
        .is_err(), "wrong order must reject");
        let mut bad = outs.clone();
        bad.push(irium_node_rs::tx::TxOutput { value: 1, script_pubkey: p2pkh(&[0x9Au8; 20]) });
        assert!(irium_node_rs::chain::validate_phase20_production_payout(
            &bad, &primary, total, height, &prev, net, &node_ext, false
        )
        .is_err(), "hidden extra output must reject");

        std::env::remove_var("IRIUM_NETWORK");
        std::env::remove_var("IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS");
    }
}
