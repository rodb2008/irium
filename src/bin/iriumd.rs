use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use std::sync::{Arc, Mutex, OnceLock};
use std::{env, fs};

use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, Json as AxumJson, Query, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use hex;

use bs58;
use irium_node_rs::anchors::AnchorManager;
use irium_node_rs::block::{Block, BlockHeader};
use irium_node_rs::chain::{block_from_locked, ChainParams, ChainState, OutPoint};
use irium_node_rs::constants::block_reward;
use irium_node_rs::genesis::load_locked_genesis;
use irium_node_rs::mempool::MempoolManager;
use irium_node_rs::network::SeedlistManager;
use irium_node_rs::p2p::P2PNode;
use irium_node_rs::pow::Target;
use irium_node_rs::rate_limiter::RateLimiter;
use irium_node_rs::storage;
use irium_node_rs::reputation::ReputationManager;
use irium_node_rs::tx::{decode_full_tx, Transaction, TxInput, TxOutput};
use get_if_addrs::get_if_addrs;

const IRIUM_P2PKH_VERSION: u8 = 0x39;

#[derive(Clone)]
struct AppState {
    chain: Arc<Mutex<ChainState>>,
    genesis_hash: String,
    mempool: Arc<Mutex<MempoolManager>>,
    anchors: Option<AnchorManager>,
    p2p: Option<P2PNode>,
    limiter: Arc<Mutex<RateLimiter>>,
}

#[derive(Serialize)]
struct PeerInfo {
    multiaddr: String,
    agent: Option<String>,
    height: Option<u64>,
    last_seen: f64,
}

#[derive(Serialize)]
struct PeersResponse {
    peers: Vec<PeerInfo>,
}

#[derive(Serialize)]
struct StatusResponse {
    height: u64,
    genesis_hash: String,
    anchors_digest: Option<String>,
    peer_count: usize,
    anchor_loaded: bool,
    node_id: Option<String>,
    sybil_difficulty: Option<u8>,
}

#[derive(Serialize)]
struct UtxoResponse {
    value: u64,
    height: u64,
    is_coinbase: bool,
}

#[derive(Serialize)]
struct BalanceResponse {
    address: String,
    pkh: String,
    balance: u64,
    mined_balance: u64,
    utxo_count: usize,
    mined_blocks: usize,
    height: u64,
}

#[derive(Serialize)]
struct UtxoItem {
    txid: String,
    index: u32,
    value: u64,
    height: u64,
    is_coinbase: bool,
    script_pubkey: String,
}

#[derive(Serialize)]
struct UtxosResponse {
    address: String,
    pkh: String,
    height: u64,
    utxos: Vec<UtxoItem>,
}

#[derive(Serialize)]
struct HistoryItem {
    txid: String,
    height: u64,
    received: u64,
    spent: u64,
    net: i64,
    is_coinbase: bool,
}

#[derive(Serialize)]
struct HistoryResponse {
    address: String,
    pkh: String,
    height: u64,
    txs: Vec<HistoryItem>,
}

#[derive(Serialize)]
struct FeeEstimateResponse {
    min_fee_per_byte: f64,
    mempool_size: usize,
}

#[derive(Deserialize)]
struct UtxoQuery {
    txid: String,
    index: u32,
}

#[derive(Deserialize)]
struct BalanceQuery {
    address: String,
}

#[derive(Deserialize)]
struct UtxosQuery {
    address: String,
}

#[derive(Deserialize)]
struct BlockQuery {
    height: u64,
}

#[derive(Deserialize)]
struct TemplateQuery {
    longpoll: Option<u8>,
    poll_secs: Option<u64>,
    max_txs: Option<usize>,
    min_fee: Option<f64>,
}

#[derive(Deserialize)]
struct SubmitTxRequest {
    tx_hex: String,
}

#[derive(Serialize)]
struct SubmitTxResponse {
    txid: String,
    accepted: bool,
}

#[derive(Serialize)]
struct TemplateTx {
    hex: String,
    fee: u64,
    relay_addresses: Vec<String>,
}

#[derive(Serialize)]
struct BlockTemplateResponse {
    height: u64,
    prev_hash: String,
    bits: String,
    target: String,
    time: u32,
    txs: Vec<TemplateTx>,
    total_fees: u64,
    coinbase_value: u64,
    mempool_count: usize,
}

#[derive(Deserialize)]
struct SubmitBlockHeader {
    version: u32,
    prev_hash: String,
    merkle_root: String,
    time: u32,
    bits: String,
    nonce: u32,
    hash: String,
}

#[derive(Deserialize)]
struct SubmitBlockRequest {
    height: u64,
    header: SubmitBlockHeader,
    tx_hex: Vec<String>,
}

#[derive(Deserialize)]
struct NodeConfig {
    /// Optional P2P bind address, e.g. "0.0.0.0:38291".
    #[serde(default)]
    p2p_bind: Option<String>,
    /// Optional list of seed peers, e.g. ["seed.example.org:38291"].
    #[serde(default)]
    p2p_seeds: Vec<String>,
    /// Optional relay payout address to advertise to peers.
    #[serde(default)]
    relay_address: Option<String>,
}

fn parse_seed_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                None
            } else {
                Some(line.to_string())
            }
        })
        .collect()
}

fn parse_seed_to_socketaddr(seed: &str, default_port: u16) -> Result<std::net::SocketAddr, String> {
    if let Ok(addr) = seed.parse::<std::net::SocketAddr>() {
        return Ok(addr);
    }
    if let Ok(ip) = seed.parse::<std::net::IpAddr>() {
        return format!("{}:{}", ip, default_port)
            .parse::<std::net::SocketAddr>()
            .map_err(|e| e.to_string());
    }
    Err("invalid seed format".to_string())
}
fn local_ip_set(bind: Option<&String>) -> HashSet<IpAddr> {
    let mut ips = HashSet::new();
    if let Some(bind) = bind {
        if let Ok(addr) = bind.parse::<SocketAddr>() {
            ips.insert(addr.ip());
        }
    }
    if let Ok(raw) = env::var("IRIUM_NODE_PUBLIC_IP").or_else(|_| env::var("IRIUM_PUBLIC_IP")) {
        if let Ok(ip) = raw.parse::<IpAddr>() {
            ips.insert(ip);
        }
    }
    if let Ok(ifaces) = get_if_addrs() {
        for iface in ifaces {
            ips.insert(iface.ip());
        }
    }
    // Also query hostname -I so we capture addresses exposed by the OS (e.g., public IPv4 on seeds).
    if let Ok(output) = std::process::Command::new("hostname").arg("-I").output() {
        if output.status.success() {
            if let Ok(list) = String::from_utf8(output.stdout) {
                for part in list.split_whitespace() {
                    if let Ok(ip) = part.parse::<IpAddr>() {
                        ips.insert(ip);
                    }
                }
            }
        }
    }
    // Optional: probe the outbound interface using a user-supplied target.
    if let Ok(target) = env::var("IRIUM_PUBLIC_IP_PROBE_TARGET") {
        if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") {
            if sock.connect(&target).is_ok() {
                if let Ok(addr) = sock.local_addr() {
                    ips.insert(addr.ip());
                }
            }
        }
    }
    ips.insert(IpAddr::V4(Ipv4Addr::LOCALHOST));
    ips.insert(IpAddr::V6(Ipv6Addr::LOCALHOST));
    ips
}

fn mask_ip(ip: &str) -> String {
    match ip.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => {
            let oct = v4.octets();
            format!("{}.{}.*.*", oct[0], oct[1])
        }
        Ok(IpAddr::V6(v6)) => {
            let seg = v6.segments();
            format!("{:x}:{:x}::*", seg[0], seg[1])
        }
        Err(_) => ip.to_string(),
    }
}

fn mask_seed_label(seed: &str) -> String {
    let (ip, port) = seed.split_once(':').unwrap_or((seed, ""));
    let masked_ip = mask_ip(ip);
    if port.is_empty() {
        masked_ip
    } else {
        format!("{}:{}", masked_ip, port)
    }
}

fn load_runtime_seeds() -> Vec<String> {
    let path = std::path::Path::new("bootstrap/seedlist.runtime");
    std::fs::read_to_string(path)
        .map(|raw| parse_seed_lines(&raw))
        .unwrap_or_default()
}

fn load_extra_seeds() -> Vec<String> {
    let path = std::path::Path::new("bootstrap/seedlist.extra");
    std::fs::read_to_string(path)
        .map(|raw| parse_seed_lines(&raw))
        .unwrap_or_default()
}

#[derive(Clone, Copy)]
struct SeedDialInfo {
    total: usize,
    filtered_local: usize,
}

fn load_signed_seeds() -> Vec<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let seed_path = std::path::Path::new("bootstrap/seedlist.txt");
    let sig_path = std::path::Path::new("bootstrap/seedlist.txt.sig");
    let allowed = std::path::Path::new("bootstrap/trust/allowed_signers");
    let Ok(seed_data) = std::fs::read_to_string(seed_path) else {
        return Vec::new();
    };

    let mut child = match Command::new("ssh-keygen")
        .arg("-Y")
        .arg("verify")
        .arg("-f")
        .arg(allowed)
        .arg("-I")
        .arg("bootstrap-signer")
        .arg("-n")
        .arg("file")
        .arg("-s")
        .arg(sig_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    if let Some(stdin) = child.stdin.as_mut() {
        if stdin.write_all(seed_data.as_bytes()).is_err() {
            return Vec::new();
        }
    }
    let status = match child.wait() {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    if status.success() {
        parse_seed_lines(&seed_data)
    } else {
        Vec::new()
    }
}

fn build_seed_addrs(
    config_seeds: &[String],
    signed_seeds: &[String],
    default_seed_port: u16,
    local_ips: &HashSet<IpAddr>,
) -> (Vec<std::net::SocketAddr>, SeedDialInfo) {
    let mut seeds_raw: Vec<String> = Vec::new();
    seeds_raw.extend(config_seeds.iter().cloned());
    seeds_raw.extend(signed_seeds.iter().cloned());
    seeds_raw.extend(load_extra_seeds());
    seeds_raw.extend(load_runtime_seeds());
    seeds_raw.sort();
    seeds_raw.dedup();

    let mut info = SeedDialInfo {
        total: seeds_raw.len(),
        filtered_local: 0,
    };
    let mut seeds: Vec<std::net::SocketAddr> = Vec::new();
    for seed in seeds_raw.iter() {
        match parse_seed_to_socketaddr(seed, default_seed_port) {
            Ok(addr) => {
                if local_ips.contains(&addr.ip()) {
                    info.filtered_local += 1;
                    continue;
                }
                seeds.push(addr)
            }
            Err(e) => eprintln!("Invalid P2P seed {}: {}", seed, e),
        }
    }
    // If everything was filtered as local, only fall back when explicitly allowed.
    if seeds.is_empty() {
        let allow = std::env::var("IRIUM_ALLOW_LOCAL_SEED_FALLBACK")
            .ok()
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
        if allow {
            if let Some(first) = seeds_raw.first() {
                if let Ok(addr) = parse_seed_to_socketaddr(first, default_seed_port) {
                    seeds.push(addr);
                }
            }
        }
    }
    let mut rep_mgr = ReputationManager::new();
    seeds.sort_by(|a, b| {
        rep_mgr
            .score_of(&b.to_string())
            .cmp(&rep_mgr.score_of(&a.to_string()))
    });
    (seeds, info)
}

fn json_log_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("IRIUM_JSON_LOG")
            .ok()
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false)
    })
}

fn base58_p2pkh_to_hash(addr: &str) -> Option<Vec<u8>> {
    let data = bs58::decode(addr).into_vec().ok()?;
    if data.len() < 25 {
        return None;
    }
    let (body, checksum) = data.split_at(data.len() - 4);
    let first = Sha256::digest(body);
    let second = Sha256::digest(&first);
    if &second[0..4] != checksum {
        return None;
    }
    if body.len() < 21 {
        return None;
    }
    let payload = &body[1..];
    if payload.len() != 20 {
        return None;
    }
    Some(payload.to_vec())
}

fn base58_p2pkh_from_hash(pkh: &[u8; 20]) -> String {
    let mut body = Vec::with_capacity(1 + 20);
    body.push(IRIUM_P2PKH_VERSION);
    body.extend_from_slice(pkh);
    let first = Sha256::digest(&body);
    let second = Sha256::digest(&first);
    let checksum = &second[0..4];
    let mut full = body;
    full.extend_from_slice(checksum);
    bs58::encode(full).into_string()
}

fn miner_address_from_tx(tx: &Transaction) -> Option<String> {
    let output = tx.outputs.first()?;
    let pkh = p2pkh_hash_from_script(&output.script_pubkey)?;
    Some(base58_p2pkh_from_hash(&pkh))
}

fn miner_address_from_tx_hex(tx_hex: &str) -> Option<String> {
    let raw = hex::decode(tx_hex).ok()?;
    let tx = decode_full_tx(&raw).ok()?;
    miner_address_from_tx(&tx)
}

fn miner_address_from_block(block: &Block) -> Option<String> {
    block.transactions.first().and_then(miner_address_from_tx)
}


fn p2pkh_hash_from_script(script: &[u8]) -> Option<[u8; 20]> {
    if script.len() != 25 {
        return None;
    }
    if script[0] != 0x76 || script[1] != 0xa9 || script[2] != 0x14 {
        return None;
    }
    if script[23] != 0x88 || script[24] != 0xac {
        return None;
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&script[3..23]);
    Some(out)
}

fn miner_blocks_dir() -> PathBuf {
    if let Ok(dir) = env::var("IRIUM_MINER_BLOCKS_DIR") {
        PathBuf::from(dir)
    } else {
        let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
        PathBuf::from(home).join(".irium/miner/blocks")
    }
}

fn same_dir(a: &PathBuf, b: &PathBuf) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn quarantine_blocks_above_dir(dir: &std::path::Path, height: u64) {
    if !dir.exists() {
        return;
    }
    let read_dir = match dir.read_dir() {
        Ok(r) => r,
        Err(_) => return,
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let backup_dir = dir.join(format!("orphaned_{}", stamp));
    let _ = fs::create_dir_all(&backup_dir);
    for entry in read_dir.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(stripped) = name.strip_prefix("block_") else {
            continue;
        };
        let Some(num_part) = stripped.strip_suffix(".json") else {
            continue;
        };
        let Ok(h) = num_part.parse::<u64>() else {
            continue;
        };
        if h > height {
            let dest = backup_dir.join(name);
            let _ = fs::rename(&path, &dest);
        }
    }
}


fn load_persisted_blocks_from(
    state: &mut ChainState,
    dir: &std::path::Path,
    skip_below_tip: bool,
) {
    if !dir.exists() {
        return;
    }
    let base_height = if skip_below_tip {
        state.tip_height()
    } else {
        0
    };
    let mut entries: Vec<(u64, std::path::PathBuf)> = Vec::new();
    if let Ok(read_dir) = dir.read_dir() {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if let Some(stripped) = name.strip_prefix("block_") {
                    if let Some(num_part) = stripped.strip_suffix(".json") {
                        if let Ok(h) = num_part.parse::<u64>() {
                            if skip_below_tip && h <= base_height {
                                continue;
                            }
                            entries.push((h, path));
                        }
                    }
                }
            }
        }
    }
    entries.sort_by_key(|(h, _)| *h);

    for (h, path) in entries {
        match std::fs::read_to_string(&path) {
            Ok(data) => {
                let parsed: serde_json::Value = match serde_json::from_str(&data) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("[⚠️] Failed to parse {}: {}", path.display(), e);
                        continue;
                    }
                };
                let header_obj = match parsed.get("header") {
                    Some(v) => v,
                    None => continue,
                };
                let get_hex32 = |key: &str| -> Option<[u8; 32]> {
                    let s = header_obj.get(key)?.as_str()?;
                    let bytes = hex::decode(s).ok()?;
                    if bytes.len() != 32 {
                        return None;
                    }
                    let mut out = [0u8; 32];
                    out.copy_from_slice(&bytes);
                    Some(out)
                };
                let prev_hash = match get_hex32("prev_hash") {
                    Some(v) => v,
                    None => continue,
                };
                let merkle_root = match get_hex32("merkle_root") {
                    Some(v) => v,
                    None => continue,
                };
                let version = header_obj.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                let time = header_obj.get("time").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let bits_str = header_obj.get("bits").and_then(|v| v.as_str()).unwrap_or("1d00ffff");
                let bits = u32::from_str_radix(bits_str, 16).unwrap_or(0x1d00_ffff);
                let nonce = header_obj.get("nonce").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

                let txs: Vec<Transaction> = match parsed.get("tx_hex").and_then(|v| v.as_array()) {
                    Some(arr) => {
                        let mut out = Vec::new();
                        for t in arr {
                            if let Some(s) = t.as_str() {
                                if let Ok(bytes) = hex::decode(s) {
                                    match decode_compact_tx(&bytes) {
                                        Ok(tx) => out.push(tx),
                                        Err(e) => eprintln!("[⚠️] Failed to decode tx in {}: {}", path.display(), e),
                                    }
                                }
                            }
                        }
                        out
                    }
                    None => Vec::new(),
                };

                let mut block = Block {
                    header: BlockHeader {
                        version,
                        prev_hash,
                        merkle_root,
                        time,
                        bits,
                        nonce,
                    },
                    transactions: txs,
                };
                block.header.merkle_root = block.merkle_root();

                if let Err(e) = state.connect_block(block) {
                    eprintln!("[⚠️] Failed to connect persisted block {}: {}", h, e);
                    let tip = state.tip_height();
                    quarantine_blocks_above_dir(dir, tip);
                    println!("[🧹] Quarantined persisted blocks above height {}", tip);
                    break;
                }
            }
            Err(e) => eprintln!("[⚠️] Failed to read {}: {}", path.display(), e),
        }
    }
}

fn load_persisted_blocks(state: &mut ChainState) {
    let node_dir = storage::blocks_dir();
    load_persisted_blocks_from(state, &node_dir, false);
    let miner_dir = miner_blocks_dir();
    if !same_dir(&node_dir, &miner_dir) {
        load_persisted_blocks_from(state, &miner_dir, true);
    }

    if state.height > 1 {
        println!("[↩️] Resumed node height {} from persisted blocks", state.height);
    }
}

fn mempool_file() -> PathBuf {
    if let Ok(path) = env::var("IRIUM_MEMPOOL_FILE") {
        PathBuf::from(path)
    } else {
        let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
        PathBuf::from(home).join(".irium/mempool/pending.json")
    }
}

fn rate_limiter() -> RateLimiter {
    let rpm = env::var("IRIUM_RATE_LIMIT_PER_MIN")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(120);
    RateLimiter::new(rpm)
}

fn rpc_body_limit_bytes() -> usize {
    env::var("IRIUM_RPC_BODY_MAX")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(32 * 1024 * 1024)
}

fn require_rpc_auth(headers: &HeaderMap) -> Result<(), StatusCode> {
    let token = match env::var("IRIUM_RPC_TOKEN") {
        Ok(t) => t,
        Err(_) => return Ok(()),
    };
    let expected = format!("Bearer {}", token);
    let header = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok());
    if header == Some(expected.as_str()) {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn rpc_authorized(headers: &HeaderMap) -> bool {
    let token = match env::var("IRIUM_RPC_TOKEN") {
        Ok(t) => t,
        Err(_) => return false,
    };
    let expected = format!("Bearer {}", token);
    let header = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok());
    header == Some(expected.as_str())
}

fn check_rate_with_auth(
    state: &AppState,
    addr: &SocketAddr,
    headers: &HeaderMap,
) -> Result<(), StatusCode> {
    if rpc_authorized(headers) {
        return Ok(());
    }
    check_rate(state, addr)
}

fn check_rate(state: &AppState, addr: &SocketAddr) -> Result<(), StatusCode> {
    let mut limiter = state.limiter.lock().unwrap();
    if limiter.is_allowed(&addr.ip().to_string()) {
        Ok(())
    } else {
        Err(StatusCode::TOO_MANY_REQUESTS)
    }
}

async fn status(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, StatusCode> {
    check_rate(&state, &addr)?;
    let (peer_count, node_id, sybil_diff) = match state.p2p {
        Some(ref p2p) => (
            p2p.peer_count().await,
            Some(p2p.node_id_hex()),
            Some(p2p.current_sybil_difficulty().await),
        ),
        None => (0, None, None),
    };
    let (height, anchors_digest) = {
        let guard = state.chain.lock().unwrap();
        let anchors_digest = state
            .anchors
            .as_ref()
            .map(|a| a.payload_digest().to_string());
        (guard.tip_height(), anchors_digest)
    };
    Ok(Json(StatusResponse {
        height,
        genesis_hash: state.genesis_hash.clone(),
        anchors_digest,
        peer_count,
        anchor_loaded: state.anchors.is_some(),
        node_id,
        sybil_difficulty: sybil_diff,
    }))
}

async fn peers(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> Result<Json<PeersResponse>, StatusCode> {
    check_rate(&state, &addr)?;
    if let Some(ref p2p) = state.p2p {
        let list = p2p
            .peers_snapshot()
            .await
            .into_iter()
            .map(|p| PeerInfo {
                multiaddr: p.multiaddr,
                agent: p.agent,
                height: p.last_height,
                last_seen: p.last_seen,
            })
            .collect();
        Ok(Json(PeersResponse { peers: list }))
    } else {
        Ok(Json(PeersResponse { peers: Vec::new() }))
    }
}

async fn metrics(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> Result<String, StatusCode> {
    check_rate(&state, &addr)?;
    let (height, anchor_loaded, tip_hash, anchor_digest) = {
        let g = state.chain.lock().unwrap();
        let tip_hash = g
            .chain
            .last()
            .map(|b| hex::encode(b.header.hash()))
            .unwrap_or_else(|| state.genesis_hash.clone());
        let digest = state
            .anchors
            .as_ref()
            .map(|a| a.payload_digest().to_string())
            .unwrap_or_default();
        (g.tip_height(), state.anchors.is_some(), tip_hash, digest)
    };
    let (peer_count, node_id_hex, sybil_diff) = match state.p2p {
        Some(ref p2p) => {
            let peers = p2p.peer_count().await;
            let node_id = p2p.node_id_hex();
            let diff = p2p.current_sybil_difficulty().await;
            (peers, node_id, diff)
        }
        None => (0usize, String::new(), 0u8),
    };
    let seeds = SeedlistManager::new(128).merged_seedlist();
    let mempool_sz = state.mempool.lock().unwrap().len();
    Ok(format!(
        "irium_height {}
irium_peers {}
irium_anchor_loaded {}
irium_tip_hash {}
irium_mempool_size {}
irium_anchor_digest {}
irium_node_id {}
irium_sybil_difficulty {}
irium_seed_count {}
",
        height,
        peer_count,
        anchor_loaded as u8,
        tip_hash,
        mempool_sz,
        anchor_digest,
        node_id_hex,
        sybil_diff,
        seeds.len()
    ))
}

async fn get_utxo(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<UtxoQuery>,
) -> Result<Json<UtxoResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    let bytes = match hex::decode(&q.txid) {
        Ok(b) => b,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };
    if bytes.len() != 32 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut txid = [0u8; 32];
    txid.copy_from_slice(&bytes);

    let guard = state.chain.lock().unwrap();
    let key = OutPoint {
        txid,
        index: q.index,
    };
    if let Some(utxo) = guard.utxos.get(&key) {
        Ok(Json(UtxoResponse {
            value: utxo.output.value,
            height: utxo.height,
            is_coinbase: utxo.is_coinbase,
        }))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn get_balance(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<BalanceQuery>,
) -> Result<Json<BalanceResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    let pkh = base58_p2pkh_to_hash(&q.address).ok_or(StatusCode::BAD_REQUEST)?;
    if pkh.len() != 20 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut pkh_arr = [0u8; 20];
    pkh_arr.copy_from_slice(&pkh);

    let (balance, utxo_count, mined_balance, mined_blocks, height) = {
        let guard = state.chain.lock().unwrap();
        let mut balance = 0u64;
        let mut utxo_count = 0usize;
        let mut mined_balance = 0u64;
        let mut mined_blocks = 0usize;
        for utxo in guard.utxos.values() {
            if let Some(script_pkh) = p2pkh_hash_from_script(&utxo.output.script_pubkey) {
                if script_pkh == pkh_arr {
                    balance = balance.saturating_add(utxo.output.value);
                    utxo_count += 1;
                    if utxo.is_coinbase {
                        mined_balance = mined_balance.saturating_add(utxo.output.value);
                        mined_blocks += 1;
                    }
                }
            }
        }
        (
            balance,
            utxo_count,
            mined_balance,
            mined_blocks,
            guard.tip_height(),
        )
    };

    Ok(Json(BalanceResponse {
        address: q.address,
        pkh: hex::encode(pkh_arr),
        balance,
        mined_balance,
        utxo_count,
        mined_blocks,
        height,
    }))
}


async fn get_utxos(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<UtxosQuery>,
) -> Result<Json<UtxosResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    let pkh = base58_p2pkh_to_hash(&q.address).ok_or(StatusCode::BAD_REQUEST)?;
    if pkh.len() != 20 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut pkh_arr = [0u8; 20];
    pkh_arr.copy_from_slice(&pkh);

    let (utxos, height) = {
        let guard = state.chain.lock().unwrap();
        let mut items = Vec::new();
        for (outpoint, utxo) in guard.utxos.iter() {
            if let Some(script_pkh) = p2pkh_hash_from_script(&utxo.output.script_pubkey) {
                if script_pkh == pkh_arr {
                    items.push(UtxoItem {
                        txid: hex::encode(outpoint.txid),
                        index: outpoint.index,
                        value: utxo.output.value,
                        height: utxo.height,
                        is_coinbase: utxo.is_coinbase,
                        script_pubkey: hex::encode(&utxo.output.script_pubkey),
                    });
                }
            }
        }
        (items, guard.tip_height())
    };

    Ok(Json(UtxosResponse {
        address: q.address,
        pkh: hex::encode(pkh_arr),
        height,
        utxos,
    }))
}


async fn get_history(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<BalanceQuery>,
) -> Result<Json<HistoryResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    let pkh = base58_p2pkh_to_hash(&q.address).ok_or(StatusCode::BAD_REQUEST)?;
    if pkh.len() != 20 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut pkh_arr = [0u8; 20];
    pkh_arr.copy_from_slice(&pkh);

    let (height, txs) = {
        let guard = state.chain.lock().unwrap();
        let mut owned: HashMap<OutPoint, u64> = HashMap::new();
        let mut map: HashMap<[u8; 32], HistoryItem> = HashMap::new();

        for (h, block) in guard.chain.iter().enumerate() {
            let height = h as u64;
            for tx in &block.transactions {
                let txid = tx.txid();
                let is_coinbase = tx.inputs.len() == 1
                    && tx.inputs[0].prev_txid == [0u8; 32]
                    && tx.inputs[0].prev_index == 0xffff_ffff;

                let mut received = 0u64;
                let mut spent = 0u64;

                if !is_coinbase {
                    for input in &tx.inputs {
                        let outpoint = OutPoint {
                            txid: input.prev_txid,
                            index: input.prev_index,
                        };
                        if let Some(value) = owned.remove(&outpoint) {
                            spent = spent.saturating_add(value);
                        }
                    }
                }

                for (idx, output) in tx.outputs.iter().enumerate() {
                    if let Some(script_pkh) = p2pkh_hash_from_script(&output.script_pubkey) {
                        if script_pkh == pkh_arr {
                            received = received.saturating_add(output.value);
                            owned.insert(
                                OutPoint {
                                    txid,
                                    index: idx as u32,
                                },
                                output.value,
                            );
                        }
                    }
                }

                if received > 0 || spent > 0 {
                    let entry = map.entry(txid).or_insert(HistoryItem {
                        txid: hex::encode(txid),
                        height,
                        received: 0,
                        spent: 0,
                        net: 0,
                        is_coinbase,
                    });
                    entry.received = entry.received.saturating_add(received);
                    entry.spent = entry.spent.saturating_add(spent);
                    entry.net = entry.received as i64 - entry.spent as i64;
                }
            }
        }

        let mut txs: Vec<HistoryItem> = map.into_values().collect();
        txs.sort_by(|a, b| b.height.cmp(&a.height));
        (guard.tip_height(), txs)
    };

    Ok(Json(HistoryResponse {
        address: q.address,
        pkh: hex::encode(pkh_arr),
        height,
        txs,
    }))
}

async fn get_fee_estimate(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<FeeEstimateResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    let (min_fee_per_byte, mempool_size) = {
        let mempool = state.mempool.lock().unwrap();
        (mempool.min_fee_per_byte(), mempool.len())
    };
    Ok(Json(FeeEstimateResponse {
        min_fee_per_byte,
        mempool_size,
    }))
}

async fn get_block_template(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<TemplateQuery>,
) -> Result<Json<BlockTemplateResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;

    let longpoll = q.longpoll.unwrap_or(0) == 1;
    let poll_secs = q.poll_secs.unwrap_or(25).max(1).min(120);
    let max_txs = q.max_txs;
    let min_fee = q.min_fee;

    if longpoll {
        let last_tip = {
            let guard = state.chain.lock().unwrap();
            guard
                .chain
                .last()
                .map(|b| hex::encode(b.header.hash()))
                .unwrap_or_else(|| state.genesis_hash.clone())
        };
        let last_mempool = state.mempool.lock().unwrap().len();

        let start = std::time::Instant::now();
        while start.elapsed().as_secs() < poll_secs {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let current_tip = {
                let guard = state.chain.lock().unwrap();
                guard
                    .chain
                    .last()
                    .map(|b| hex::encode(b.header.hash()))
                    .unwrap_or_else(|| state.genesis_hash.clone())
            };
            let current_mempool = state.mempool.lock().unwrap().len();
            if current_tip != last_tip || current_mempool != last_mempool {
                break;
            }
        }
    }

    let (height, prev_hash, bits, target, time) = {
        let guard = state.chain.lock().unwrap();
        let tip = guard.chain.last();
        let prev_hash = tip
            .map(|b| hex::encode(b.header.hash()))
            .unwrap_or_else(|| "00".repeat(32));
        let height = guard.height;
        let target = guard.target_for_height(height);
        let bits = target.bits;
        let prev_time = tip.map(|b| b.header.time).unwrap_or(0);
        let now = Utc::now().timestamp() as u32;
        let time = now.max(prev_time.saturating_add(1));
        (height, prev_hash, bits, target_hex(bits), time)
    };

    let mut mempool_entries = state.mempool.lock().unwrap().ordered_entries();
    if let Some(min_fee) = min_fee {
        mempool_entries.retain(|e| e.fee_per_byte >= min_fee);
    }
    if let Some(max) = max_txs {
        if mempool_entries.len() > max {
            mempool_entries.truncate(max);
        }
    }
    let mempool_count = mempool_entries.len();
    let mut total_fees = 0u64;
    let txs = mempool_entries
        .into_iter()
        .map(|entry| {
            total_fees = total_fees.saturating_add(entry.fee);
            TemplateTx {
                hex: hex::encode(entry.raw),
                fee: entry.fee,
                relay_addresses: entry.relay_addresses,
            }
        })
        .collect();

    let coinbase_value = block_reward(height).saturating_add(total_fees);

    Ok(Json(BlockTemplateResponse {
        height,
        prev_hash,
        bits: format!("{:08x}", bits),
        target,
        time,
        txs,
        total_fees,
        coinbase_value,
        mempool_count,
    }))
}

async fn get_block(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<BlockQuery>,
) -> Result<Json<Value>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    let block_json = {
        let guard = state.chain.lock().unwrap();
        let idx = q.height as usize;
        if idx >= guard.chain.len() {
            return Err(StatusCode::NOT_FOUND);
        }
        let block = &guard.chain[idx];
        let header = &block.header;
        serde_json::json!({
            "height": q.height,
            "header": {
                "version": header.version,
                "prev_hash": hex::encode(header.prev_hash),
                "merkle_root": hex::encode(header.merkle_root),
                "time": header.time,
                "bits": format!("{:08x}", header.bits),
                "nonce": header.nonce,
                "hash": hex::encode(header.hash()),
            },
            "tx_hex": block.transactions.iter().map(|tx| hex::encode(tx.serialize())).collect::<Vec<_>>(),
            "miner_address": miner_address_from_block(block)
        })
    };

    Ok(Json(block_json))
}

fn decode_compact_tx(raw: &[u8]) -> Result<Transaction, String> {
    let mut offset = 0usize;

    let read_u8 = |buf: &[u8], off: &mut usize| -> Result<u8, String> {
        if *off >= buf.len() {
            return Err("unexpected EOF".to_string());
        }
        let v = buf[*off];
        *off += 1;
        Ok(v)
    };
    let read_u32 = |buf: &[u8], off: &mut usize| -> Result<u32, String> {
        if *off + 4 > buf.len() {
            return Err("unexpected EOF".to_string());
        }
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&buf[*off..*off + 4]);
        *off += 4;
        Ok(u32::from_le_bytes(bytes))
    };
    let read_u64 = |buf: &[u8], off: &mut usize| -> Result<u64, String> {
        if *off + 8 > buf.len() {
            return Err("unexpected EOF".to_string());
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&buf[*off..*off + 8]);
        *off += 8;
        Ok(u64::from_le_bytes(bytes))
    };
    let read_bytes = |buf: &[u8], off: &mut usize, len: usize| -> Result<Vec<u8>, String> {
        if *off + len > buf.len() {
            return Err("unexpected EOF".to_string());
        }
        let out = buf[*off..*off + len].to_vec();
        *off += len;
        Ok(out)
    };

    let version = read_u32(raw, &mut offset)?;
    let input_count = read_u8(raw, &mut offset)? as usize;
    let mut inputs = Vec::with_capacity(input_count);
    for _ in 0..input_count {
        let prev_len = read_u8(raw, &mut offset)? as usize;
        let prev_txid_bytes = read_bytes(raw, &mut offset, prev_len)?;
        let mut prev_txid = [0u8; 32];
        if prev_txid_bytes.len() == 32 {
            prev_txid.copy_from_slice(&prev_txid_bytes);
        } else {
            let start = 32 - prev_txid_bytes.len();
            prev_txid[start..].copy_from_slice(&prev_txid_bytes);
        }
        let prev_index = read_u32(raw, &mut offset)?;
        let script_sig_len = read_u8(raw, &mut offset)? as usize;
        let script_sig = read_bytes(raw, &mut offset, script_sig_len)?;
        let sequence = read_u32(raw, &mut offset)?;
        inputs.push(TxInput {
            prev_txid,
            prev_index,
            script_sig,
            sequence,
        });
    }

    let output_count = read_u8(raw, &mut offset)? as usize;
    let mut outputs = Vec::with_capacity(output_count);
    for _ in 0..output_count {
        let value = read_u64(raw, &mut offset)?;
        let script_len = read_u8(raw, &mut offset)? as usize;
        let script_pubkey = read_bytes(raw, &mut offset, script_len)?;
        outputs.push(TxOutput {
            value,
            script_pubkey,
        });
    }

    let locktime = read_u32(raw, &mut offset)?;

    Ok(Transaction {
        version,
        inputs,
        outputs,
        locktime,
    })
}

fn target_hex(bits: u32) -> String {
    let target = Target { bits }.to_target();
    let mut bytes = target.to_bytes_be();
    if bytes.len() < 32 {
        let mut padded = vec![0u8; 32 - bytes.len()];
        padded.extend_from_slice(&bytes);
        bytes = padded;
    }
    hex::encode(bytes)
}

fn parse_header_bits(bits_str: &str) -> Result<u32, String> {
    let trimmed = bits_str.trim_start_matches("0x");
    u32::from_str_radix(trimmed, 16).map_err(|e| format!("invalid bits field: {e}"))
}

async fn submit_block(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<SubmitBlockRequest>,
) -> Result<Json<Value>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;
    // Rebuild header from JSON.
    let header = &req.header;

    let prev_bytes = hex::decode(&header.prev_hash).map_err(|_| StatusCode::BAD_REQUEST)?;
    let merkle_bytes = hex::decode(&header.merkle_root).map_err(|_| StatusCode::BAD_REQUEST)?;
    let hash_bytes = hex::decode(&header.hash).map_err(|_| StatusCode::BAD_REQUEST)?;
    if prev_bytes.len() != 32 || merkle_bytes.len() != 32 || hash_bytes.len() != 32 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let bits = parse_header_bits(&header.bits).map_err(|_| StatusCode::BAD_REQUEST)?;

    let mut prev_hash = [0u8; 32];
    prev_hash.copy_from_slice(&prev_bytes);
    let mut merkle_root = [0u8; 32];
    merkle_root.copy_from_slice(&merkle_bytes);

    let block_header = BlockHeader {
        version: header.version,
        prev_hash,
        merkle_root,
        time: header.time,
        bits,
        nonce: header.nonce,
    };

    // Sanity-check header hash matches payload.
    let derived_hash = block_header.hash();
    if derived_hash[..] != hash_bytes[..] {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Decode full transactions from hex payload.
    let mut txs: Vec<Transaction> = Vec::with_capacity(req.tx_hex.len());
    for tx_hex in &req.tx_hex {
        let raw = hex::decode(tx_hex).map_err(|_| StatusCode::BAD_REQUEST)?;
        let tx = decode_full_tx(&raw).map_err(|_| StatusCode::BAD_REQUEST)?;
        txs.push(tx);
    }

    if txs.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let block = Block {
        header: block_header,
        transactions: txs,
    };

    // Apply to chain state under lock, enforcing consensus rules.
    let (new_height, new_tip_hash) = {
        let mut chain = state.chain.lock().unwrap();

        // Height must match the next expected block height.
        if req.height != chain.height {
            return Err(StatusCode::BAD_REQUEST);
        }

        if let Err(_e) = chain.connect_block(block.clone()) {
            return Err(StatusCode::BAD_REQUEST);
        }

        let tip_hash = block.header.hash();
        (chain.tip_height(), hex::encode(tip_hash))
    };

    // If anchors are loaded, enforce anchor consistency on the new tip.
    if let Some(ref anchors) = state.anchors {
        if !anchors.is_chain_valid(new_height, &new_tip_hash) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    // Persist JSON representation alongside miner-written blocks.
    if let Err(_e) = storage::write_block_json(req.height, &block) {
        // The block is already in memory; surface a server error if disk write fails.
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Remove any included transactions from the HTTP mempool.
    {
        let mut mempool = state.mempool.lock().unwrap();
        for tx in block.transactions.iter().skip(1) {
            let txid = tx.txid();
            mempool.remove(&txid);
        }
    }

    // Broadcast the newly accepted block over P2P if enabled.
    if let Some(ref p2p) = state.p2p {
        let mut bytes = Vec::new();
        // Serialize header + transactions using the canonical Rust format.
        //
        // For now we reuse Transaction::serialize() and BlockHeader::serialize()
        // and simply concatenate them; remote peers can interpret this as needed.
        bytes.extend_from_slice(&block.header.serialize());
        for tx in &block.transactions {
            bytes.extend_from_slice(&tx.serialize());
        }
        if let Err(e) = p2p.broadcast_block(&bytes).await {
            eprintln!("Failed to broadcast accepted block over P2P: {}", e);
        }
    }

    Ok(Json(json!({
        "accepted": true,
        "height": req.height,
        "hash": header.hash,
    })))
}

async fn submit_tx(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<SubmitTxRequest>,
) -> Result<Json<SubmitTxResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;
    let bytes = match hex::decode(&req.tx_hex) {
        Ok(b) => b,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };
    let tx = decode_compact_tx(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    if tx.inputs.is_empty() || tx.outputs.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let txid = tx.txid();

    // Delegate validation to ChainState and compute fees.
    let fee = {
        let chain = state.chain.lock().unwrap();
        match chain.calculate_fees(&tx) {
            Ok(f) => f,
            Err(_) => return Err(StatusCode::BAD_REQUEST),
        }
    };

    let mut mempool = state.mempool.lock().unwrap();
    let hex_txid = hex::encode(txid);
    if mempool.contains(&txid) {
        return Ok(Json(SubmitTxResponse {
            txid: hex_txid,
            accepted: false,
        }));
    }

    let raw = bytes;
    if let Err(e) = mempool.add_transaction(tx, raw, fee) {
        eprintln!("Failed to add tx to mempool: {}", e);
        return Ok(Json(SubmitTxResponse {
            txid: hex_txid,
            accepted: false,
        }));
    }

    Ok(Json(SubmitTxResponse {
        txid: hex_txid,
        accepted: true,
    }))
}

#[tokio::main]
async fn main() {
    // Initialize chain state with locked genesis.
    let locked = load_locked_genesis().expect("load locked genesis");
    let block = block_from_locked(&locked);
    let pow_limit = Target { bits: 0x1d00_ffff };
    let params = ChainParams {
        genesis_block: block,
        pow_limit,
    };
    let mut state = ChainState::new(params);
    load_persisted_blocks(&mut state);
    let shared_state = Arc::new(Mutex::new(state));
    let genesis_hash = locked.header.hash.clone();
    let mempool = Arc::new(Mutex::new(MempoolManager::new(mempool_file(), 1000, 1.0)));
    let limiter = Arc::new(Mutex::new(rate_limiter()));

    // Attempt to load anchors from the repo root if present. On mainnet,
    // the anchors file is shipped and verified out-of-band.
    let anchors = match AnchorManager::from_default_repo_root(PathBuf::from(".")) {
        Ok(a) => Some(a),
        Err(e) => {
            eprintln!("Failed to load anchors: {}", e);
            std::process::exit(1);
        }
    };
    if let Some(a) = anchors.clone() {
        let mut guard = shared_state.lock().unwrap();
        guard.set_anchors(a);
    }

    // Optional node configuration from JSON file, e.g. configs/node.json.
    let node_cfg: Option<NodeConfig> = std::env::var("IRIUM_NODE_CONFIG")
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str::<NodeConfig>(&raw).ok());

    // Enforce anchor consistency if anchors are present
    if let Some(ref a) = anchors {
        if let Some(latest) = a.get_latest_anchor() {
            let expected = latest.hash.to_lowercase();
            let tip_hash = genesis_hash.to_lowercase();
            if latest.height <= 1 && expected != tip_hash {
                panic!(
                    "Anchors mismatch: latest anchor hash {} != genesis hash {}",
                    expected, tip_hash
                );
            }
        }
    }

    // Validate anchors against genesis if available.
    if let Some(ref a) = anchors {
        if let Some(latest) = a.get_latest_anchor() {
            if latest.height <= 1 && latest.hash.to_lowercase() != genesis_hash.to_lowercase() {
                panic!(
                    "Anchors mismatch: latest anchor hash {} != genesis hash {}",
                    latest.hash, genesis_hash
                );
            }
        }
    }

    let agent_string =
        std::env::var("IRIUM_NODE_AGENT").unwrap_or_else(|_| "Irium-Node".to_string());
    let relay_address = node_cfg
        .as_ref()
        .and_then(|c| c.relay_address.clone())
        .or_else(|| std::env::var("IRIUM_RELAY_ADDRESS").ok());

    // Set up P2P node if configured.
    let p2p: Option<P2PNode> = if let Some(ref cfg) = node_cfg {
        if let Some(bind) = &cfg.p2p_bind {
            match bind.parse::<SocketAddr>() {
                Ok(addr) => {
                    let node = P2PNode::new(
                        addr,
                        agent_string.clone(),
                        Some(shared_state.clone()),
                        Some(mempool.clone()),
                        relay_address.clone(),
                    );
                    // Start listener in the background.
                    if let Err(e) = node.start().await {
                        eprintln!("Failed to start P2P listener on {}: {}", addr, e);
                        None
                    } else {
                        Some(node)
                    }
                }
                Err(e) => {
                    eprintln!("Invalid P2P bind address {}: {}", bind, e);
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // Build seed list: merge config, signed, and runtime seeds; filter locals.
    let default_seed_port: u16 = node_cfg
        .as_ref()
        .and_then(|cfg| cfg.p2p_bind.as_ref())
        .and_then(|b| b.split(":").last())
        .and_then(|p| p.parse().ok())
        .unwrap_or(38291);

    let config_seeds: Vec<String> = node_cfg
        .as_ref()
        .map(|cfg| cfg.p2p_seeds.clone())
        .unwrap_or_default();
    let signed_seeds = load_signed_seeds();
    let local_ips = local_ip_set(node_cfg.as_ref().and_then(|cfg| cfg.p2p_bind.as_ref()));

    // Connect to seed peers using a basic handshake and keep retrying in background.
    if let Some(node) = p2p.clone() {
        let config_seeds = config_seeds.clone();
        let signed_seeds = signed_seeds.clone();
        let local_ips = local_ips.clone();
        let agent_clone = agent_string.clone();
        let shared_clone = shared_state.clone();
        tokio::spawn(async move {
            let node = node;
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            let mut no_seed_logged = false;
            loop {
                let (seeds, seed_info) = build_seed_addrs(&config_seeds, &signed_seeds, default_seed_port, &local_ips);
                if seeds.is_empty() {
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        node.connect_known_peers(5),
                    )
                    .await;
                    if !no_seed_logged {
                        if seed_info.total > 0 && seed_info.filtered_local == seed_info.total {
                            // All seeds are local; wait for inbound peers quietly.
                        } else {
                            println!(
                                "[{}] no seeds configured; trying peer cache",
                                Utc::now().format("%H:%M:%S")
                            );
                        }
                        no_seed_logged = true;
                    }
                    interval.tick().await;
                    continue;
                }
                no_seed_logged = false;
                for addr in &seeds {
                    if node.is_connected(addr).await {
                        continue;
                    }
                    if node.is_self_ip(addr.ip()).await {
                        continue;
                    }
                    if node.is_ip_connected(addr.ip()).await {
                        continue;
                    }
                    let height = {
                        let chain = shared_clone.lock().unwrap();
                        chain.tip_height()
                    };
                    println!(
                        "[{}] dialing seed {} (h={})",
                        Utc::now().format("%H:%M:%S"),
                        addr,
                        height
                    );
                    if let Err(e) = node
                        .connect_and_handshake(*addr, height, &agent_clone)
                        .await
                    {
                        eprintln!(
                            "[{}] outbound {} failed: {}",
                            Utc::now().format("%H:%M:%S"),
                            addr,
                            e
                        );
                    }
                }
                interval.tick().await;
            }
        });
    }

    // Periodic heartbeat logging to surface peers and seedlist.
    if let Some(ref node) = p2p {
        let node_clone = node.clone();
        let chain_clone = shared_state.clone();
        let mempool_clone = mempool.clone();
        let genesis_hex = genesis_hash.clone();
        tokio::spawn(async move {
            let seed_mgr = SeedlistManager::new(128);
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let peers = node_clone.peers_snapshot().await;
                node_clone.refresh_seedlist().await;
                let _ = tokio::time::timeout(std::time::Duration::from_secs(2), node_clone.connect_known_peers(3)).await;
                let seeds = seed_mgr.merged_seedlist();

                let mut peer_ips = std::collections::HashSet::new();
                let mut peer_list: Vec<String> = Vec::new();
                for p in peers.iter() {
                    let parts: Vec<&str> = p.multiaddr.split("/").collect();
                    if parts.len() >= 5 {
                        let ip = parts[2];
                        if peer_ips.insert(ip.to_string()) {
                            let label = p.agent.clone().unwrap_or_else(|| "peer".to_string());
                            peer_list.push(label);
                        }
                    } else if peer_ips.insert(p.multiaddr.clone()) {
                            let label = p.agent.clone().unwrap_or_else(|| "peer".to_string());
                            peer_list.push(label);
                    }
                }
                if peer_list.is_empty() {
                    peer_list.push("-".to_string());
                }

                let best_peer_height = peers.iter().filter_map(|p| p.last_height).max();

                let mut seed_ips = std::collections::HashSet::new();
                let mut seed_list: Vec<String> = Vec::new();
                for s in seeds.iter() {
                    let parts: Vec<&str> = s.split('/').collect();
                    if parts.len() >= 5 {
                        let ip = parts[2];
                        if seed_ips.insert(ip.to_string()) {
                            seed_list.push(mask_seed_label(ip));
                        }
                    } else if seed_ips.insert(s.clone()) {
                        seed_list.push(mask_seed_label(s));
                    }
                }
                if seed_list.is_empty() {
                    seed_list.push("-".to_string());
                }

                let (local_height, tip_hash, mempool_size) = {
                    let g = match chain_clone.lock() {
                        Ok(g) => g,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    let tip = g
                        .chain
                        .last()
                        .map(|b| hex::encode(b.header.hash()))
                        .unwrap_or_else(|| genesis_hex.clone());
                    let mem_sz = match mempool_clone.lock() {
                        Ok(g) => g.len(),
                        Err(poisoned) => poisoned.into_inner().len(),
                    };
                    (g.tip_height(), tip, mem_sz)
                };
                let next_height = local_height.saturating_add(1);
                let peer_height = best_peer_height.unwrap_or(0);
                let chain_height = std::cmp::max(local_height, peer_height);

                let peer_sample = peer_list
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                let seed_count = seed_list.len();

                if json_log_enabled() {
                    println!(
                        "{}",
                        json!({
                            "ts": Utc::now().format("%H:%M:%S").to_string(),
                            "level": "info",
                            "event": "heartbeat",
                            "height": local_height,
                            "local_height": local_height,
                            "chain_height": chain_height,
                            "peer_height": peer_height,
                            "next_height": next_height,
                            "peers": peer_ips.len(),
                            "peer_sample": peer_sample,
                            "seed_count": seed_count,
                            "agent": std::env::var("IRIUM_NODE_AGENT").unwrap_or_else(|_| "Irium-Node".to_string()),
                            "tip": tip_hash,
                            "mempool": mempool_size,
                        })
                    );
                } else {
                    let short_tip = tip_hash.chars().take(12).collect::<String>();
                    println!(
                        "[{}] ❤️ heartbeat Irium chain height={} 🏠 local height={} 🧱 next height={} ⛏ tip={} 👥 peers={} 🌱 seedlist={} 🧺 mempool={}",
                        Utc::now().format("%H:%M:%S"),
                        chain_height,
                        local_height,
                        next_height,
                        short_tip,
                        peer_ips.len(),
                        seed_count,
                        mempool_size
                    );
                }

            }
        });
    }

    let app_state = AppState {
        chain: shared_state.clone(),
        genesis_hash: genesis_hash.clone(),
        mempool: mempool.clone(),
        anchors,
        p2p,
        limiter: limiter.clone(),
    };

    let app = Router::new()
        .route("/status", get(status))
        .route("/peers", get(peers))
        .route("/metrics", get(metrics))
        .route("/rpc/balance", get(get_balance))
        .route("/rpc/utxos", get(get_utxos))
        .route("/rpc/history", get(get_history))
        .route("/rpc/fee_estimate", get(get_fee_estimate))
        .route("/rpc/utxo", get(get_utxo))
        .route("/rpc/getblocktemplate", get(get_block_template))
        .route("/rpc/block", get(get_block))
        .route("/rpc/submit_block", post(submit_block))
        .route("/rpc/submit_tx", post(submit_tx))
        .layer(DefaultBodyLimit::max(rpc_body_limit_bytes()))
        .with_state(app_state)
        .into_make_service_with_connect_info::<SocketAddr>();

    let host = std::env::var("IRIUM_NODE_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port: u16 = std::env::var("IRIUM_NODE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(38300);

    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .expect("valid bind address");

    let tls_cert = std::env::var("IRIUM_TLS_CERT").ok();
    let tls_key = std::env::var("IRIUM_TLS_KEY").ok();
    if let (Some(cert_path), Some(key_path)) = (tls_cert, tls_key) {
        let config = RustlsConfig::from_pem_file(cert_path, key_path)
            .await
            .expect("failed to load TLS cert/key");
        if json_log_enabled() {
            println!(
                "{}",
                json!({"ts": Utc::now().format("%H:%M:%S").to_string(), "level": "info", "event": "http_listen", "host": host, "port": port, "scheme": "https"})
            );
        } else {
            println!("Irium Rust node HTTPS listening on https://{}:{}", host, port);
        }
        axum_server::bind_rustls(addr, config)
            .serve(app)
            .await
            .expect("server error");
    } else {
        if json_log_enabled() {
            println!(
                "{}",
                json!({"ts": Utc::now().format("%H:%M:%S").to_string(), "level": "info", "event": "http_listen", "host": host, "port": port, "scheme": "http"})
            );
        } else {
            println!("Irium Rust node HTTP listening on http://{}:{}", host, port);
        }

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .expect("bind failed");

        axum::serve(listener, app).await.expect("server error");
    }
}
