use crate::block::{
    build_coinbase_tx, build_merkle_branches, coinbase_prefix_suffix, header_bytes,
    merkle_root_from_coinbase, parse_address_to_pkh, parse_hex32, parse_u32_hex,
};
use crate::pow::{sha256d, target_from_bits, target_from_difficulty_with_limit};
use crate::template::{GetBlockTemplate, TemplateClient};
use anyhow::{anyhow, Result};
use num_bigint::BigUint;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
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
}

#[derive(Clone)]
struct SessionState {
    extranonce1: Vec<u8>,
    worker: Option<String>,
    pkh: Option<[u8; 20]>,
    difficulty: f64,
    current_job: Option<Job>,
    last_share_ts: Option<u64>,
    last_retarget_ts: u64,
    coinbase_bip34: bool,
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
static LAST_SHARE_ACCEPTED_AT: AtomicU64 = AtomicU64::new(0);
static LAST_SHARE_REJECTED_AT: AtomicU64 = AtomicU64::new(0);

fn mark_accepted_share() {
    ACCEPTED_SHARES.fetch_add(1, Ordering::SeqCst);
    LAST_SHARE_ACCEPTED_AT.store(unix_now_secs(), Ordering::SeqCst);
}

fn mark_rejected_share() {
    REJECTED_SHARES.fetch_add(1, Ordering::SeqCst);
    LAST_SHARE_REJECTED_AT.store(unix_now_secs(), Ordering::SeqCst);
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
                        "last_share_accepted_at": LAST_SHARE_ACCEPTED_AT.load(Ordering::SeqCst),
                        "last_share_rejected_at": LAST_SHARE_REJECTED_AT.load(Ordering::SeqCst)
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
        "[stratum] listening on {} hash_cmp_mode={} miner_family_mode={} sharecheck_samples={} vardiff={} min={} max={} target_s={} retarget_s={} coinbase_bip34={}",
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

    loop {
        match client.fetch_template().await {
            Ok(tpl) => {
                let job = to_job(seq, &tpl)?;
                let prevhash = hex::encode(job.prev_hash);
                let now_ts = unix_now_secs();
                {
                    let mut st = template_state.write().await;
                    st.last_height = job.height;
                    st.last_prevhash = prevhash.clone();
                    st.last_update_unix = now_ts;
                }
                info!("[tmpl] height={} prev={} ts={}", job.height, prevhash, now_ts);

                let key = format!("{}:{}", job.height, prevhash);
                if key != last_key {
                    last_key = key;
                    seq = seq.wrapping_add(1);
                    {
                        let mut w = current.write().await;
                        *w = Some(job.clone());
                    }
                    let _ = tx.send(job.clone());
                    info!(
                        "[job] id={} height={} block_target={} bits={} prev={}",
                        job.job_id,
                        job.height,
                        biguint_to_32hex(&target_from_bits(job.bits)),
                        job.nbits_hex,
                        hex::encode(job.prev_hash)
                    );
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

fn to_job(seq: u64, tpl: &GetBlockTemplate) -> Result<Job> {
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
        current_job: None,
        last_share_ts: None,
        last_retarget_ts: unix_now_secs(),
        coinbase_bip34: config.coinbase_bip34,
    };

    let (rd, mut wr) = stream.into_split();
    let mut lines = BufReader::new(rd).lines();

    let result = loop {
        tokio::select! {
            job = rx.recv() => {
                if let Ok(j) = job {
                    if session.pkh.is_some() {
                        if let Err(e) = send_set_difficulty(&mut wr, id, session.worker.as_deref(), session.difficulty).await { break Err(e); }
                        if let Err(e) = send_notify(&mut wr, &session, &j).await { break Err(e); }
                    }
                    session.current_job = Some(j);
                }
            }
            line = lines.next_line() => {
                let Some(line) = line? else { return Err(anyhow!("EOF")); };
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
            send_set_difficulty(wr, conn_id, session.worker.as_deref(), session.difficulty).await?;
        }
        "mining.authorize" => {
            let user = params.first().and_then(|v| v.as_str()).unwrap_or("");
            let addr = user.split('.').next().unwrap_or("").trim();
            match parse_address_to_pkh(addr) {
                Ok(pkh) => {
                    session.worker = Some(user.to_string());
                    session.pkh = Some(pkh);
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
                    let resp = json!({"id": id, "result": false, "error": [23, e.to_string(), null]});
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

fn mode_allows_combo(
    mode: &MinerFamilyMode,
    prev_name: &str,
    mr_name: &str,
    v_name: &str,
    t_name: &str,
    b_name: &str,
    n_name: &str,
) -> bool {
    match mode {
        MinerFamilyMode::Asic => prev_name == "prev_canon" && mr_name == "mr_canon" && v_name == "v_be" && t_name == "t_raw" && b_name == "b_raw" && n_name == "n_raw",
        MinerFamilyMode::Ccminer => (prev_name == "prev_canon" || prev_name == "prev_rev32") && (mr_name == "mr_canon" || mr_name == "mr_rev32") && v_name == "v_be" && t_name == "t_raw" && b_name == "b_raw" && (n_name == "n_raw" || n_name == "n_rev"),
        MinerFamilyMode::Auto => (prev_name == "prev_canon" || prev_name == "prev_rev32" || prev_name == "prev_swap4" || prev_name == "prev_rev32_swap4") && (mr_name == "mr_canon" || mr_name == "mr_rev32") && v_name == "v_be" && t_name == "t_raw" && b_name == "b_raw" && (n_name == "n_raw" || n_name == "n_rev"),
        MinerFamilyMode::Cpuminer => true,
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
    let pkh = session.pkh.ok_or_else(|| anyhow!("unauthorized"))?;
    let job = session
        .current_job
        .clone()
        .ok_or_else(|| anyhow!("no active job"))?;

    if params.len() < 5 {
        return Err(anyhow!("invalid params"));
    }
    let job_id = params[1].as_str().unwrap_or("");
    let extranonce2_hex = params[2].as_str().unwrap_or("");
    let ntime_hex = params[3].as_str().unwrap_or("");
    let nonce_hex = params[4].as_str().unwrap_or("");

    if job_id != job.job_id {
        return Err(anyhow!("stale share"));
    }

    let extra2 = hex::decode(extranonce2_hex).map_err(|e| anyhow!("extranonce2 decode: {e}"))?;
    let mut en = session.extranonce1.clone();
    en.extend_from_slice(&extra2);

    let cb = build_coinbase_tx(job.height, job.coinbase_value, &pkh, &en, config.coinbase_bip34);
    let cb_hash = sha256d(&cb);
    let mr = merkle_root_from_coinbase(cb_hash, &job.branches);

    let ntime = parse_u32_hex(ntime_hex)?;
    let nonce = parse_u32_hex(nonce_hex)?;
    let version: u32 = 1;
    let mut mr_rev = mr;
    mr_rev.reverse();
    let mr_swap4 = swap4_bytes_each_word(mr);
    let mr_rev32_swap4 = swap4_bytes_each_word(mr_rev);

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

    let merkle_variants: [(&str, [u8; 32]); 4] = [
        ("mr_canon", mr),
        ("mr_rev32", mr_rev),
        ("mr_swap4", mr_swap4),
        ("mr_rev32_swap4", mr_rev32_swap4),
    ];

    let share_target = target_from_difficulty_with_limit(session.difficulty, &config.pow_limit);
    let block_target = target_from_bits(job.bits);

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
    let ntime_raw = decode_hex4(ntime_hex)?;
    let nonce_raw = decode_hex4(nonce_hex)?;
    let version_be = [0x00, 0x00, 0x00, 0x01];
    let version_le = version.to_le_bytes();

    let version_opts = [("v_be", version_be), ("v_le", version_le)];
    let time_opts = [("t_raw", ntime_raw), ("t_rev", reverse_4(ntime_raw))];
    let bits_opts = [("b_raw", nbits_raw), ("b_rev", reverse_4(nbits_raw))];
    let nonce_opts = [("n_raw", nonce_raw), ("n_rev", reverse_4(nonce_raw))];

    for (prev_name, prev_for_header) in prev_variants {
        for (mr_name, mr_for_header) in merkle_variants {
            for (v_name, v_bytes) in version_opts {
                for (t_name, t_bytes) in time_opts {
                    for (b_name, b_bytes) in bits_opts {
                        for (n_name, n_bytes) in nonce_opts {
                            if !mode_allows_combo(&config.miner_family_mode, prev_name, mr_name, v_name, t_name, b_name, n_name) {
                                continue;
                            }
                            let hdr_v = header_bytes_from_wire(
                                v_bytes,
                                prev_for_header,
                                mr_for_header,
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
    info!(
        "[sharedebug-header] worker={} job={} variant={} version_hex={:08x} prevhash={} merkle_root={} ntime_hex={} nbits_hex={} nonce_hex={:08x} header80={} hash={}",
        worker,
        job.job_id,
        canonical.name,
        version,
        hex::encode(job.prev_hash),
        hex::encode(mr),
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

    let mut hash = canonical.hash;
    let mut ok_block = false;
    let selected_variant;

    let check_line = if let Some(idx) = accepted_idx {
        let chosen = &checks[idx];
        hash = chosen.hash;
        ok_block = if use_le { chosen.ok_block_le } else { chosen.ok_block_be };
        selected_variant = chosen.name;
        format!(
            "[sharecheck] worker={} job={} assigned_diff={} share_target={} block_target={} chosen_variant={} variants_checked={} sample={}",
            worker,
            job.job_id,
            session.difficulty,
            biguint_to_32hex(&share_target),
            biguint_to_32hex(&block_target),
            selected_variant,
            total_variants, summary
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
            total_variants, summary
        )
    };

    let ok_share = accepted_idx.is_some();

    if ok_share {
        mark_accepted_share();
        info!("{}", check_line);
        info!("[share] accepted worker={} hash={}", worker, hex::encode(hash));
    } else if config.soft_accept_invalid_shares {
        // Count compatibility soft-accepts as accepted responses from the pool API perspective.
        mark_accepted_share();
        warn!("{}", check_line);
        warn!("[share] soft-accepted worker={} reason=compat_soft_accept", worker);
        return Ok(true);
    } else {
        mark_rejected_share();
        warn!("{}", check_line);
        warn!("[share] reject worker={} reason=low_difficulty", worker);
        return Ok(false);
    }

    if ok_block {
        info!(
            "[block] candidate worker={} height={} hash={}",
            worker,
            job.height,
            hex::encode(hash)
        );
        let mut tx_hex = Vec::with_capacity(job.tx_hex.len() + 1);
        tx_hex.push(hex::encode(&cb));
        tx_hex.extend(job.tx_hex.clone());

        let req = SubmitRequest {
            height: job.height,
            header: SubmitHeader {
                version: 1,
                prev_hash: hex::encode(job.prev_hash),
                merkle_root: hex::encode(mr),
                time: ntime,
                bits: format!("{:08x}", job.bits),
                nonce,
                hash: hex::encode(hash),
            },
            tx_hex,
        };

        let url = format!("{}/rpc/submit_block", config.rpc_base.trim_end_matches('/'));
        let client = reqwest::Client::builder().danger_accept_invalid_certs(true).build()?;
        let resp = client
            .post(url)
            .bearer_auth(&config.rpc_token)
            .json(&req)
            .send()
            .await?;
        if resp.status().is_success() {
            info!("[block] submitted worker={} height={}", worker, job.height);
        } else {
            warn!("[block] submit failed status={} worker={}", resp.status(), worker);
        }
    }

    if ok_share && config.vardiff_enabled {
        let now = unix_now_secs();
        if let Some(last_share) = session.last_share_ts {
            let observed = now.saturating_sub(last_share);
            let since_retarget = now.saturating_sub(session.last_retarget_ts);
            if since_retarget >= config.vardiff_retarget_secs {
                let mut new_diff = session.difficulty;
                let fast_threshold = (config.vardiff_target_share_secs / 2).max(1);
                let slow_threshold = config.vardiff_target_share_secs.saturating_mul(2);
                if observed < fast_threshold {
                    new_diff = (session.difficulty * 2.0).min(config.vardiff_max_diff);
                } else if observed > slow_threshold {
                    new_diff = (session.difficulty / 2.0).max(config.vardiff_min_diff);
                }
                if (new_diff - session.difficulty).abs() > f64::EPSILON {
                    let old = session.difficulty;
                    session.difficulty = new_diff;
                    session.last_retarget_ts = now;
                    info!(
                        "[vardiff] worker={} old_diff={} new_diff={} observed_share_s={} target_s={}",
                        worker, old, new_diff, observed, config.vardiff_target_share_secs
                    );
                    send_set_difficulty(wr, conn_id, session.worker.as_deref(), session.difficulty).await?;
                }
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
    session: &SessionState,
    job: &Job,
) -> Result<()> {
    let pkh = session.pkh.ok_or_else(|| anyhow!("unauthorized"))?;
    let (cb1, cb2) = coinbase_prefix_suffix(job.height, job.coinbase_value, &pkh, session.coinbase_bip34);
    let branches: Vec<String> = job.branches.iter().map(hex::encode).collect();
    let prev_hex = hex::encode(job.prev_hash);
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
            true
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
