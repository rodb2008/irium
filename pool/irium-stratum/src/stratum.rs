use crate::block::{
    build_coinbase_tx_with_roll, build_merkle_branches, coinbase_prefix_suffix_with_roll, header_bytes,
    merkle_root_from_coinbase, parse_address_to_pkh, parse_hex32, parse_u32_hex,
};
use crate::pow::{sha256d, target_from_bits, target_from_difficulty_with_limit};
use crate::template::{GetBlockTemplate, TemplateClient};
use anyhow::{anyhow, Result};
use num_bigint::BigUint;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::collections::{HashMap, HashSet};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub enum HashCmpMode {
    Be,
    Le,
}

impl HashCmpMode {
    pub fn from_env(value: Option<String>) -> Self {
        match value.unwrap_or_else(|| "le".to_string()).to_ascii_lowercase().as_str() {
            "be" => Self::Be,
            _ => Self::Le,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Be => "be",
            Self::Le => "le",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MinerClass {
    Cpu,
    Gpu,
    Asic,
    Unknown,
}

impl MinerClass {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
            Self::Asic => "asic",
            Self::Unknown => "unknown",
        }
    }
}

fn classify_miner_agent(agent: &str) -> MinerClass {
    let a = agent.to_ascii_lowercase();
    if a.contains("cpuminer") || a.contains("minerd") {
        MinerClass::Cpu
    } else if a.contains("ccminer") || a.contains("cuda") || a.contains("nvml") {
        MinerClass::Gpu
    } else if a.contains("cgminer")
        || a.contains("bmminer")
        || a.contains("antminer")
        || a.contains("whatsminer")
        || a.contains("avalon")
        || a.contains("asic")
    {
        MinerClass::Asic
    } else {
        MinerClass::Unknown
    }
}

fn miner_diff_bounds(class: &MinerClass) -> (f64, f64) {
    match class {
        MinerClass::Cpu => (0.5, 16.0),
        MinerClass::Gpu => (8.0, 256.0),
        MinerClass::Asic => (1024.0, 65536.0),
        MinerClass::Unknown => (0.5, 256.0),
    }
}

#[derive(Clone, Debug)]
pub enum MinerFamilyMode {
    Auto,
    Asic,
    Ccminer,
    Cpuminer,
}

impl MinerFamilyMode {
    pub fn from_env(value: Option<String>) -> Self {
        match value.unwrap_or_else(|| "auto".to_string()).to_ascii_lowercase().as_str() {
            "asic" => Self::Asic,
            "ccminer" => Self::Ccminer,
            "cpuminer" => Self::Cpuminer,
            _ => Self::Auto,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Asic => "asic",
            Self::Ccminer => "ccminer",
            Self::Cpuminer => "cpuminer",
        }
    }
}

#[derive(Clone)]
pub struct StratumConfig {
    pub bind: String,
    pub metrics_bind: Option<String>,
    pub native_bind: Option<String>,
    pub default_diff: f64,
    pub extranonce1_size: usize,
    pub refresh_ms: u64,
    pub rpc_base: String,
    pub rpc_token: String,
    pub pow_limit: BigUint,
    pub hash_cmp_mode: HashCmpMode,
    pub soft_accept_invalid_shares: bool,
    pub miner_family_mode: MinerFamilyMode,
    pub sharecheck_samples: usize,
    pub vardiff_enabled: bool,
    pub vardiff_min_diff: f64,
    pub vardiff_max_diff: f64,
    pub vardiff_target_share_secs: u64,
    pub vardiff_retarget_secs: u64,
    pub max_template_age_seconds: u64,
    pub coinbase_bip34: bool,
    pub keepalive_notify_secs: u64,
    pub job_rotate_secs: u64,
    pub dev_simulate_submit: bool,
}

#[derive(Clone)]
struct Job {
    job_id: String,
    height: u64,
    prev_hash: [u8; 32],
    bits: u32,
    nbits_hex: String,
    ntime_hex: String,
    coinbase_value: u64,
    tx_hex: Vec<String>,
    branches: Vec<[u8; 32]>,
    template_target_hex: String,
    roll_nonce: u32,
    clean_jobs: bool,
}

#[derive(Clone)]
struct SessionState {
    extranonce1: Vec<u8>,
    worker: Option<String>,
    pkh: Option<[u8; 20]>,
    difficulty: f64,
    miner_class: MinerClass,
    current_job: Option<Job>,
    last_share_ts: Option<u64>,
    last_retarget_ts: u64,
    coinbase_bip34: bool,
    last_notify_coinbase1: Vec<u8>,
    last_notify_coinbase2: Vec<u8>,
    seen_submissions: HashSet<String>,
}


#[derive(Clone, Default)]
struct TemplateState {
    last_height: u64,
    last_prevhash: String,
    last_update_unix: u64,
}

static ACTIVE_SESSIONS: AtomicU64 = AtomicU64::new(0);
static ACCEPTED_SHARES: AtomicU64 = AtomicU64::new(0);
static REJECTED_SHARES: AtomicU64 = AtomicU64::new(0);
static CANONICAL_SHARES: AtomicU64 = AtomicU64::new(0);
static COMPAT_SHARES: AtomicU64 = AtomicU64::new(0);
static JOBS_ROTATED: AtomicU64 = AtomicU64::new(0);
static EXTRANONCE_ROLLS: AtomicU64 = AtomicU64::new(0);
static VARDIFF_ADJUSTMENTS: AtomicU64 = AtomicU64::new(0);
static VARDIFF_INTERVAL_SUM_MS: AtomicU64 = AtomicU64::new(0);
static VARDIFF_INTERVAL_COUNT: AtomicU64 = AtomicU64::new(0);
static JOBS_BROADCAST: AtomicU64 = AtomicU64::new(0);
static JOB_REFRESH_EVENTS: AtomicU64 = AtomicU64::new(0);
static NEW_BLOCK_EVENTS: AtomicU64 = AtomicU64::new(0);
static LAST_SHARE_ACCEPTED_AT: AtomicU64 = AtomicU64::new(0);
static LAST_SHARE_REJECTED_AT: AtomicU64 = AtomicU64::new(0);
static CANDIDATES_DETECTED: AtomicU64 = AtomicU64::new(0);
static CANDIDATES_SUBMITTED: AtomicU64 = AtomicU64::new(0);
static BLOCKS_ACCEPTED: AtomicU64 = AtomicU64::new(0);
static BLOCKS_REJECTED_STALE: AtomicU64 = AtomicU64::new(0);
static BLOCKS_REJECTED_INVALID: AtomicU64 = AtomicU64::new(0);
static BLOCKS_REJECTED_DUPLICATE: AtomicU64 = AtomicU64::new(0);
static BLOCKS_REJECTED_OTHER: AtomicU64 = AtomicU64::new(0);
static RPC_ERRORS: AtomicU64 = AtomicU64::new(0);
static HEADER_HASH_CHECKS: AtomicU64 = AtomicU64::new(0);
static CANDIDATE_HASHES: AtomicU64 = AtomicU64::new(0);
static SIMULATED_SUBMITS: AtomicU64 = AtomicU64::new(0);
static REJECTED_STALE: AtomicU64 = AtomicU64::new(0);
static REJECTED_LOW_DIFFICULTY: AtomicU64 = AtomicU64::new(0);
static REJECTED_INVALID: AtomicU64 = AtomicU64::new(0);
static REJECTED_DUPLICATE: AtomicU64 = AtomicU64::new(0);
static RPC_SUBMIT_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static RPC_SUBMIT_SUCCESS: AtomicU64 = AtomicU64::new(0);
static RPC_SUBMIT_FAIL: AtomicU64 = AtomicU64::new(0);

fn mark_accepted_share() {
    ACCEPTED_SHARES.fetch_add(1, Ordering::SeqCst);
    LAST_SHARE_ACCEPTED_AT.store(unix_now_secs(), Ordering::SeqCst);
}

fn mark_rejected_share() {
    REJECTED_SHARES.fetch_add(1, Ordering::SeqCst);
    LAST_SHARE_REJECTED_AT.store(unix_now_secs(), Ordering::SeqCst);
}

fn mark_rejected_stale() { mark_rejected_share(); REJECTED_STALE.fetch_add(1, Ordering::SeqCst); }
fn mark_rejected_low_difficulty() { mark_rejected_share(); REJECTED_LOW_DIFFICULTY.fetch_add(1, Ordering::SeqCst); }
fn mark_rejected_invalid() { mark_rejected_share(); REJECTED_INVALID.fetch_add(1, Ordering::SeqCst); }
fn mark_rejected_duplicate() { mark_rejected_share(); REJECTED_DUPLICATE.fetch_add(1, Ordering::SeqCst); }

fn mark_canonical_share() {
    CANONICAL_SHARES.fetch_add(1, Ordering::SeqCst);
    mark_accepted_share();
}

fn mark_compat_share() {
    COMPAT_SHARES.fetch_add(1, Ordering::SeqCst);
}

fn worker_diff_map() -> &'static Mutex<HashMap<String, f64>> {
    static MAP: OnceLock<Mutex<HashMap<String, f64>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn record_worker_diff(worker: &str, diff: f64) {
    if let Ok(mut m) = worker_diff_map().lock() {
        m.insert(worker.to_string(), diff);
    }
}

fn remove_worker_diff(worker: &str) {
    if let Ok(mut m) = worker_diff_map().lock() {
        m.remove(worker);
    }
}

fn avg_share_time_seconds() -> f64 {
    let c = VARDIFF_INTERVAL_COUNT.load(Ordering::SeqCst);
    if c == 0 {
        return 0.0;
    }
    let sum = VARDIFF_INTERVAL_SUM_MS.load(Ordering::SeqCst);
    (sum as f64 / c as f64) / 1000.0
}


#[derive(Debug, Clone)]
struct NodeTip {
    height: u64,
    best_hash: String,
}

#[derive(Debug, Clone)]
enum SubmitClass {
    Accepted,
    Stale,
    Invalid,
    Duplicate,
    RpcError,
    Other,
}

fn classify_submit_reason(msg: &str) -> SubmitClass {
    let m = msg.to_ascii_lowercase();
    if m.contains("stale") || m.contains("prev_hash") || m.contains("height mismatch") {
        return SubmitClass::Stale;
    }
    if m.contains("duplicate") || m.contains("already known") {
        return SubmitClass::Duplicate;
    }
    if m.contains("invalid") || m.contains("bad request") || m.contains("checksum") || m.contains("decode") {
        return SubmitClass::Invalid;
    }
    if m.contains("timeout") || m.contains("connection") || m.contains("tls") || m.contains("rpc") {
        return SubmitClass::RpcError;
    }
    SubmitClass::Other
}

async fn fetch_node_tip(config: &StratumConfig) -> Result<NodeTip> {
    let url = format!("{}/status", config.rpc_base.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(4))
        .build()?;

    let resp = client
        .get(url)
        .bearer_auth(&config.rpc_token)
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(anyhow!("status fetch failed: {}", resp.status()));
    }
    let v: Value = resp.json().await?;
    let height = v.get("height").and_then(|x| x.as_u64()).unwrap_or(0);
    let best = v
        .get("best_header_tip")
        .and_then(|x| x.get("hash"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    Ok(NodeTip { height, best_hash: best })
}

async fn submit_block_with_classification(config: &StratumConfig, req: &SubmitRequest) -> Result<(SubmitClass, String)> {
    let url = format!("{}/rpc/submit_block", config.rpc_base.trim_end_matches('/'));
    let payload = serde_json::to_vec(req)?;
    RPC_SUBMIT_ATTEMPTS.fetch_add(1, Ordering::SeqCst);
    info!(
        "[block-submit] attempt method=submit_block payload_bytes={} timeout_s=5",
        payload.len()
    );

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(5))
        .build()?;

    let mut attempts = 0u8;
    let resp = loop {
        attempts += 1;
        let send = client
            .post(&url)
            .bearer_auth(&config.rpc_token)
            .header("Content-Type", "application/json")
            .body(payload.clone())
            .send()
            .await;

        match send {
            Ok(r) => break r,
            Err(e) => {
                if attempts < 2 {
                    warn!("[block-submit] rpc error attempt={} err={}; retrying", attempts, e);
                    sleep(Duration::from_millis(300)).await;
                    continue;
                }
                RPC_SUBMIT_FAIL.fetch_add(1, Ordering::SeqCst);
                return Ok((SubmitClass::RpcError, format!("request_error: {e}")));
            }
        }
    };

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let body_short: String = body.chars().take(280).collect();

    if status.is_success() {
        if body_short.to_ascii_lowercase().contains(r#""accepted":true"#) || body_short.is_empty() {
            RPC_SUBMIT_SUCCESS.fetch_add(1, Ordering::SeqCst);
            return Ok((SubmitClass::Accepted, format!("http={} body={}", status, body_short)));
        }
        let cls = classify_submit_reason(&body_short);
        RPC_SUBMIT_FAIL.fetch_add(1, Ordering::SeqCst);
        return Ok((cls, format!("http={} body={}", status, body_short)));
    }

    let cls = classify_submit_reason(&format!("{} {}", status, body_short));
    RPC_SUBMIT_FAIL.fetch_add(1, Ordering::SeqCst);
    Ok((cls, format!("http={} body={}", status, body_short)))
}


async fn native_rpc_loop(bind: String, config: StratumConfig) -> Result<()> {
    let listener = TcpListener::bind(&bind).await?;
    info!("[native-rpc] listening on http://{bind}");

    loop {
        let (mut stream, addr) = listener.accept().await?;
        let cfg = config.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 65536];
            let n = match stream.read(&mut buf).await {
                Ok(n) => n,
                Err(e) => {
                    debug!("[native-rpc] read failed from {addr}: {e}");
                    return;
                }
            };
            if n == 0 {
                return;
            }

            let req = String::from_utf8_lossy(&buf[..n]);
            let first = req.lines().next().unwrap_or("");
            let body_off = buf[..n]
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|i| i + 4)
                .unwrap_or(n);
            let body_bytes = buf[body_off..n].to_vec();

            let (status, body) = if first.starts_with("GET /status") {
                forward_native_rpc(&cfg, "GET", "/status", None).await
            } else if first.starts_with("GET /rpc/getblocktemplate") {
                forward_native_rpc(&cfg, "GET", "/rpc/getblocktemplate", None).await
            } else if first.starts_with("POST /rpc/submit_block") {
                forward_native_rpc(&cfg, "POST", "/rpc/submit_block", Some(body_bytes)).await
            } else {
                ("404 Not Found".to_string(), "{\"error\":\"not_found\"}".to_string())
            };

            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes()).await;
        });
    }
}

async fn forward_native_rpc(config: &StratumConfig, method: &str, path: &str, body: Option<Vec<u8>>) -> (String, String) {
    let url = format!("{}/{}", config.rpc_base.trim_end_matches('/'), path.trim_start_matches('/'));
    let client = match reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return (
                "500 Internal Server Error".to_string(),
                json!({"error": format!("client_build_failed:{e}")}).to_string(),
            )
        }
    };

    let mut req = match method {
        "POST" => client.post(url),
        _ => client.get(url),
    };

    if !config.rpc_token.is_empty() {
        req = req.bearer_auth(&config.rpc_token);
    }

    if let Some(b) = body {
        req = req.header("Content-Type", "application/json").body(b);
    }

    match req.send().await {
        Ok(resp) => {
            let st = resp.status();
            let line = format!("{} {}", st.as_u16(), st.canonical_reason().unwrap_or("Unknown"));
            let body = resp.text().await.unwrap_or_default();
            (line, body)
        }
        Err(e) => (
            "502 Bad Gateway".to_string(),
            json!({"error": format!("upstream_request_failed:{e}")}).to_string(),
        ),
    }
}

async fn metrics_loop(bind: String, template_state: Arc<RwLock<TemplateState>>, max_template_age_seconds: u64) -> Result<()> {
    let listener = TcpListener::bind(&bind).await?;
    info!("[metrics] listening on http://{bind}/metrics");

    loop {
        let (mut stream, addr) = listener.accept().await?;
        let template_state = Arc::clone(&template_state);
        tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            let n = match stream.read(&mut buf).await {
                Ok(n) => n,
                Err(e) => {
                    debug!("[metrics] read failed from {addr}: {e}");
                    return;
                }
            };
            if n == 0 {
                return;
            }
            let req = String::from_utf8_lossy(&buf[..n]);
            let first = req.lines().next().unwrap_or("");

            let (status, body) = if first.starts_with("GET /metrics") {
                (
                    "200 OK",
                    json!({
                        "active_tcp_sessions": ACTIVE_SESSIONS.load(Ordering::SeqCst),
                        "accepted_shares": ACCEPTED_SHARES.load(Ordering::SeqCst),
                        "rejected_shares": REJECTED_SHARES.load(Ordering::SeqCst),
                        "rejected_stale": REJECTED_STALE.load(Ordering::SeqCst),
                        "rejected_low_difficulty": REJECTED_LOW_DIFFICULTY.load(Ordering::SeqCst),
                        "rejected_invalid": REJECTED_INVALID.load(Ordering::SeqCst),
                        "rejected_duplicate": REJECTED_DUPLICATE.load(Ordering::SeqCst),
                        "canonical_shares": CANONICAL_SHARES.load(Ordering::SeqCst),
                        "compat_shares": COMPAT_SHARES.load(Ordering::SeqCst),
                        "jobs_rotated": JOBS_ROTATED.load(Ordering::SeqCst),
                        "extranonce_rolls": EXTRANONCE_ROLLS.load(Ordering::SeqCst),
                        "vardiff_adjustments": VARDIFF_ADJUSTMENTS.load(Ordering::SeqCst),
                        "avg_share_time": avg_share_time_seconds(),
                        "worker_diff": worker_diff_map().lock().map(|m| m.clone()).unwrap_or_default(),
                        "jobs_broadcast": JOBS_BROADCAST.load(Ordering::SeqCst),
                        "job_refresh_events": JOB_REFRESH_EVENTS.load(Ordering::SeqCst),
                        "new_block_events": NEW_BLOCK_EVENTS.load(Ordering::SeqCst),
                        "last_share_accepted_at": LAST_SHARE_ACCEPTED_AT.load(Ordering::SeqCst),
                        "last_share_rejected_at": LAST_SHARE_REJECTED_AT.load(Ordering::SeqCst),
                        "candidates_detected": CANDIDATES_DETECTED.load(Ordering::SeqCst),
                        "candidates_submitted": CANDIDATES_SUBMITTED.load(Ordering::SeqCst),
                        "blocks_accepted": BLOCKS_ACCEPTED.load(Ordering::SeqCst),
                        "blocks_rejected_stale": BLOCKS_REJECTED_STALE.load(Ordering::SeqCst),
                        "blocks_rejected_invalid": BLOCKS_REJECTED_INVALID.load(Ordering::SeqCst),
                        "blocks_rejected_duplicate": BLOCKS_REJECTED_DUPLICATE.load(Ordering::SeqCst),
                        "blocks_rejected_other": BLOCKS_REJECTED_OTHER.load(Ordering::SeqCst),
                        "rpc_errors": RPC_ERRORS.load(Ordering::SeqCst),
                        "header_hash_checks": HEADER_HASH_CHECKS.load(Ordering::SeqCst),
                        "candidate_hashes": CANDIDATE_HASHES.load(Ordering::SeqCst),
                        "simulated_submits": SIMULATED_SUBMITS.load(Ordering::SeqCst),
                        "rpc_submit_attempts": RPC_SUBMIT_ATTEMPTS.load(Ordering::SeqCst),
                        "rpc_submit_success": RPC_SUBMIT_SUCCESS.load(Ordering::SeqCst),
                        "rpc_submit_fail": RPC_SUBMIT_FAIL.load(Ordering::SeqCst)
                    })
                    .to_string(),
                )
            } else if first.starts_with("GET /health") {
                let now = unix_now_secs();
                let st = template_state.read().await;
                let age = now.saturating_sub(st.last_update_unix);
                let stale = st.last_update_unix == 0 || age > max_template_age_seconds;
                if stale {
                    error!(
                        "[health] template stale height={} age_seconds={} max_age_seconds={} prev={}",
                        st.last_height, age, max_template_age_seconds, st.last_prevhash
                    );
                }
                (
                    "200 OK",
                    json!({
                        "status": if stale { "stale" } else { "ok" },
                        "height": st.last_height,
                        "age_seconds": age,
                        "prevhash": st.last_prevhash,
                    })
                    .to_string(),
                )
            } else {
                ("404 Not Found", "{\"error\":\"not_found\"}".to_string())
            };

            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes()).await;
        });
    }
}

#[derive(serde::Serialize)]
struct SubmitHeader {
    version: u32,
    prev_hash: String,
    merkle_root: String,
    time: u32,
    bits: String,
    nonce: u32,
    hash: String,
}

#[derive(serde::Serialize)]
struct SubmitRequest {
    height: u64,
    header: SubmitHeader,
    tx_hex: Vec<String>,
}

pub async fn run(config: StratumConfig) -> Result<()> {
    let listener = TcpListener::bind(&config.bind).await?;
    info!(
        "[stratum] listening on {} hash_cmp_mode={} miner_family_mode={} sharecheck_samples={} vardiff={} min={} max={} target_s={} retarget_s={} coinbase_bip34={} keepalive_notify_secs={}",
        config.bind,
        config.hash_cmp_mode.as_str(),
        config.miner_family_mode.as_str(),
        config.sharecheck_samples,
        config.vardiff_enabled,
        config.vardiff_min_diff,
        config.vardiff_max_diff,
        config.vardiff_target_share_secs,
        config.vardiff_retarget_secs,
        config.coinbase_bip34,
        config.keepalive_notify_secs,
    );

    let (tx, _) = broadcast::channel::<Job>(256);
    let current = Arc::new(RwLock::new(None::<Job>));
    let template_state = Arc::new(RwLock::new(TemplateState::default()));

    if let Some(bind) = config.metrics_bind.clone() {
        let health_state = Arc::clone(&template_state);
        let max_age = config.max_template_age_seconds;
        tokio::spawn(async move {
            if let Err(e) = metrics_loop(bind, health_state, max_age).await {
                error!("[metrics] loop stopped: {e}");
            }
        });
    }

    if let Some(bind) = config.native_bind.clone() {
        let cfg = config.clone();
        tokio::spawn(async move {
            if let Err(e) = native_rpc_loop(bind, cfg).await {
                error!("[native-rpc] loop stopped: {e}");
            }
        });
    }

    let cfg_clone = config.clone();
    let tx_clone = tx.clone();
    let current_clone = Arc::clone(&current);
    let template_state_clone = Arc::clone(&template_state);
    tokio::spawn(async move {
        if let Err(e) = template_loop(cfg_clone, tx_clone, current_clone, template_state_clone).await {
            error!("[tmpl] loop stopped: {e}");
        }
    });

    let conn_id = Arc::new(AtomicU64::new(1));
    loop {
        let (stream, addr) = listener.accept().await?;
        let id = conn_id.fetch_add(1, Ordering::SeqCst);
        info!("[conn] accepted id={} from {}", id, addr);

        let mut rx = tx.subscribe();
        let current = Arc::clone(&current);
        let cfg = config.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(id, stream, cfg, &mut rx, current).await {
                warn!("[conn] id={} ended: {}", id, e);
            }
        });
    }
}

async fn template_loop(
    config: StratumConfig,
    tx: broadcast::Sender<Job>,
    current: Arc<RwLock<Option<Job>>>,
    template_state: Arc<RwLock<TemplateState>>,
) -> Result<()> {
    let client = TemplateClient::new(config.rpc_base.clone(), config.rpc_token.clone())?;
    let mut last_key = String::new();
    let mut seq: u64 = 1;
    let mut roll_nonce: u32 = 1;
    let mut last_rotate_ts = unix_now_secs();

    loop {

        match client.fetch_template().await {
            Ok(tpl) => {
                let prev_hash = parse_hex32(&tpl.prev_hash)?;
                let prevhash = hex::encode(prev_hash);
                let now_ts = unix_now_secs();
                {
                    let mut st = template_state.write().await;
                    st.last_height = tpl.height;
                    st.last_prevhash = prevhash.clone();
                    st.last_update_unix = now_ts;
                }

                let key = format!("{}:{}", tpl.height, prevhash);
                let mut rotate_reason = "";
                let should_emit = if key != last_key {
                    last_key = key;
                    last_rotate_ts = now_ts;
                    rotate_reason = "new_template";
                    true
                } else if now_ts.saturating_sub(last_rotate_ts) >= config.job_rotate_secs {
                    last_rotate_ts = now_ts;
                    rotate_reason = "periodic";
                    true
                } else {
                    false
                };

                if should_emit {
                    seq = seq.wrapping_add(1);
                    roll_nonce = roll_nonce.wrapping_add(1);
                    EXTRANONCE_ROLLS.fetch_add(1, Ordering::SeqCst);
                    let clean_jobs = rotate_reason != "periodic";
                    let job = to_job(seq, &tpl, roll_nonce, clean_jobs)?;
                    info!("[tmpl] template height={} prevhash={} bits={} target={} ntime={} coinbase_value={} txs={} ts={}", job.height, prevhash, job.nbits_hex, biguint_to_32hex(&target_from_bits(job.bits)), job.ntime_hex, job.coinbase_value, job.tx_hex.len(), now_ts);

                    {
                        let mut w = current.write().await;
                        *w = Some(job.clone());
                    }
                    let _ = tx.send(job.clone());
                    JOBS_BROADCAST.fetch_add(1, Ordering::SeqCst);

                    if rotate_reason == "periodic" {
                        JOBS_ROTATED.fetch_add(1, Ordering::SeqCst);
                        JOB_REFRESH_EVENTS.fetch_add(1, Ordering::SeqCst);
                        info!("[job] periodic_refresh job_id={} height={} roll_nonce={} prev={}", job.job_id, job.height, job.roll_nonce, hex::encode(job.prev_hash));
                    } else {
                        NEW_BLOCK_EVENTS.fetch_add(1, Ordering::SeqCst);
                        info!(
                            "[job] new_block_template height={} job_id={} block_target={} bits={} prev={} roll_nonce={}",
                            job.height,
                            job.job_id,
                            biguint_to_32hex(&target_from_bits(job.bits)),
                            job.nbits_hex,
                            hex::encode(job.prev_hash),
                            job.roll_nonce
                        );
                    }
                    info!(
                        "[tmpl] new job id={} height={} txs={} target={}",
                        job.job_id,
                        job.height,
                        job.tx_hex.len(),
                        job.template_target_hex
                    );
                }
            }
            Err(e) => warn!("[tmpl] fetch failed: {e}"),
        }
        sleep(Duration::from_millis(config.refresh_ms)).await;
    }
}

fn to_job(seq: u64, tpl: &GetBlockTemplate, roll_nonce: u32, clean_jobs: bool) -> Result<Job> {
    let prev_hash = parse_hex32(&tpl.prev_hash)?;
    let bits = parse_u32_hex(&tpl.bits)?;
    let ntime_hex = format!("{:08x}", tpl.time);
    let tx_hex: Vec<String> = tpl.txs.iter().map(|t| t.hex.clone()).collect();
    let branches = build_merkle_branches(&tx_hex)?;

    Ok(Job {
        job_id: format!("{seq:016x}"),
        height: tpl.height,
        prev_hash,
        bits,
        nbits_hex: tpl.bits.clone(),
        ntime_hex,
        coinbase_value: tpl.coinbase_value,
        tx_hex,
        branches,
        template_target_hex: tpl.target.clone(),
        roll_nonce,
        clean_jobs,
    })
}

async fn handle_conn(
    id: u64,
    stream: TcpStream,
    config: StratumConfig,
    rx: &mut broadcast::Receiver<Job>,
    current: Arc<RwLock<Option<Job>>>,
) -> Result<()> {
    ACTIVE_SESSIONS.fetch_add(1, Ordering::SeqCst);

    let extranonce1 = id.to_be_bytes()[8 - config.extranonce1_size..].to_vec();
    let mut session = SessionState {
        extranonce1,
        worker: None,
        pkh: None,
        difficulty: config.default_diff,
        miner_class: MinerClass::Unknown,
        current_job: None,
        last_share_ts: None,
        last_retarget_ts: unix_now_secs(),
        coinbase_bip34: config.coinbase_bip34,
        last_notify_coinbase1: Vec::new(),
        last_notify_coinbase2: Vec::new(),
        seen_submissions: HashSet::new(),
    };

    let (rd, mut wr) = stream.into_split();
    let mut lines = BufReader::new(rd).lines();
    let mut keepalive_tick = tokio::time::interval(Duration::from_secs(config.keepalive_notify_secs.max(5)));
    keepalive_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let result = loop {
        tokio::select! {
            _ = keepalive_tick.tick() => {
                if session.pkh.is_some() {
                    if let Some(j) = session.current_job.clone() {
                        if let Err(e) = send_notify(&mut wr, &mut session, &j).await { break Err(e); }
                        debug!("[keepalive] conn={} worker={} job={}", id, session.worker.as_deref().unwrap_or("-"), j.job_id);
                    }
                }
            }
            job = rx.recv() => {
                if let Ok(j) = job {
                    if session.pkh.is_some() {
                        if let Err(e) = send_set_difficulty(&mut wr, id, session.worker.as_deref(), session.difficulty).await { break Err(e); }
                        if let Err(e) = send_notify(&mut wr, &mut session, &j).await { break Err(e); }
                    }
                    let changed = session.current_job.as_ref().map(|cj| cj.job_id.as_str()) != Some(j.job_id.as_str());
                    if changed { session.seen_submissions.clear(); }
                    session.current_job = Some(j);
                }
            }
            line = lines.next_line() => {
                let Some(line) = line? else { break Err(anyhow!("EOF")); };
                let v: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(e) => {
                        warn!("[conn] id={} bad json: {e}", id);
                        continue;
                    }
                };
                if let Err(e) = handle_message(id, &mut wr, &mut session, &config, &current, v).await { break Err(e); }
            }
        }
    };

    if let Some(w) = session.worker.as_ref() {
        remove_worker_diff(w);
    }
    ACTIVE_SESSIONS.fetch_sub(1, Ordering::SeqCst);
    result
}

async fn handle_message(
    conn_id: u64,
    wr: &mut tokio::net::tcp::OwnedWriteHalf,
    session: &mut SessionState,
    config: &StratumConfig,
    current: &Arc<RwLock<Option<Job>>>,
    msg: Value,
) -> Result<()> {
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = msg
        .get("params")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();

    match method {
        "mining.subscribe" => {
            let agent = params.first().and_then(|v| v.as_str()).unwrap_or("-");
            session.miner_class = classify_miner_agent(agent);
            let (class_min, class_max) = miner_diff_bounds(&session.miner_class);
            let min_d = class_min.max(config.vardiff_min_diff);
            let max_d = class_max.min(config.vardiff_max_diff).max(min_d);
            session.difficulty = min_d;

            let resp = json!({
                "id": id,
                "result": [
                    [["mining.set_difficulty","irium"],["mining.notify","irium"]],
                    hex::encode(&session.extranonce1),
                    config.extranonce1_size
                ],
                "error": null
            });
            write_json(wr, &resp).await?;
            info!("[vardiff] conn={} agent='{}' class={} initial_diff={} min={} max={}", conn_id, agent, session.miner_class.as_str(), session.difficulty, min_d, max_d);
            send_set_difficulty(wr, conn_id, session.worker.as_deref(), session.difficulty).await?;
        }
        "mining.authorize" => {
            let user = params.first().and_then(|v| v.as_str()).unwrap_or("");
            let addr = user.split('.').next().unwrap_or("").trim();
            match parse_address_to_pkh(addr) {
                Ok(pkh) => {
                    session.worker = Some(user.to_string());
                    session.seen_submissions.clear();
                    session.pkh = Some(pkh);
                    record_worker_diff(user, session.difficulty);
                    let resp = json!({"id": id, "result": true, "error": null});
                    write_json(wr, &resp).await?;

                    let cur = current.read().await;
                    if let Some(job) = cur.clone() {
                        session.current_job = Some(job.clone());
                        send_set_difficulty(wr, conn_id, session.worker.as_deref(), session.difficulty).await?;
                        send_notify(wr, session, &job).await?;
                    }
                }
                Err(e) => {
                    let resp = json!({"id": id, "result": false, "error": [20, format!("invalid address: {e}"), null]});
                    write_json(wr, &resp).await?;
                }
            }
        }
        "mining.submit" => {
            let result = handle_submit(conn_id, wr, session, config, &params).await;
            match result {
                Ok(accepted) => {
                    let resp = json!({"id": id, "result": accepted, "error": null});
                    write_json(wr, &resp).await?;
                }
                Err(e) => {
                    let em = e.to_string();
                    let (code, detail) = if let Some((c, d)) = em.split_once(":") {
                        (c.to_string(), d.to_string())
                    } else {
                        ("invalid".to_string(), em)
                    };
                    let stable = match code.as_str() {
                        "stale_share" | "low_difficulty" | "invalid" | "duplicate" => code,
                        _ => "invalid".to_string(),
                    };
                    let resp = json!({"id": id, "result": false, "error": [23, stable, detail]});
                    write_json(wr, &resp).await?;
                }
            }
        }
        _ => {
            let resp = json!({"id": id, "result": null, "error": [20, "unsupported method", null]});
            write_json(wr, &resp).await?;
        }
    }
    Ok(())
}

fn mr_is_canon(name: &str) -> bool {
    name.ends_with("_canon")
}

fn mr_is_rev32(name: &str) -> bool {
    name.ends_with("_rev32")
}

fn mode_allows_combo(
    mode: &MinerFamilyMode,
    prev_name: &str,
    mr_name: &str,
    v_name: &str,
    t_name: &str,
    b_name: &str,
    n_name: &str,
) -> bool {
    let mr_canon = mr_is_canon(mr_name);
    let mr_rev32 = mr_is_rev32(mr_name);

    match mode {
        MinerFamilyMode::Asic => {
            // Legacy combo (pre-Fix-2a, no Bitcoin-standard variant names):
            let legacy = prev_name == "prev_canon"
                && mr_name == "mr_canon"
                && v_name == "v_be"
                && t_name == "t_raw"
                && b_name == "b_raw"
                && n_name == "n_raw";
            // Bitcoin-standard ASIC combo. mining.notify now sends
            // reverse_32(display) = sha256d_native, so raw-write ASICs place
            // exactly that in header[4..36] = the "prev_rev32" variant
            // (reverse_32 of job.prev_hash which is display order). This is
            // the ONLY variant compatible with iriumd's reverse_32 serialize
            // for prev_hash. Firmwares that apply their own swap4 will land
            // on prev_swap4 or prev_rev32_swap4 and fall through here ->
            // share rejected as non-standard, which is correct.
            let modern = prev_name == "prev_rev32"
                && matches!(mr_name, "mr_fold_raw_raw" | "mr_fold_raw_raw:swap4")
                && matches!(v_name, "v_be" | "v_le" | "v_rolled")
                && matches!(t_name, "t_raw" | "t_rev")
                && matches!(b_name, "b_raw" | "b_rev")
                && matches!(n_name, "n_raw" | "n_rev");
            legacy || modern
        }
        MinerFamilyMode::Ccminer => {
            (prev_name == "prev_canon" || prev_name == "prev_rev32")
                && (mr_canon || mr_rev32)
                && v_name == "v_be"
                && t_name == "t_raw"
                && b_name == "b_raw"
                && (n_name == "n_raw" || n_name == "n_rev")
        }
        MinerFamilyMode::Auto => {
            // canonical first, then legacy cpuminer fallback
            ((prev_name == "prev_canon")
                && mr_canon
                && v_name == "v_be"
                && t_name == "t_raw"
                && b_name == "b_raw"
                && n_name == "n_raw")
                || ((prev_name == "prev_canon")
                    && mr_canon
                    && v_name == "v_be"
                    && t_name == "t_raw"
                    && b_name == "b_raw"
                    && n_name == "n_rev")
                || ((prev_name == "prev_swap4")
                    && mr_name == "mr_wire_x2_raw_canon"
                    && v_name == "v_le"
                    && t_name == "t_rev"
                    && b_name == "b_rev"
                    && n_name == "n_rev")
        }
        MinerFamilyMode::Cpuminer => {
            prev_name == "prev_swap4"
                && mr_name == "mr_wire_x2_raw_canon"
                && v_name == "v_le"
                && t_name == "t_rev"
                && b_name == "b_rev"
                && n_name == "n_rev"
        }
    }
}

async fn handle_submit(
    conn_id: u64,
    wr: &mut tokio::net::tcp::OwnedWriteHalf,
    session: &mut SessionState,
    config: &StratumConfig,
    params: &[Value],
) -> Result<bool> {
    let worker = session
        .worker
        .clone()
        .unwrap_or_else(|| format!("conn-{conn_id}"));
    let pkh = match session.pkh {
        Some(v) => v,
        None => {
            mark_rejected_invalid();
            warn!("[share] reject reason=invalid job_id=- worker={} detail=unauthorized", worker);
            return Err(anyhow!("invalid:unauthorized"));
        }
    };
    let job = match session.current_job.clone() {
        Some(j) => j,
        None => {
            mark_rejected_stale();
            warn!("[share] reject reason=stale job_id=- worker={} detail=no_active_job", worker);
            return Err(anyhow!("stale_share:no_active_job"));
        }
    };

    if params.len() < 5 {
        mark_rejected_invalid();
        warn!("[share] reject reason=invalid job_id={} worker={} detail=invalid_params", job.job_id, worker);
        return Err(anyhow!("invalid:invalid_params"));
    }
    let job_id = params[1].as_str().unwrap_or("");
    let extranonce2_hex = params[2].as_str().unwrap_or("");
    let ntime_hex = params[3].as_str().unwrap_or("");
    let nonce_hex = params[4].as_str().unwrap_or("");
    // BIP310 / Antminer-style version-rolling: ASIC firmware sends the
    // rolled version as a 6th hex param on mining.submit. We trust it
    // as-is (no version_mask validation — same approach as mainstream
    // pools) and feed it into the share-check loop as a "v_rolled"
    // candidate. None means the ASIC isn't rolling, so we stay with the
    // base v_be/v_le candidates only.
    let rolled_version: Option<u32> = params.get(5)
        .and_then(|v| v.as_str())
        .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok());

    if job_id != job.job_id {
        mark_rejected_stale();
        warn!("[share] reject reason=stale job_id={} worker={} detail=job_mismatch", job_id, worker);
        return Err(anyhow!("stale_share:job_mismatch"));
    }

    let submit_key = format!("{}:{}:{}:{}", job_id, extranonce2_hex, ntime_hex, nonce_hex);
    if !session.seen_submissions.insert(submit_key) {
        mark_rejected_duplicate();
        warn!("[share] reject reason=duplicate job_id={} worker={}", job_id, worker);
        return Err(anyhow!("duplicate:duplicate_share"));
    }

    let extra2_raw = match hex::decode(extranonce2_hex) {
        Ok(v) => v,
        Err(_) => {
            mark_rejected_invalid();
            warn!("[share] reject reason=invalid job_id={} worker={} detail=bad_extranonce2", job.job_id, worker);
            return Err(anyhow!("invalid:bad_extranonce2"));
        }
    };
    let mut en_submit = session.extranonce1.clone();
    en_submit.extend_from_slice(&extra2_raw);
    let cb = build_coinbase_tx_with_roll(job.height, job.coinbase_value, &pkh, &en_submit, config.coinbase_bip34, job.roll_nonce);
    let cb_submit = if !session.last_notify_coinbase1.is_empty() || !session.last_notify_coinbase2.is_empty() {
        let mut cb_wire = Vec::with_capacity(session.last_notify_coinbase1.len() + session.extranonce1.len() + extra2_raw.len() + session.last_notify_coinbase2.len());
        cb_wire.extend_from_slice(&session.last_notify_coinbase1);
        cb_wire.extend_from_slice(&session.extranonce1);
        cb_wire.extend_from_slice(&extra2_raw);
        cb_wire.extend_from_slice(&session.last_notify_coinbase2);
        cb_wire
    } else {
        cb.clone()
    };
    let mut extra2_rev = extra2_raw.clone();
    extra2_rev.reverse();

    let mut extra2_candidates: Vec<(&str, Vec<u8>)> = vec![("x2_raw", extra2_raw.clone())];
    if extra2_rev != extra2_raw {
        extra2_candidates.push(("x2_rev", extra2_rev));
    }

    let _ntime = match parse_u32_hex(ntime_hex) {
        Ok(v) => v,
        Err(_) => {
            mark_rejected_invalid();
            warn!("[share] reject reason=invalid job_id={} worker={} detail=bad_ntime", job.job_id, worker);
            return Err(anyhow!("invalid:bad_ntime"));
        }
    };
    let nonce = match parse_u32_hex(nonce_hex) {
        Ok(v) => v,
        Err(_) => {
            mark_rejected_invalid();
            warn!("[share] reject reason=invalid job_id={} worker={} detail=bad_nonce", job.job_id, worker);
            return Err(anyhow!("invalid:bad_nonce"));
        }
    };
    let version: u32 = 1;

    let prev_canon = job.prev_hash;
    let prev_rev32 = reverse_32(job.prev_hash);
    let prev_swap4 = swap4_bytes_each_word(job.prev_hash);
    let prev_rev32_swap4 = swap4_bytes_each_word(prev_rev32);

    let prev_variants: [(&str, [u8; 32]); 4] = [
        ("prev_canon", prev_canon),
        ("prev_rev32", prev_rev32),
        ("prev_swap4", prev_swap4),
        ("prev_rev32_swap4", prev_rev32_swap4),
    ];

    let mut merkle_variants: Vec<(String, [u8; 32])> = Vec::new();
    for (x2_name, x2_bytes) in &extra2_candidates {
        let mut en = session.extranonce1.clone();
        en.extend_from_slice(x2_bytes);

        let cb_built = build_coinbase_tx_with_roll(job.height, job.coinbase_value, &pkh, &en, config.coinbase_bip34, job.roll_nonce);
        let cb_built_hash = sha256d(&cb_built);
        let mr_built = merkle_root_from_coinbase(cb_built_hash, &job.branches);
        let mut mr_built_rev = mr_built;
        mr_built_rev.reverse();
        merkle_variants.push((format!("mr_built_{}_canon", x2_name), mr_built));
        merkle_variants.push((format!("mr_built_{}_rev32", x2_name), mr_built_rev));

        if !session.last_notify_coinbase1.is_empty() || !session.last_notify_coinbase2.is_empty() {
            let mut cb_wire = Vec::with_capacity(
                session.last_notify_coinbase1.len() + session.extranonce1.len() + x2_bytes.len() + session.last_notify_coinbase2.len()
            );
            cb_wire.extend_from_slice(&session.last_notify_coinbase1);
            cb_wire.extend_from_slice(&session.extranonce1);
            cb_wire.extend_from_slice(x2_bytes);
            cb_wire.extend_from_slice(&session.last_notify_coinbase2);
            let cb_wire_hash = sha256d(&cb_wire);
            let mr_wire = merkle_root_from_coinbase(cb_wire_hash, &job.branches);
            let mut mr_wire_rev = mr_wire;
            mr_wire_rev.reverse();
            merkle_variants.push((format!("mr_wire_{}_canon", x2_name), mr_wire));
            merkle_variants.push((format!("mr_wire_{}_rev32", x2_name), mr_wire_rev));
        }
    }
    if merkle_variants.is_empty() {
        return Err(anyhow!("no merkle variants"));
    }
    let mr_log = merkle_variants[0].1;

    let share_target = target_from_difficulty_with_limit(session.difficulty, &config.pow_limit);
    let block_target = target_from_bits(job.bits);
    HEADER_HASH_CHECKS.fetch_add(1, Ordering::SeqCst);

    #[derive(Clone)]
    struct CheckResult {
        name: &'static str,
        header: [u8; 80],
        hash: [u8; 32],
        ok_share_be: bool,
        ok_share_le: bool,
        ok_block_be: bool,
        ok_block_le: bool,
    }

    let use_le = matches!(config.hash_cmp_mode, HashCmpMode::Le);

    let mut checks: Vec<CheckResult> = Vec::new();
    let mut accepted_idx: Option<usize> = None;

    let nbits_raw = decode_hex4(&job.nbits_hex)?;
    let ntime_raw = match decode_hex4(ntime_hex) {
        Ok(v) => v,
        Err(_) => {
            mark_rejected_invalid();
            warn!("[share] reject reason=invalid job_id={} worker={} detail=bad_ntime_hex", job.job_id, worker);
            return Err(anyhow!("invalid:bad_ntime_hex"));
        }
    };
    let nonce_raw = match decode_hex4(nonce_hex) {
        Ok(v) => v,
        Err(_) => {
            mark_rejected_invalid();
            warn!("[share] reject reason=invalid job_id={} worker={} detail=bad_nonce_hex", job.job_id, worker);
            return Err(anyhow!("invalid:bad_nonce_hex"));
        }
    };
    let version_be = [0x00, 0x00, 0x00, 0x01];
    let version_le = version.to_le_bytes();

    let mut version_opts: Vec<(&str, [u8; 4])> = vec![("v_be", version_be), ("v_le", version_le)];
    if let Some(v) = rolled_version {
        // BIP310: params[5] is JUST the rolled BITS (within the version-mask).
        // Reconstruct the full header version by OR-ing with the base version
        // from mining.notify. Without this OR, the header low byte ends in
        // 0x00 instead of base's 0x01, so the share never matches.
        version_opts.push(("v_rolled", (version | v).to_le_bytes()));
    }
    let time_opts = [("t_raw", ntime_raw), ("t_rev", reverse_4(ntime_raw))];
    let bits_opts = [("b_raw", nbits_raw), ("b_rev", reverse_4(nbits_raw))];
    let nonce_opts = [("n_raw", nonce_raw), ("n_rev", reverse_4(nonce_raw))];

    for (prev_name, prev_for_header) in prev_variants {
        for (mr_name, mr_for_header) in &merkle_variants {
            for (v_name, v_bytes) in version_opts {
                for (t_name, t_bytes) in time_opts {
                    for (b_name, b_bytes) in bits_opts {
                        for (n_name, n_bytes) in nonce_opts {
                            if !mode_allows_combo(&config.miner_family_mode, prev_name, mr_name.as_str(), v_name, t_name, b_name, n_name) {
                                continue;
                            }
                            let hdr_v = header_bytes_from_wire(
                                v_bytes,
                                prev_for_header,
                                *mr_for_header,
                                t_bytes,
                                b_bytes,
                                n_bytes,
                            );
                            let mode = format!("{}:{}:{}:{}", v_name, t_name, b_name, n_name);
            let hash_v = sha256d(&hdr_v);

            let hash_int_be = BigUint::from_bytes_be(&hash_v);
            let mut hash_rev = hash_v;
            hash_rev.reverse();
            let hash_int_le = BigUint::from_bytes_be(&hash_rev);

            let ok_share_be = hash_int_be <= share_target;
            let ok_share_le = hash_int_le <= share_target;
            let ok_block_be = hash_int_be <= block_target;
            let ok_block_le = hash_int_le <= block_target;

            let ok_share = if use_le { ok_share_le } else { ok_share_be };

                            checks.push(CheckResult {
                                name: Box::leak(format!("{}+{}:{}", prev_name, mr_name, mode).into_boxed_str()),
                                header: hdr_v,
                                hash: hash_v,
                                ok_share_be,
                                ok_share_le,
                                ok_block_be,
                                ok_block_le,
                            });

                            if ok_share && accepted_idx.is_none() {
                                accepted_idx = Some(checks.len() - 1);
                            }
                        }
                    }
                }
            }
        }
    }

    let canonical = &checks[0];
    debug!(
        "[sharedebug-header] worker={} job={} variant={} version_hex={:08x} prevhash={} merkle_root={} ntime_hex={} nbits_hex={} nonce_hex={:08x} header80={} hash={}",
        worker,
        job.job_id,
        canonical.name,
        version,
        hex::encode(job.prev_hash),
        hex::encode(mr_log),
        ntime_hex,
        job.nbits_hex,
        nonce,
        hex::encode(canonical.header),
        hex::encode(canonical.hash)
    );

    let total_variants = checks.len();
    let sample_n = config.sharecheck_samples.min(total_variants);
    let summary = checks
        .iter()
        .take(sample_n)
        .map(|c| {
            format!(
                "{}:sb={} sl={} bb={} bl={}",
                c.name,
                c.ok_share_be,
                c.ok_share_le,
                c.ok_block_be,
                c.ok_block_le
            )
        })
        .collect::<Vec<_>>()
        .join(" | ");

    let canonical_variant = "prev_canon+mr_built_x2_raw_canon:v_be:t_raw:b_raw:n_raw";
    let cpuminer_variant = "prev_swap4+mr_wire_x2_raw_canon:v_le:t_rev:b_rev:n_rev";

    let canonical_idx = checks.iter().position(|c| {
        c.name == canonical_variant && if use_le { c.ok_share_le } else { c.ok_share_be }
    });
    let _cpu_idx = if session.miner_class == MinerClass::Cpu {
        checks.iter().position(|c| {
            c.name == cpuminer_variant && if use_le { c.ok_share_le } else { c.ok_share_be }
        })
    } else {
        None
    };

    // Canonical enforcement: only canonical variant counts as accepted work.
    // We still keep compat detection for diagnostics, but compat shares are rejected.
    let selected_idx = canonical_idx.or(accepted_idx);
    let selected_ok_block = selected_idx
        .map(|idx| {
            let c = &checks[idx];
            if use_le { c.ok_block_le } else { c.ok_block_be }
        })
        .unwrap_or(false);

    let mut hash = canonical.hash;
    let selected_variant;
    let mut selected_header = canonical.header;

    let check_line = if let Some(idx) = selected_idx {
        let chosen = &checks[idx];
        hash = chosen.hash;
        selected_header = chosen.header;
        selected_variant = chosen.name;
        format!(
            "[sharecheck] worker={} job={} assigned_diff={} share_target={} block_target={} chosen_variant={} variants_checked={} sample={}",
            worker,
            job.job_id,
            session.difficulty,
            biguint_to_32hex(&share_target),
            biguint_to_32hex(&block_target),
            selected_variant,
            total_variants,
            summary
        )
    } else {
        selected_variant = "none";
        format!(
            "[sharecheck] worker={} job={} assigned_diff={} share_target={} block_target={} chosen_variant={} variants_checked={} sample={}",
            worker,
            job.job_id,
            session.difficulty,
            biguint_to_32hex(&share_target),
            biguint_to_32hex(&block_target),
            selected_variant,
            total_variants,
            summary
        )
    };

    let ok_share_any = accepted_idx.is_some();
    let mut ok_share_canonical = selected_idx.is_some() && selected_variant == canonical_variant;

    if ok_share_canonical {
        mark_canonical_share();
        debug!("{}", check_line);
        debug!("[share] accepted worker={} hash={}", worker, hex::encode(hash));
    } else if ok_share_any {
        if selected_variant == cpuminer_variant {
            mark_compat_share();
            mark_accepted_share();
            ok_share_canonical = true;
            info!("[share] accepted compat-cpuminer worker={} variant={}", worker, selected_variant);
        } else {
        let mut mapped_share_ok = false;
        let mut mapped_hash_hex = String::new();

        if selected_idx.is_some() {
            let version_submit = u32::from_le_bytes(selected_header[0..4].try_into().unwrap_or([0u8; 4]));
            let time_submit = u32::from_le_bytes(selected_header[68..72].try_into().unwrap_or([0u8; 4]));
            let bits_submit = u32::from_le_bytes(selected_header[72..76].try_into().unwrap_or([0u8; 4]));
            let nonce_submit = u32::from_le_bytes(selected_header[76..80].try_into().unwrap_or([0u8; 4]));

            let mut prev_sel = [0u8; 32];
            prev_sel.copy_from_slice(&selected_header[4..36]);
            let mut merkle_sel = [0u8; 32];
            merkle_sel.copy_from_slice(&selected_header[36..68]);

            let variant_head = selected_variant
                .split(':')
                .next()
                .unwrap_or("prev_canon+mr_built_x2_raw_canon");
            let (prev_tag, mr_tag) = variant_head
                .split_once('+')
                .unwrap_or(("prev_canon", "mr_built_x2_raw_canon"));

            let prev_canon = match prev_tag {
                "prev_canon" => prev_sel,
                "prev_rev32" => reverse_32(prev_sel),
                "prev_swap4" => swap4_bytes_each_word(prev_sel),
                "prev_rev32_swap4" => reverse_32(swap4_bytes_each_word(prev_sel)),
                _ => job.prev_hash,
            };

            let merkle_from_variant = if mr_tag.ends_with("_rev32") {
                reverse_32(merkle_sel)
            } else {
                merkle_sel
            };
            let merkle_expected = merkle_root_from_coinbase(sha256d(&cb_submit), &job.branches);
            let merkle_canon = if merkle_from_variant == merkle_expected {
                merkle_from_variant
            } else {
                merkle_expected
            };

            let mut prev_wire = prev_canon;
            prev_wire.reverse();
            let mut merkle_wire = merkle_canon;
            merkle_wire.reverse();

            let header_wire = header_bytes(
                version_submit,
                prev_wire,
                merkle_wire,
                time_submit,
                bits_submit,
                nonce_submit,
            );
            let submit_hash = sha256d(&header_wire);
            let submit_hash_int = BigUint::from_bytes_be(&submit_hash);
            mapped_hash_hex = hex::encode(submit_hash);

            if submit_hash_int <= share_target {
                mapped_share_ok = true;
                ok_share_canonical = true;
                hash = submit_hash;
            }
        }

        if mapped_share_ok {
            mark_canonical_share();
            info!("[share] accepted remapped-canonical worker={} chosen_variant={} mapped_hash={}", worker, selected_variant, mapped_hash_hex);
        } else {
            mark_compat_share();
            mark_rejected_invalid();
            warn!("{}", check_line);
            warn!("[sharecheck] compat-share rejected (non canonical header) worker={} chosen_variant={}", worker, selected_variant);
            warn!("[share] reject reason=invalid job_id={} worker={} detail=non_canonical_header", job.job_id, worker);
            return Err(anyhow!("invalid:non_canonical_header"));
        }
        }
    } else if config.soft_accept_invalid_shares {
        mark_rejected_invalid();
        warn!("{}", check_line);
        warn!("[share] soft-accepted worker={} reason=compat_soft_accept", worker);
        return Ok(true);
    } else {
        mark_rejected_low_difficulty();
        let mut h = canonical.hash;
        if use_le { h.reverse(); }
        let h_int = BigUint::from_bytes_be(&h);
        let share_diff = if h_int == BigUint::from(0u8) {
            "inf".to_string()
        } else {
            let scaled = (&config.pow_limit * BigUint::from(1000u32)) / h_int;
            let q = &scaled / BigUint::from(1000u32);
            let r = (&scaled % BigUint::from(1000u32)).to_str_radix(10);
            format!("{}.{}", q.to_str_radix(10), format!("{:0>3}", r))
        };
        warn!("{}", check_line);
        warn!("[share] reject reason=low_difficulty share_diff={} target={} job_id={} worker={}", share_diff, session.difficulty, job.job_id, worker);
        return Err(anyhow!("low_difficulty:below_assigned_target"));
    }

    if ok_share_canonical {

        let tip = fetch_node_tip(config).await.ok();
        if let Some(t) = &tip {
            if !t.best_hash.is_empty() && t.best_hash != hex::encode(job.prev_hash) {
                BLOCKS_REJECTED_STALE.fetch_add(1, Ordering::SeqCst);
                warn!(
                    "[block] stale-template worker={} height={} job_prev={} node_best={} node_height={}",
                    worker,
                    job.height,
                    hex::encode(job.prev_hash),
                    t.best_hash,
                    t.height
                );
                return Ok(true);
            }
        }

        let version_submit = u32::from_le_bytes(selected_header[0..4].try_into().unwrap_or([0u8; 4]));
        let time_submit = u32::from_le_bytes(selected_header[68..72].try_into().unwrap_or([0u8; 4]));
        let bits_submit = u32::from_le_bytes(selected_header[72..76].try_into().unwrap_or([0u8; 4]));
        let nonce_submit = u32::from_le_bytes(selected_header[76..80].try_into().unwrap_or([0u8; 4]));

        let mut prev_sel = [0u8; 32];
        prev_sel.copy_from_slice(&selected_header[4..36]);
        let mut merkle_sel = [0u8; 32];
        merkle_sel.copy_from_slice(&selected_header[36..68]);

        let variant_head = selected_variant
            .split(':')
            .next()
            .unwrap_or("prev_canon+mr_built_x2_raw_canon");
        let (prev_tag, mr_tag) = variant_head
            .split_once('+')
            .unwrap_or(("prev_canon", "mr_built_x2_raw_canon"));

        let prev_canon = match prev_tag {
            "prev_canon" => prev_sel,
            "prev_rev32" => reverse_32(prev_sel),
            "prev_swap4" => swap4_bytes_each_word(prev_sel),
            "prev_rev32_swap4" => reverse_32(swap4_bytes_each_word(prev_sel)),
            _ => job.prev_hash,
        };

        let merkle_from_variant = if mr_tag.ends_with("_rev32") {
            reverse_32(merkle_sel)
        } else {
            merkle_sel
        };
        let merkle_expected = merkle_root_from_coinbase(sha256d(&cb_submit), &job.branches);
        let merkle_canon = if merkle_from_variant == merkle_expected {
            merkle_from_variant
        } else {
            debug!(
                "[block] merkle_variant_mismatch worker={} job={} variant={} from_variant={} expected={}",
                worker,
                job.job_id,
                selected_variant,
                hex::encode(merkle_from_variant),
                hex::encode(merkle_expected)
            );
            merkle_expected
        };

        let mut prev_wire = prev_canon;
        prev_wire.reverse();
        let mut merkle_wire = merkle_canon;
        merkle_wire.reverse();

        let header_wire = header_bytes(
            version_submit,
            prev_wire,
            merkle_wire,
            time_submit,
            bits_submit,
            nonce_submit,
        );
            let submit_hash = sha256d(&header_wire);
            let submit_hash_int = BigUint::from_bytes_be(&submit_hash);

        let mut selected_cmp_hash = hash;
        if use_le {
            selected_cmp_hash.reverse();
        }
        let selected_hash_int = BigUint::from_bytes_be(&selected_cmp_hash);

        let candidate_by_selected = selected_ok_block || selected_hash_int <= block_target;
        let candidate_by_mapped = submit_hash_int <= block_target;

        if !candidate_by_mapped {
            if candidate_by_selected {
                debug!(
                    "[block] candidate_selected_only_ignored worker={} job={} height={} selected_variant={} selected_hash={} mapped_hash={} target_network={}",
                    worker,
                    job.job_id,
                    job.height,
                    selected_variant,
                    hex::encode(hash),
                    hex::encode(submit_hash),
                    biguint_to_32hex(&block_target),
                );
            } else {
                debug!(
                    "[block] candidate_not_network worker={} job={} height={} selected_variant={} selected_hash={} mapped_hash={} target_network={}",
                    worker,
                    job.job_id,
                    job.height,
                    selected_variant,
                    hex::encode(hash),
                    hex::encode(submit_hash),
                    biguint_to_32hex(&block_target),
                );
            }
            return Ok(true);
        }

        CANDIDATE_HASHES.fetch_add(1, Ordering::SeqCst);
        CANDIDATES_DETECTED.fetch_add(1, Ordering::SeqCst);

        info!(
            "[block] candidate_detected worker={} job={} height={} hash={} target_network={} nbits={} ntime={} nonce={} variant={} source={}",
            worker,
            job.job_id,
            job.height,
            hex::encode(submit_hash),
            biguint_to_32hex(&block_target),
            job.nbits_hex,
            ntime_hex,
            nonce_hex,
            selected_variant,
            if candidate_by_mapped { "mapped" } else { "selected" },
        );

        let mut tx_hex = Vec::with_capacity(job.tx_hex.len() + 1);
        tx_hex.push(hex::encode(&cb_submit));
        tx_hex.extend(job.tx_hex.clone());

        let submit_height = job.height;
        info!("[block] header_assembly worker={} submit_height={} header_len={} prev_sel={} prev_canon={} merkle_sel={} merkle_canon={} version={} time={} bits={:08x} nonce={}",
            worker,
            submit_height,
            header_wire.len(),
            hex::encode(prev_sel),
            hex::encode(prev_canon),
            hex::encode(merkle_sel),
            hex::encode(merkle_canon),
            version_submit,
            time_submit,
            bits_submit,
            nonce_submit
        );

        let req = SubmitRequest {
            height: submit_height,
            header: SubmitHeader {
                version: version_submit,
                prev_hash: hex::encode(prev_canon),
                merkle_root: hex::encode(merkle_canon),
                time: time_submit,
                bits: format!("{:08x}", bits_submit),
                nonce: nonce_submit,
                hash: hex::encode(submit_hash),
            },
            tx_hex,
        };
        if config.dev_simulate_submit {
            SIMULATED_SUBMITS.fetch_add(1, Ordering::SeqCst);
            info!("[block-submit] SIMULATED BLOCK SUBMIT OK worker={} height={} job={} hash={}", worker, job.height, job.job_id, hex::encode(hash));
            return Ok(true);
        }

        CANDIDATES_SUBMITTED.fetch_add(1, Ordering::SeqCst);
        let tip_h = tip.as_ref().map(|x| x.height).unwrap_or(0);
        let tip_hash = tip.as_ref().map(|x| x.best_hash.clone()).unwrap_or_default();

        match submit_block_with_classification(config, &req).await {
            Ok((SubmitClass::Accepted, detail)) => {
                BLOCKS_ACCEPTED.fetch_add(1, Ordering::SeqCst);
                info!("[block-submit] result=accepted worker={} height={} node_tip_height={} node_tip_hash={} detail={}", worker, job.height, tip_h, tip_hash, detail);
            }
            Ok((SubmitClass::Stale, detail)) => {
                BLOCKS_REJECTED_STALE.fetch_add(1, Ordering::SeqCst);
                warn!("[block-submit] result=rejected class=stale worker={} height={} node_tip_height={} node_tip_hash={} detail={}", worker, job.height, tip_h, tip_hash, detail);
            }
            Ok((SubmitClass::Invalid, detail)) => {
                BLOCKS_REJECTED_INVALID.fetch_add(1, Ordering::SeqCst);
                warn!("[block-submit] result=rejected class=invalid worker={} height={} node_tip_height={} node_tip_hash={} detail={}", worker, job.height, tip_h, tip_hash, detail);
            }
            Ok((SubmitClass::Duplicate, detail)) => {
                BLOCKS_REJECTED_DUPLICATE.fetch_add(1, Ordering::SeqCst);
                warn!("[block-submit] result=rejected class=duplicate worker={} height={} node_tip_height={} node_tip_hash={} detail={}", worker, job.height, tip_h, tip_hash, detail);
            }
            Ok((SubmitClass::RpcError, detail)) => {
                RPC_ERRORS.fetch_add(1, Ordering::SeqCst);
                BLOCKS_REJECTED_OTHER.fetch_add(1, Ordering::SeqCst);
                warn!("[block-submit] result=rpc_error worker={} height={} node_tip_height={} node_tip_hash={} detail={}", worker, job.height, tip_h, tip_hash, detail);
            }
            Ok((SubmitClass::Other, detail)) => {
                BLOCKS_REJECTED_OTHER.fetch_add(1, Ordering::SeqCst);
                warn!("[block-submit] result=rejected class=other worker={} height={} node_tip_height={} node_tip_hash={} detail={}", worker, job.height, tip_h, tip_hash, detail);
            }
            Err(e) => {
                RPC_ERRORS.fetch_add(1, Ordering::SeqCst);
                BLOCKS_REJECTED_OTHER.fetch_add(1, Ordering::SeqCst);
                warn!("[block-submit] result=error worker={} height={} err={}", worker, job.height, e);
            }
        }
    }

    if ok_share_canonical && config.vardiff_enabled {
        let now = unix_now_secs();
        if let Some(last_share) = session.last_share_ts {
            let observed = now.saturating_sub(last_share);
            VARDIFF_INTERVAL_SUM_MS.fetch_add(observed.saturating_mul(1000), Ordering::SeqCst);
            VARDIFF_INTERVAL_COUNT.fetch_add(1, Ordering::SeqCst);

            let (class_min, class_max) = miner_diff_bounds(&session.miner_class);
            let floor = class_min.max(config.vardiff_min_diff);
            let ceil = class_max.min(config.vardiff_max_diff).max(floor);

            let mut new_diff = session.difficulty;
            if observed < 5 {
                new_diff = (session.difficulty * 2.0).min(ceil);
            } else if observed > 20 {
                new_diff = (session.difficulty / 2.0).max(floor);
            }

            if (new_diff - session.difficulty).abs() > f64::EPSILON {
                session.last_retarget_ts = now;
                let old = session.difficulty;
                session.difficulty = new_diff;
                VARDIFF_ADJUSTMENTS.fetch_add(1, Ordering::SeqCst);
                record_worker_diff(&worker, new_diff);
                info!("[vardiff] worker={} class={} old_diff={} new_diff={} observed_share_s={}", worker, session.miner_class.as_str(), old, new_diff, observed);
                send_set_difficulty(wr, conn_id, session.worker.as_deref(), session.difficulty).await?;
            }
        }
        session.last_share_ts = Some(now);
    }

    Ok(true)
}

async fn send_set_difficulty(
    wr: &mut tokio::net::tcp::OwnedWriteHalf,
    conn_id: u64,
    worker: Option<&str>,
    diff: f64,
) -> Result<()> {
    let worker_name = worker.unwrap_or("-");
    info!(
        "[diff] send conn={} worker={} assigned_diff={}",
        conn_id, worker_name, diff
    );
    let msg = json!({"id": Value::Null, "method": "mining.set_difficulty", "params": [diff]});
    write_json(wr, &msg).await
}

async fn send_notify(
    wr: &mut tokio::net::tcp::OwnedWriteHalf,
    session: &mut SessionState,
    job: &Job,
) -> Result<()> {
    let pkh = session.pkh.ok_or_else(|| anyhow!("unauthorized"))?;
    let (cb1, cb2) = coinbase_prefix_suffix_with_roll(job.height, job.coinbase_value, &pkh, session.coinbase_bip34, job.roll_nonce);
    let branches: Vec<String> = job.branches.iter().map(hex::encode).collect();
    let mut en_preview = session.extranonce1.clone();
    en_preview.extend_from_slice(&[0u8; 4]);
    let cb_preview = build_coinbase_tx_with_roll(job.height, job.coinbase_value, &pkh, &en_preview, session.coinbase_bip34, job.roll_nonce);
    let mr_preview = merkle_root_from_coinbase(sha256d(&cb_preview), &job.branches);
    info!("[coinbase] new merkle root generated worker={} job={} merkle_root={} roll_nonce={}", session.worker.as_deref().unwrap_or("-"), job.job_id, hex::encode(mr_preview), job.roll_nonce);
    // Bitcoin-standard wire target: header[4..36] must contain
    // reverse_32(display) = sha256d_native. iriumd's serialize_for_height
    // reverses prev_hash (per peek_prev_hash comment "wire = natural order,
    // stored = display order which is reverse(wire)"). So the ASIC's
    // header[4..36] must equal reverse_32(display).
    //
    // Antminer-class firmware applies swap4 (per-32-bit-word byteswap) to
    // whatever prev_hash hex it receives in mining.notify before placing it
    // in header[4..36]. To cancel that out and land on Bitcoin-standard wire,
    // pre-condition the value: send swap4(reverse_32(display)). After ASIC's
    // own swap4: swap4(swap4(reverse_32(display))) = reverse_32(display). The
    // brute-force loop then matches the `prev_rev32` variant which is the
    // only one mode_allows_combo::Asic accepts post-A2.
    let mut prev_rev = job.prev_hash;
    prev_rev.reverse();
    let prev_hex = hex::encode(swap4_bytes_each_word(prev_rev));
    session.last_notify_coinbase1 = cb1.clone();
    session.last_notify_coinbase2 = cb2.clone();
    let cb1_hex = hex::encode(&cb1);
    let cb2_hex = hex::encode(&cb2);
    info!(
        "[notify] worker={} job={} version=00000001 prevhash={} nbits={} ntime={} extranonce1={} coinbase1={} coinbase2={} branches={}",
        session.worker.as_deref().unwrap_or("-"),
        job.job_id,
        prev_hex,
        job.nbits_hex,
        job.ntime_hex,
        hex::encode(&session.extranonce1),
        cb1_hex,
        cb2_hex,
        branches.join(",")
    );
    let msg = json!({
        "id": Value::Null,
        "method": "mining.notify",
        "params": [
            job.job_id,
            prev_hex,
            cb1_hex,
            cb2_hex,
            branches,
            "00000001",
            job.nbits_hex,
            job.ntime_hex,
            job.clean_jobs
        ]
    });
    write_json(wr, &msg).await
}

fn merkle_steps_hex(coinbase_hash: [u8; 32], branches: &[[u8; 32]]) -> String {
    let mut root = coinbase_hash;
    let mut parts = Vec::new();
    for (i, b) in branches.iter().enumerate() {
        let mut data = Vec::with_capacity(64);
        data.extend_from_slice(&root);
        data.extend_from_slice(b);
        root = sha256d(&data);
        parts.push(format!("{}:{}->{}", i, hex::encode(b), hex::encode(root)));
    }
    if parts.is_empty() {
        return "none".to_string();
    }
    parts.join("|")
}

fn decode_hex4(s: &str) -> Result<[u8; 4]> {
    let raw = hex::decode(s).map_err(|e| anyhow!("hex decode: {e}"))?;
    if raw.len() != 4 {
        return Err(anyhow!("expected 4-byte hex, got {} bytes", raw.len()));
    }
    let mut out = [0u8; 4];
    out.copy_from_slice(&raw);
    Ok(out)
}

fn header_bytes_from_wire(
    version: [u8; 4],
    prev_hash: [u8; 32],
    merkle_root: [u8; 32],
    ntime: [u8; 4],
    nbits: [u8; 4],
    nonce: [u8; 4],
) -> [u8; 80] {
    let mut h = [0u8; 80];
    h[0..4].copy_from_slice(&version);
    h[4..36].copy_from_slice(&prev_hash);
    h[36..68].copy_from_slice(&merkle_root);
    h[68..72].copy_from_slice(&ntime);
    h[72..76].copy_from_slice(&nbits);
    h[76..80].copy_from_slice(&nonce);
    h
}

fn reverse_4(input: [u8; 4]) -> [u8; 4] {
    [input[3], input[2], input[1], input[0]]
}

fn reverse_32(mut input: [u8; 32]) -> [u8; 32] {
    input.reverse();
    input
}

fn swap4_bytes_each_word(input: [u8; 32]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for i in 0..8 {
        let j = i * 4;
        out[j] = input[j + 3];
        out[j + 1] = input[j + 2];
        out[j + 2] = input[j + 1];
        out[j + 3] = input[j];
    }
    out
}

fn biguint_to_32hex(v: &BigUint) -> String {
    let mut bytes = v.to_bytes_be();
    if bytes.len() > 32 {
        bytes = bytes[bytes.len() - 32..].to_vec();
    }
    if bytes.len() < 32 {
        let mut padded = vec![0u8; 32 - bytes.len()];
        padded.extend_from_slice(&bytes);
        bytes = padded;
    }
    hex::encode(bytes)
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

async fn write_json(wr: &mut tokio::net::tcp::OwnedWriteHalf, v: &Value) -> Result<()> {
    let mut s = serde_json::to_string(v)?;
    s.push('\n');
    wr.write_all(s.as_bytes()).await?;
    Ok(())
}

pub fn dev_replay_header(header_hex: &str) -> Result<bool> {
    let h = header_hex.trim().trim_start_matches("0x");
    let raw = hex::decode(h).map_err(|e| anyhow!("header decode: {e}"))?;
    if raw.len() != 80 {
        return Err(anyhow!("header must be exactly 80 bytes, got {}", raw.len()));
    }

    let mut bits_b = [0u8; 4];
    bits_b.copy_from_slice(&raw[72..76]);
    let bits = u32::from_be_bytes(bits_b);

    let hash = sha256d(&raw);
    let target = target_from_bits(bits);
    let candidate = BigUint::from_bytes_be(&hash) <= target;

    info!(
        "[dev-replay] computed_hash={} target_network={} bits_be={:08x} candidate={} header_len=80",
        hex::encode(hash),
        biguint_to_32hex(&target),
        bits,
        candidate
    );

    Ok(candidate)
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::pow::target_from_bits;

    #[test]
    fn test_header_hash_vector_from_logs() {
        let header_hex = "0000000100000000011a29422d2da5ee9df411a62bfbe0f6264d8fbcdea47ba8bd3fba0179e6c4695b48b9e043e22890fb09ecda0728aeddd90dfe4f03bc427fd26841e969a8cd3f1c01782e6c321467";
        let expected_hash = "0420549adc8d6ccfeb01a19d2b2cbca444e0d8d609abdd54371d6843cb92e2e1";
        let raw = hex::decode(header_hex).unwrap();
        let h = sha256d(&raw);
        assert_eq!(hex::encode(h), expected_hash);
    }

    #[test]
    fn test_network_target_vector_chain_bits() {
        let t = target_from_bits(0x1c01782e);
        assert_eq!(biguint_to_32hex(&t), "0000000001782e00000000000000000000000000000000000000000000000000");
    }
}
