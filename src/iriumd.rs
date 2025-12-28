use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::{env, fs};

use chrono::Utc;
use axum::{
    extract::{ConnectInfo, Json as AxumJson, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use irium_node_rs::anchors::AnchorManager;
use irium_node_rs::block::{Block, BlockHeader};
use irium_node_rs::chain::{block_from_locked, ChainParams, ChainState, OutPoint};
use irium_node_rs::genesis::load_locked_genesis;
use irium_node_rs::mempool::MempoolManager;
use irium_node_rs::network::SeedlistManager;
use irium_node_rs::p2p::P2PNode;
use irium_node_rs::pow::Target;
use irium_node_rs::rate_limiter::RateLimiter;
use irium_node_rs::tx::{decode_full_tx, Transaction, TxInput, TxOutput};

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
}

#[derive(Serialize)]
struct UtxoResponse {
    value: u64,
    height: u64,
    is_coinbase: bool,
}

#[derive(Deserialize)]
struct UtxoQuery {
    txid: String,
    index: u32,
}

#[derive(Deserialize)]
struct BlockQuery {
    height: u64,
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

fn json_log_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| std::env::var("IRIUM_JSON_LOG").ok().map(|v| v == "1" || v.to_lowercase() == "true").unwrap_or(false))
}

fn blocks_dir() -> PathBuf {
    if let Ok(dir) = env::var("IRIUM_BLOCKS_DIR") {
        PathBuf::from(dir)
    } else {
        let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
        PathBuf::from(home).join(".irium/blocks")
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
    let peer_count = match state.p2p {
        Some(ref p2p) => p2p.peer_count().await,
        None => 0,
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
    }))
}

async fn peers(State(state): State<AppState>) -> Result<Json<PeersResponse>, StatusCode> {
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

async fn metrics(State(state): State<AppState>) -> String {
    let (height, anchor_loaded, tip_hash) = {
        let g = state.chain.lock().unwrap();
        let tip_hash = g
            .chain
            .last()
            .map(|b| hex::encode(b.header.hash()))
            .unwrap_or_else(|| state.genesis_hash.clone());
        (g.tip_height(), state.anchors.is_some(), tip_hash)
    };
    let peer_count = match state.p2p {
        Some(ref p2p) => p2p.peer_count().await,
        None => 0,
    };
    let mempool_sz = state.mempool.lock().unwrap().len();
    format!("irium_height {}
irium_peers {}
irium_anchor_loaded {}
irium_tip_hash {}
irium_mempool_size {}
",
        height,
        peer_count,
        anchor_loaded as u8,
        tip_hash,
        mempool_sz)
}

async fn get_utxo(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    Query(q): Query<UtxoQuery>,
) -> Result<Json<UtxoResponse>, StatusCode> {
    check_rate(&state, &addr)?;
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

async fn get_block(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    Query(q): Query<BlockQuery>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr)?;
    let dir = blocks_dir();
    let path = dir.join(format!("block_{}.json", q.height));
    let data = fs::read_to_string(&path).map_err(|_| StatusCode::NOT_FOUND)?;
    let v: Value = serde_json::from_str(&data).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(v))
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

#[derive(Serialize)]
struct JsonHeader {
    version: u32,
    prev_hash: String,
    merkle_root: String,
    time: u32,
    bits: String,
    nonce: u32,
    hash: String,
}

#[derive(Serialize)]
struct JsonBlock {
    height: u64,
    header: JsonHeader,
    tx_hex: Vec<String>,
}

fn write_block_json(height: u64, block: &Block) -> std::io::Result<()> {
    let dir = blocks_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("block_{}.json", height));

    let header = &block.header;
    let hash = header.hash();

    let jb = JsonBlock {
        height,
        header: JsonHeader {
            version: header.version,
            prev_hash: hex::encode(header.prev_hash),
            merkle_root: hex::encode(header.merkle_root),
            time: header.time,
            bits: format!("{:08x}", header.bits),
            nonce: header.nonce,
            hash: hex::encode(hash),
        },
        tx_hex: block
            .transactions
            .iter()
            .map(|tx| hex::encode(tx.serialize()))
            .collect(),
    };

    let json = serde_json::to_string_pretty(&jb)?;
    fs::write(path, json)
}

fn parse_header_bits(bits_str: &str) -> Result<u32, String> {
    let trimmed = bits_str.trim_start_matches("0x");
    u32::from_str_radix(trimmed, 16).map_err(|e| format!("invalid bits field: {e}"))
}

async fn submit_block(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    AxumJson(req): AxumJson<SubmitBlockRequest>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr)?;
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
    if let Err(_e) = write_block_json(req.height, &block) {
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
    AxumJson(req): AxumJson<SubmitTxRequest>,
) -> Result<Json<SubmitTxResponse>, StatusCode> {
    check_rate(&state, &addr)?;
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
    if let Err(e) = mempool.add_transaction(tx.clone(), raw.clone(), fee) {
        eprintln!("Failed to add tx to mempool: {}", e);
        return Ok(Json(SubmitTxResponse {
            txid: hex_txid,
            accepted: false,
        }));
    }
    drop(mempool);

    // Broadcast to peers if P2P is enabled.
    if let Some(ref p2p) = state.p2p {
        let _ = p2p.broadcast_tx(&raw).await;
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
    let state = ChainState::new(params);
    let shared_state = Arc::new(Mutex::new(state));
    let genesis_hash = locked.header.hash.clone();
    let mempool = Arc::new(Mutex::new(MempoolManager::new(mempool_file(), 1000, 1.0)));
    let limiter = Arc::new(Mutex::new(rate_limiter()));

    // Attempt to load anchors from the repo root if present. On mainnet,
    // the anchors file is shipped and verified out-of-band.
    let anchors = AnchorManager::from_default_repo_root(PathBuf::from(".")).ok();

    // Optional node configuration from JSON file, e.g. configs/node.json.
    let node_cfg: Option<NodeConfig> = std::env::var("IRIUM_NODE_CONFIG")
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str::<NodeConfig>(&raw).ok());
    let relay_address = node_cfg
        .as_ref()
        .and_then(|c| c.relay_address.clone())
        .or_else(|| std::env::var("IRIUM_RELAY_ADDRESS").ok());

        // Validate anchors against genesis if available.
    if let Some(ref a) = anchors {
        if let Some(latest) = a.get_latest_anchor() {
            if latest.height <= 1 && latest.hash.to_lowercase() != genesis_hash.to_lowercase() {
                panic!("Anchors mismatch: latest anchor hash {} != genesis hash {}", latest.hash, genesis_hash);
            }
        }
    }

    let agent_string = std::env::var("IRIUM_NODE_AGENT").unwrap_or_else(|_| "Irium-Node".to_string());

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

    // Connect to configured seed peers using a basic handshake.
    if let (Some(ref node), Some(ref cfg)) = (&p2p, &node_cfg) {
        for seed in &cfg.p2p_seeds {
            let addr: SocketAddr = match seed.parse() {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("Invalid P2P seed address {}: {}", seed, e);
                    continue;
                }
            };

            let height = {
                let chain = shared_state.lock().unwrap();
                chain.tip_height()
            };

            if let Err(e) = node
                .connect_and_handshake(addr, height, &agent_string)
                .await
            {
                eprintln!("Failed outbound P2P handshake with {}: {}", addr, e);
            }
        }
    }

    // Periodic heartbeat logging to surface peers and seedlist.
    if let Some(ref node) = p2p {
        let node_clone = node.clone();
        let chain_clone = shared_state.clone();
        tokio::spawn(async move {
            let seed_mgr = SeedlistManager::new(128);
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let peers = node_clone.peers_snapshot().await;
                let seeds = seed_mgr.merged_seedlist();

                let mut peer_ips = std::collections::HashSet::new();
                let mut peer_list: Vec<String> = Vec::new();
                for p in peers.iter() {
                    let parts: Vec<&str> = p.multiaddr.split('/').collect();
                    if parts.len() >= 5 {
                        let ip = parts[2];
                        let port = parts[4];
                        if peer_ips.insert(ip.to_string()) {
                            let h = p.last_height.map(|v| format!("h={}", v)).unwrap_or_else(|| "h=-".to_string());
                            let label = match &p.agent {
                                Some(agent) => format!("{}:{} ({} {})", ip, port, h, agent),
                                None => format!("{}:{} ({})", ip, port, h),
                            };
                            peer_list.push(label);
                        }
                    } else if peer_ips.insert(p.multiaddr.clone()) {
                        let h = p.last_height.map(|v| format!("h={}", v)).unwrap_or_else(|| "h=-".to_string());
                        peer_list.push(format!("{} ({})", p.multiaddr, h));
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
                    if parts.len() >= 3 {
                        let ip = parts[2];
                        if seed_ips.insert(ip.to_string()) {
                            seed_list.push(ip.to_string());
                        }
                    } else if let Some(pos) = s.find(":") {
                        let ip = &s[..pos];
                        if seed_ips.insert(ip.to_string()) {
                            seed_list.push(ip.to_string());
                        }
                    } else if seed_ips.insert(s.clone()) {
                        seed_list.push(s.clone());
                    }
                }
                if seed_list.is_empty() {
                    seed_list.push("-".to_string());
                }

                let local_height = {
                    let g = chain_clone.lock().unwrap();
                    g.tip_height()
                };
                let chain_height = best_peer_height.unwrap_or(local_height);

                let peer_sample = peer_list.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
                let seed_sample = seed_list.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
                if json_log_enabled() {
                    println!("{}", json!({"ts": Utc::now().format("%H:%M:%S").to_string(), "level": "info", "event": "heartbeat", "peers": peer_ips.len(), "peer_sample": peer_sample, "seeds": seed_sample, "height": local_height, "local_height": local_height, "chain_height": chain_height}));
                } else {
                    println!("[{}] 🔁 heartbeat Irium chain height={} local height={} peers={} [{}] seeds=[{}]",                        Utc::now().format("%H:%M:%S"),                        chain_height,                        local_height,                        peer_ips.len(),                        peer_sample,                        seed_sample                    );
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
        .route("/rpc/utxo", get(get_utxo))
        .route("/rpc/block", get(get_block))
        .route("/rpc/submit_block", post(submit_block))
        .route("/rpc/submit_tx", post(submit_tx))
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

    if json_log_enabled() {
        println!("{}", json!({"ts": Utc::now().format("%H:%M:%S").to_string(), "level": "info", "event": "http_listen", "host": host, "port": port}));
    } else {
        println!("Irium Rust node HTTP listening on http://{}:{}", host, port);
    }

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind failed");

    axum::serve(listener, app).await.expect("server error");
}
