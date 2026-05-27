//! PPLNS payout subsystem.
//!
//! When a miner finds a block, iriumd accepts our submission, and the block
//! eventually matures (coinbase becomes spendable after 100 blocks), the pool
//! distributes 99% of the 50 IRM block reward proportionally to all miners
//! who contributed shares in the last 10,000-share window. The pool retains
//! 1% as fee.
//!
//! Two-stage flow:
//!
//!   STAGE 1 (block-find time, immediate):
//!     stratum::mark_submit_accepted() hook calls queue_block_for_payout()
//!     which snapshots SHARE_WINDOW under lock, computes per-address
//!     weighted contributions, and writes the snapshot to a persisted
//!     PendingBlock entry. No wallet send yet — the coinbase is still
//!     immature (unspendable for 100 blocks).
//!
//!   STAGE 2 (maturity poller, every 30s):
//!     maturity_poller queries iriumd /status for chain_height. For each
//!     PendingBlock where chain_height >= block.height + 100, verifies the
//!     block is still canonical (re-fetches block_by_height, compares hash
//!     — guards against reorgs), computes per-miner sat amounts from the
//!     saved snapshot, accumulates into PENDING_PAYOUTS, then shells out to
//!     irium-wallet send for every miner whose pending balance clears
//!     MIN_PAYOUT_SATS. Sub-dust amounts carry over to the next block.
//!
//! State persisted to disk (atomic write via tmp + rename):
//!   /opt/irium-pool/pending_blocks.json    — queued blocks awaiting maturity
//!   /opt/irium-pool/pending_payouts.json   — per-address carry-over sats
//!   /opt/irium-pool/paid_blocks.json       — idempotency set of paid heights
//!   /opt/irium-pool/payout_log.jsonl       — append-only log of sent payouts

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tracing::{error, info, warn};

// Constants

const POOL_PAYOUT_PKH_HEX_DEFAULT: &str = "f2053fc49e48a5108e91e645e4c9c16b397cd406";
const POOL_PAYOUT_ADDR_DEFAULT: &str = "QKL4wKuGmApRYL44FsSfkQTg8ibja6JBqv";

const BLOCK_REWARD_SATS: u64 = 5_000_000_000;
const POOL_FEE_BPS: u64 = 100; // 1.00%
const MIN_PAYOUT_SATS: u64 = 1_000_000; // 0.01 IRM dust threshold
const SHARE_WINDOW_CAP: usize = 10_000;
const PAYOUT_LOG_CAP: usize = 50;
const COINBASE_MATURITY_BLOCKS: u64 = 100;
const MATURITY_POLL_INTERVAL_SECS: u64 = 30;

const STATE_DIR: &str = "/opt/irium-pool";
const PENDING_BLOCKS_FILE: &str = "/opt/irium-pool/pending_blocks.json";
const PENDING_PAYOUTS_FILE: &str = "/opt/irium-pool/pending_payouts.json";
const PAID_BLOCKS_FILE: &str = "/opt/irium-pool/paid_blocks.json";
const PAYOUT_LOG_FILE: &str = "/opt/irium-pool/payout_log.jsonl";

const WALLET_BIN_DEFAULT: &str = "/home/irium/irium/target/release/irium-wallet";

// Pool pkh accessor — lazy hex-decode of POOL_PAYOUT_PKH_HEX_DEFAULT or env override.
pub static POOL_PAYOUT_PKH_BYTES: LazyLock<[u8; 20]> = LazyLock::new(|| {
    let hex_str = std::env::var("IRIUM_POOL_PAYOUT_PKH")
        .unwrap_or_else(|_| POOL_PAYOUT_PKH_HEX_DEFAULT.to_string());
    let bytes = hex::decode(&hex_str).unwrap_or_else(|e| {
        panic!("invalid IRIUM_POOL_PAYOUT_PKH hex: {}", e);
    });
    bytes
        .try_into()
        .unwrap_or_else(|_| panic!("IRIUM_POOL_PAYOUT_PKH must be exactly 20 bytes"))
});

// Shared state

static SHARE_WINDOW: LazyLock<Mutex<VecDeque<(String, f64, u64)>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(SHARE_WINDOW_CAP)));

static PENDING_BLOCKS: LazyLock<Mutex<HashMap<u64, PendingBlock>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static PENDING_PAYOUTS: LazyLock<Mutex<HashMap<String, u64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static PAID_BLOCKS: LazyLock<Mutex<HashSet<u64>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

static PAYOUT_LOG: LazyLock<Mutex<VecDeque<PayoutEvent>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(PAYOUT_LOG_CAP)));

// Types

#[derive(Clone, Serialize, Deserialize)]
pub struct PendingBlock {
    pub height: u64,
    pub canonical_hash: String,
    pub found_at_unix: u64,
    pub counts: HashMap<String, u64>,
    pub weights: HashMap<String, f64>,
    pub total_weighted: f64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PayoutEvent {
    pub block_height: u64,
    pub canonical_hash: String,
    pub miner_address: String,
    pub amount_sats: u64,
    pub share_count: u64,
    pub pct: f64,
    pub tx_id: Option<String>,
    pub timestamp: u64,
    pub status: String,
}

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// Public hook 1: share accept

pub fn record_share(worker: &str, diff: f64) {
    if worker.is_empty() || diff <= 0.0 {
        return;
    }
    let now = unix_now_secs();
    let mut window = SHARE_WINDOW.lock().unwrap_or_else(|e| e.into_inner());
    window.push_back((worker.to_string(), diff, now));
    while window.len() > SHARE_WINDOW_CAP {
        window.pop_front();
    }
}

// Public hook 2: block accepted by iriumd

pub fn queue_block_for_payout(block_height: u64, canonical_hash: String) {
    {
        let paid = PAID_BLOCKS.lock().unwrap_or_else(|e| e.into_inner());
        if paid.contains(&block_height) {
            info!("[payout-queue] block={} already paid, skipping", block_height);
            return;
        }
    }
    {
        let pending = PENDING_BLOCKS.lock().unwrap_or_else(|e| e.into_inner());
        if pending.contains_key(&block_height) {
            info!("[payout-queue] block={} already queued, skipping", block_height);
            return;
        }
    }
    let (counts, weights, total_weighted) = snapshot_share_window();
    let n_miners = counts.len();
    let now = unix_now_secs();
    let pending_block = PendingBlock {
        height: block_height,
        canonical_hash: canonical_hash.clone(),
        found_at_unix: now,
        counts,
        weights,
        total_weighted,
    };
    {
        let mut pending = PENDING_BLOCKS.lock().unwrap_or_else(|e| e.into_inner());
        pending.insert(block_height, pending_block);
    }
    if let Err(e) = persist_pending_blocks() {
        warn!("[payout-queue] persist failed: {}", e);
    }
    if n_miners == 0 || total_weighted <= 0.0 {
        warn!(
            "[payout-queue] block={} EMPTY share window — pool keeps 100% on maturity",
            block_height
        );
    } else {
        let short_hash = &canonical_hash[..16.min(canonical_hash.len())];
        info!(
            "[payout-queue] block={} hash={} miners={} total_weighted={:.0} matures_at={}",
            block_height,
            short_hash,
            n_miners,
            total_weighted,
            block_height + COINBASE_MATURITY_BLOCKS
        );
    }
}

fn snapshot_share_window() -> (HashMap<String, u64>, HashMap<String, f64>, f64) {
    let window = SHARE_WINDOW.lock().unwrap_or_else(|e| e.into_inner());
    let mut counts: HashMap<String, u64> = HashMap::new();
    let mut weights: HashMap<String, f64> = HashMap::new();
    let mut total_weighted = 0.0_f64;
    for (worker, diff, _ts) in window.iter() {
        let addr = worker.split('.').next().unwrap_or("").trim();
        if addr.is_empty() {
            continue;
        }
        *counts.entry(addr.to_string()).or_insert(0) += 1;
        *weights.entry(addr.to_string()).or_insert(0.0) += diff;
        total_weighted += diff;
    }
    (counts, weights, total_weighted)
}

// Persistence (atomic: tmp + rename)

fn atomic_write_json<T: Serialize>(path: &str, value: &T) -> Result<(), String> {
    if let Err(e) = std::fs::create_dir_all(STATE_DIR) {
        return Err(format!("mkdir {}: {}", STATE_DIR, e));
    }
    let tmp_path = format!("{}.tmp", path);
    let s = serde_json::to_string_pretty(value).map_err(|e| format!("serialize {}: {}", path, e))?;
    std::fs::write(&tmp_path, s).map_err(|e| format!("write {}: {}", tmp_path, e))?;
    std::fs::rename(&tmp_path, path)
        .map_err(|e| format!("rename {} -> {}: {}", tmp_path, path, e))?;
    Ok(())
}

fn persist_pending_blocks() -> Result<(), String> {
    let snapshot: HashMap<u64, PendingBlock> = PENDING_BLOCKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    atomic_write_json(PENDING_BLOCKS_FILE, &snapshot)
}

fn persist_pending_payouts() -> Result<(), String> {
    let snapshot: HashMap<String, u64> = PENDING_PAYOUTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    atomic_write_json(PENDING_PAYOUTS_FILE, &snapshot)
}

fn persist_paid_blocks() -> Result<(), String> {
    let snapshot: HashSet<u64> = PAID_BLOCKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    atomic_write_json(PAID_BLOCKS_FILE, &snapshot)
}

fn append_payout_log_jsonl(event: &PayoutEvent) -> Result<(), String> {
    let line = serde_json::to_string(event).map_err(|e| format!("serialize payout: {}", e))?;
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(PAYOUT_LOG_FILE)
        .map_err(|e| format!("open {}: {}", PAYOUT_LOG_FILE, e))?;
    writeln!(f, "{}", line).map_err(|e| format!("write {}: {}", PAYOUT_LOG_FILE, e))?;
    Ok(())
}

fn push_payout_event(event: PayoutEvent) {
    let _ = append_payout_log_jsonl(&event);
    let mut log = PAYOUT_LOG.lock().unwrap_or_else(|e| e.into_inner());
    log.push_back(event);
    while log.len() > PAYOUT_LOG_CAP {
        log.pop_front();
    }
}

fn load_persisted_state() {
    if let Ok(s) = std::fs::read_to_string(PENDING_BLOCKS_FILE) {
        if let Ok(loaded) = serde_json::from_str::<HashMap<u64, PendingBlock>>(&s) {
            let mut g = PENDING_BLOCKS.lock().unwrap_or_else(|e| e.into_inner());
            *g = loaded;
        }
    }
    if let Ok(s) = std::fs::read_to_string(PENDING_PAYOUTS_FILE) {
        if let Ok(loaded) = serde_json::from_str::<HashMap<String, u64>>(&s) {
            let mut g = PENDING_PAYOUTS.lock().unwrap_or_else(|e| e.into_inner());
            *g = loaded;
        }
    }
    if let Ok(s) = std::fs::read_to_string(PAID_BLOCKS_FILE) {
        if let Ok(loaded) = serde_json::from_str::<HashSet<u64>>(&s) {
            let mut g = PAID_BLOCKS.lock().unwrap_or_else(|e| e.into_inner());
            *g = loaded;
        }
    }
}

// Maturity poller — background task spawned from main.rs

pub async fn maturity_poller(rpc_base: String, rpc_token: String) {
    info!(
        "[payout] maturity poller starting (interval={}s, maturity={} blocks)",
        MATURITY_POLL_INTERVAL_SECS, COINBASE_MATURITY_BLOCKS
    );
    load_persisted_state();
    {
        let n_pending = PENDING_BLOCKS.lock().map(|m| m.len()).unwrap_or(0);
        let n_paid = PAID_BLOCKS.lock().map(|m| m.len()).unwrap_or(0);
        let n_carry = PENDING_PAYOUTS.lock().map(|m| m.len()).unwrap_or(0);
        info!(
            "[payout] state loaded: pending_blocks={} paid_blocks={} carry_over_addrs={}",
            n_pending, n_paid, n_carry
        );
    }
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            error!("[payout] http client init failed, poller exiting: {}", e);
            return;
        }
    };
    loop {
        tokio::time::sleep(Duration::from_secs(MATURITY_POLL_INTERVAL_SECS)).await;
        if let Err(e) = poll_once(&client, &rpc_base, &rpc_token).await {
            warn!("[payout] poll cycle error: {}", e);
        }
    }
}

async fn poll_once(
    client: &reqwest::Client,
    rpc_base: &str,
    rpc_token: &str,
) -> Result<(), String> {
    let status_url = format!("{}/status", rpc_base.trim_end_matches('/'));
    let resp = client
        .get(&status_url)
        .send()
        .await
        .map_err(|e| format!("status fetch: {}", e))?;
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("status parse: {}", e))?;
    let chain_height = v["height"]
        .as_u64()
        .ok_or_else(|| "no height in /status".to_string())?;

    let matured: Vec<u64> = {
        let pending = PENDING_BLOCKS.lock().unwrap_or_else(|e| e.into_inner());
        pending
            .keys()
            .filter(|&&h| chain_height >= h + COINBASE_MATURITY_BLOCKS)
            .copied()
            .collect()
    };

    for height in matured {
        process_matured_block(client, rpc_base, rpc_token, height).await;
    }
    Ok(())
}

async fn process_matured_block(
    client: &reqwest::Client,
    rpc_base: &str,
    rpc_token: &str,
    height: u64,
) {
    {
        let paid = PAID_BLOCKS.lock().unwrap_or_else(|e| e.into_inner());
        if paid.contains(&height) {
            return;
        }
    }
    let pending_block = {
        let pending = PENDING_BLOCKS.lock().unwrap_or_else(|e| e.into_inner());
        match pending.get(&height) {
            Some(pb) => pb.clone(),
            None => return,
        }
    };

    // Reorg guard
    let block_url = format!(
        "{}/rpc/block?height={}",
        rpc_base.trim_end_matches('/'),
        height
    );
    let resp = match client.get(&block_url).bearer_auth(rpc_token).send().await {
        Ok(r) => r,
        Err(e) => {
            warn!(
                "[payout] block={} canonical-verify fetch failed: {} — will retry next poll",
                height, e
            );
            return;
        }
    };
    if !resp.status().is_success() {
        warn!(
            "[payout] block={} canonical-verify non-200: {} — will retry next poll",
            height,
            resp.status()
        );
        return;
    }
    let block_json: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            warn!(
                "[payout] block={} canonical-verify parse failed: {} — will retry next poll",
                height, e
            );
            return;
        }
    };
    let on_chain_hash = block_json["header"]["hash"].as_str().unwrap_or("");
    if on_chain_hash != pending_block.canonical_hash {
        warn!(
            "[payout] block={} REORGED: queued_hash={} on_chain_hash={} — removing from pending",
            height, pending_block.canonical_hash, on_chain_hash
        );
        {
            let mut pending = PENDING_BLOCKS.lock().unwrap_or_else(|e| e.into_inner());
            pending.remove(&height);
        }
        let _ = persist_pending_blocks();
        return;
    }

    if pending_block.total_weighted <= 0.0 || pending_block.counts.is_empty() {
        warn!(
            "[payout] block={} matured with empty share window — pool retains 100% of {} sats",
            height, BLOCK_REWARD_SATS
        );
        finalize_block(height);
        return;
    }

    let distributable_sats = BLOCK_REWARD_SATS - (BLOCK_REWARD_SATS * POOL_FEE_BPS / 10_000);
    {
        let mut payouts = PENDING_PAYOUTS.lock().unwrap_or_else(|e| e.into_inner());
        for (addr, weight) in &pending_block.weights {
            let share_sats =
                ((weight / pending_block.total_weighted) * distributable_sats as f64) as u64;
            *payouts.entry(addr.clone()).or_insert(0) += share_sats;
        }
    }
    let _ = persist_pending_payouts();

    let to_send: Vec<(String, u64)> = {
        let payouts = PENDING_PAYOUTS.lock().unwrap_or_else(|e| e.into_inner());
        payouts
            .iter()
            .filter(|(_, &amt)| amt >= MIN_PAYOUT_SATS)
            .map(|(addr, &amt)| (addr.clone(), amt))
            .collect()
    };

    let pool_addr = std::env::var("IRIUM_POOL_PAYOUT_ADDRESS")
        .unwrap_or_else(|_| POOL_PAYOUT_ADDR_DEFAULT.to_string());

    for (miner_addr, amt_sats) in to_send {
        let count = pending_block.counts.get(&miner_addr).copied().unwrap_or(0);
        let pct = if pending_block.total_weighted > 0.0 {
            (pending_block.weights.get(&miner_addr).copied().unwrap_or(0.0)
                / pending_block.total_weighted)
                * 100.0
        } else {
            0.0
        };
        match execute_wallet_send(&pool_addr, &miner_addr, amt_sats, rpc_base).await {
            Ok(txid) => {
                warn!(
                    "[payout] block={} miner={} amount={:.8} shares={} pct={:.2}",
                    height,
                    miner_addr,
                    amt_sats as f64 / 100_000_000.0,
                    count,
                    pct
                );
                {
                    let mut payouts = PENDING_PAYOUTS.lock().unwrap_or_else(|e| e.into_inner());
                    payouts.insert(miner_addr.clone(), 0);
                }
                push_payout_event(PayoutEvent {
                    block_height: height,
                    canonical_hash: pending_block.canonical_hash.clone(),
                    miner_address: miner_addr.clone(),
                    amount_sats: amt_sats,
                    share_count: count,
                    pct,
                    tx_id: Some(txid),
                    timestamp: unix_now_secs(),
                    status: "sent".to_string(),
                });
            }
            Err(e) => {
                error!(
                    "[payout] block={} miner={} amount={:.8} SEND FAILED: {} — pending balance retained, will retry next block",
                    height,
                    miner_addr,
                    amt_sats as f64 / 100_000_000.0,
                    e
                );
                push_payout_event(PayoutEvent {
                    block_height: height,
                    canonical_hash: pending_block.canonical_hash.clone(),
                    miner_address: miner_addr.clone(),
                    amount_sats: amt_sats,
                    share_count: count,
                    pct,
                    tx_id: None,
                    timestamp: unix_now_secs(),
                    status: format!("failed: {}", e),
                });
            }
        }
    }

    let _ = persist_pending_payouts();
    finalize_block(height);
}

fn finalize_block(height: u64) {
    {
        let mut paid = PAID_BLOCKS.lock().unwrap_or_else(|e| e.into_inner());
        paid.insert(height);
    }
    {
        let mut pending = PENDING_BLOCKS.lock().unwrap_or_else(|e| e.into_inner());
        pending.remove(&height);
    }
    let _ = persist_paid_blocks();
    let _ = persist_pending_blocks();
}

async fn execute_wallet_send(
    pool_addr: &str,
    miner_addr: &str,
    amount_sats: u64,
    rpc_base: &str,
) -> Result<String, String> {
    let amount_irm = format!("{:.8}", amount_sats as f64 / 100_000_000.0);
    let wallet_bin =
        std::env::var("IRIUM_WALLET_BIN").unwrap_or_else(|_| WALLET_BIN_DEFAULT.to_string());
    let rpc_token = std::env::var("IRIUM_RPC_TOKEN").unwrap_or_default();
    let output = tokio::process::Command::new(&wallet_bin)
        .env("IRIUM_RPC_TOKEN", &rpc_token)
        .args([
            "send",
            pool_addr,
            miner_addr,
            &amount_irm,
            "--rpc",
            rpc_base,
        ])
        .output()
        .await
        .map_err(|e| format!("wallet send spawn: {}", e))?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let raw = stdout.trim();
        let txid = raw.strip_prefix("txid ").unwrap_or(raw).to_string();
        if txid.len() != 64 || !txid.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("wallet returned invalid txid: {}", txid));
        }
        Ok(txid)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(stderr)
    }
}

// Read-only views for /payouts and /miners_payout HTTP endpoints

pub fn payouts_view_json() -> serde_json::Value {
    let log = PAYOUT_LOG.lock().unwrap_or_else(|e| e.into_inner());
    let arr: Vec<serde_json::Value> = log
        .iter()
        .map(|e| {
            serde_json::json!({
                "block_height": e.block_height,
                "canonical_hash": e.canonical_hash,
                "miner_address": e.miner_address,
                "amount_sats": e.amount_sats,
                "amount_irm": e.amount_sats as f64 / 100_000_000.0,
                "share_count": e.share_count,
                "pct": e.pct,
                "tx_id": e.tx_id,
                "timestamp": e.timestamp,
                "status": e.status,
            })
        })
        .collect();
    serde_json::json!({
        "count": arr.len(),
        "payouts": arr,
    })
}

pub fn miners_payout_view_json() -> serde_json::Value {
    let (counts, weights, total_weighted) = snapshot_share_window();
    let distributable_sats = BLOCK_REWARD_SATS - (BLOCK_REWARD_SATS * POOL_FEE_BPS / 10_000);
    let mut by_addr = serde_json::Map::new();
    for (addr, &cnt) in counts.iter() {
        let w = weights.get(addr).copied().unwrap_or(0.0);
        let pct = if total_weighted > 0.0 {
            (w / total_weighted) * 100.0
        } else {
            0.0
        };
        let est_sats = if total_weighted > 0.0 {
            ((w / total_weighted) * distributable_sats as f64) as u64
        } else {
            0
        };
        by_addr.insert(
            addr.clone(),
            serde_json::json!({
                "pending_shares": cnt,
                "weighted": w,
                "pct_of_window": pct,
                "estimated_payout_irm": est_sats as f64 / 100_000_000.0,
            }),
        );
    }
    let window_entries = SHARE_WINDOW.lock().map(|w| w.len()).unwrap_or(0);
    serde_json::json!({
        "share_window_size": {
            "entries": window_entries,
            "unique_addresses": counts.len(),
            "total_weighted": total_weighted,
        },
        "by_address": serde_json::Value::Object(by_addr),
    })
}
