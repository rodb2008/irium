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
            DelegError::MinerPkhMismatch => "delegation: miner_pkh does not match miner_pubkey".into(),
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

/// Spawn the delegation HTTP server if `IRIUM_POAWX_DELEGATION_BIND` is set.
/// Default = DISABLED (no bind). Refuses any non-loopback bind. Safe to call
/// unconditionally; it is a no-op unless explicitly configured.
pub fn maybe_spawn(rpc_base: String, rpc_token: String) {
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
    tokio::spawn(async move {
        if let Err(e) = serve(bind, rpc_base, rpc_token).await {
            warn!("[poawx-deleg] server stopped: {e}");
        }
    });
}

struct ServerCtx {
    network_id: u8,
    /// None on mainnet (delegation disabled → 503).
    key: Option<Arc<DelegateKey>>,
    store: Option<Arc<dyn DelegationStore>>,
    rpc_base: String,
    rpc_token: String,
}

async fn serve(bind: String, rpc_base: String, rpc_token: String) -> anyhow::Result<()> {
    let network_id = network_id_from_env();
    let mainnet = network_id == 0;
    let (key, store) = if mainnet {
        warn!("[poawx-deleg] mainnet context: delegation endpoints will return 503 (mode-1 hard-off)");
        (None, None)
    } else {
        let key = DelegateKey::load_or_generate(&delegate_key_path(), true)
            .map_err(|e| anyhow::anyhow!("delegate key: {e}"))?;
        let store = Arc::new(
            JsonDelegationStore::open(delegations_path())
                .map_err(|e| anyhow::anyhow!("delegation store: {e}"))?,
        ) as Arc<dyn DelegationStore>;
        info!(
            "[poawx-deleg] pool_pubkey={} network_id={network_id}",
            key.pubkey_hex()
        );
        (Some(Arc::new(key)), Some(store))
    };

    let listener = TcpListener::bind(&bind).await?;
    info!("[poawx-deleg] listening on http://{bind} (loopback-only, network_id={network_id})");

    let ctx = Arc::new(ServerCtx {
        network_id,
        key,
        store,
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
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
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
    let tpl = crate::template::TemplateClient::new(rpc_base.to_string(), rpc_token.to_string()).ok()?;
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
        ("GET", "/poawx/pool-identity") => match &ctx.key {
            None => {
                respond(
                    &mut stream,
                    503,
                    "Service Unavailable",
                    &serde_json::json!({"error":"delegation unavailable on mainnet"}),
                )
                .await
            }
            Some(key) => {
                let v = pool_identity_json(&key.pubkey_hex(), ctx.network_id);
                respond(&mut stream, 200, "OK", &v).await
            }
        },
        ("POST", "/poawx/delegation") => {
            let (key, store) = match (&ctx.key, &ctx.store) {
                (Some(k), Some(s)) => (k, s),
                _ => {
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
        assert_eq!(canon.message_hash(), mir.message_hash(), "message_hash parity");
        assert_eq!(canon.digest(), mir.digest(), "digest parity");
        assert_eq!(Delegation::VERSION, irium_node_rs::poawx::Delegation::VERSION);
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
        assert!(mir.verify_signature().is_ok(), "mirror verifies canonical-signed");
        assert_eq!(mir.miner_pkh(), canon.miner_pkh(), "miner_pkh parity");

        // Tampered canonical delegation must be rejected by the mirror.
        let mut tampered = canon.clone();
        tampered.delegation_sig[0] ^= 0xff;
        let mir_bad = Delegation::deserialize(&tampered.serialize()).unwrap();
        assert!(mir_bad.verify_signature().is_err(), "mirror rejects tampered");
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
        assert_eq!(s2.get(&rec.miner_pkh, "rig1").unwrap(), rec, "persists across reload");
        // Registry stores no private keys.
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
            verify_and_store(&store, &hexd(&mirror_signed(&miner, pool_pub, 0, "r", 100, 0)), "r", "", &pool_pub, 0, 10, 1),
            Err(DelegError::Mainnet)
        );
        // network mismatch
        assert_eq!(
            verify_and_store(&store, &hexd(&mirror_signed(&miner, pool_pub, 2, "r", 100, 0)), "r", "", &pool_pub, 1, 10, 1),
            Err(DelegError::NetworkMismatch)
        );
        // pool pubkey mismatch
        assert_eq!(
            verify_and_store(&store, &hexd(&mirror_signed(&miner, other_pool, 1, "r", 100, 0)), "r", "", &pool_pub, 1, 10, 1),
            Err(DelegError::PoolPubkeyMismatch)
        );
        // fee > 0
        assert_eq!(
            verify_and_store(&store, &hexd(&mirror_signed(&miner, pool_pub, 1, "r", 100, 100)), "r", "", &pool_pub, 1, 10, 1),
            Err(DelegError::NonZeroFee)
        );
        // worker_tag mismatch (claimed worker differs from signed tag)
        assert_eq!(
            verify_and_store(&store, &hexd(&mirror_signed(&miner, pool_pub, 1, "rig1", 100, 0)), "rig2", "", &pool_pub, 1, 10, 1),
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
            verify_and_store(&store, &hexd(&mirror_signed(&miner, pool_pub, 1, "r", 100, 0)), "r", &"ff".repeat(20), &pool_pub, 1, 10, 1),
            Err(DelegError::MinerPkhMismatch)
        );
        // expired (expiry <= tip)
        assert_eq!(
            verify_and_store(&store, &hexd(&mirror_signed(&miner, pool_pub, 1, "r", 10, 0)), "r", "", &pool_pub, 1, 10, 1),
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
}
