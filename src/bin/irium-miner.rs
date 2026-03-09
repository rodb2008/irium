use reqwest::blocking::Client;
use reqwest::Certificate;
use reqwest::StatusCode;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};
use std::{env, fs, sync::OnceLock};

use bs58;
use chrono::Utc;
use num_bigint::BigUint;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use irium_node_rs::anchors::AnchorManager;
use irium_node_rs::block::{Block, BlockHeader};
use irium_node_rs::chain::{block_from_locked, decode_compact_tx, ChainParams, ChainState};
use irium_node_rs::constants::block_reward;
use irium_node_rs::genesis::load_locked_genesis;
use irium_node_rs::mempool::MempoolManager;
use irium_node_rs::pow::{meets_target, sha256d, Target};
use irium_node_rs::relay::RelayCommitment;
use irium_node_rs::tx::{Transaction, TxInput, TxOutput};

fn load_env_file(path: &str) -> bool {
    let contents = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(_) => return false,
    };
    for raw in contents.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || env::var_os(key).is_some() {
            continue;
        }
        let mut val = value.trim().to_string();
        if (val.starts_with('"') && val.ends_with('"'))
            || (val.starts_with('\'') && val.ends_with('\''))
        {
            val = val[1..val.len() - 1].to_string();
        }
        env::set_var(key, val);
    }
    true
}

fn rpc_token() -> Option<String> {
    env::var("IRIUM_RPC_TOKEN")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn htlcv1_activation_height() -> Option<u64> {
    env::var("IRIUM_HTLCV1_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
}

fn rpc_status_error(prefix: &str, status: StatusCode) -> String {
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        format!("{}: HTTP {} (check IRIUM_RPC_TOKEN)", prefix, status)
    } else {
        format!("{}: HTTP {}", prefix, status)
    }
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

fn blocks_dir() -> PathBuf {
    if let Ok(dir) = env::var("IRIUM_BLOCKS_DIR") {
        PathBuf::from(dir)
    } else {
        let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
        PathBuf::from(home).join(".irium/miner/blocks")
    }
}

fn prune_blocks_above(height: u64) {
    let dir = blocks_dir();
    if !dir.exists() {
        return;
    }
    let read_dir = match dir.read_dir() {
        Ok(r) => r,
        Err(_) => return,
    };
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
            let _ = fs::remove_file(&path);
        }
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

fn base58_p2pkh_to_hash(addr: &str) -> Option<Vec<u8>> {
    let data = bs58::decode(addr).into_vec().ok()?;
    if data.len() < 25 {
        return None;
    }
    let (body, checksum) = data.split_at(data.len() - 4);
    let mut hasher = Sha256::new();
    hasher.update(body);
    let first = hasher.finalize_reset();
    hasher.update(first);
    let second = hasher.finalize();
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

fn script_from_relay_address(addr: &str) -> Result<Vec<u8>, String> {
    // Try hex-encoded 20-byte pubkey hash (P2PKH).
    if addr.len() == 40 {
        if let Ok(pkh) = hex::decode(addr) {
            if pkh.len() == 20 {
                let mut s = Vec::with_capacity(25);
                s.push(0x76); // OP_DUP
                s.push(0xa9); // OP_HASH160
                s.push(0x14); // push 20
                s.extend_from_slice(&pkh);
                s.push(0x88); // OP_EQUALVERIFY
                s.push(0xac); // OP_CHECKSIG
                return Ok(s);
            }
        }
    }

    // Fallback: OP_RETURN marker carrying the address string (truncated if needed).
    let data = addr.as_bytes();
    if data.len() > 75 {
        return Err("Relay address too long for OP_RETURN marker".to_string());
    }
    let mut script = Vec::with_capacity(2 + data.len());
    script.push(0x6a); // OP_RETURN
    script.push(data.len() as u8);
    script.extend_from_slice(data);
    Ok(script)
}

fn op_return_output(data: &[u8]) -> TxOutput {
    let mut script = Vec::with_capacity(2 + data.len());
    script.push(0x6a); // OP_RETURN
    script.push(data.len() as u8);
    script.extend_from_slice(data);
    TxOutput {
        value: 0,
        script_pubkey: script,
    }
}

fn coinbase_metadata_output() -> Option<TxOutput> {
    let raw = std::env::var("IRIUM_COINBASE_METADATA")
        .ok()
        .or_else(|| std::env::var("IRIUM_NOTARY_HASH").ok())?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let hex_hash = if raw.len() == 64 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
        raw.to_lowercase()
    } else {
        let mut hasher = Sha256::new();
        hasher.update(raw.as_bytes());
        let digest = hasher.finalize();
        hex::encode(digest)
    };
    let payload = format!("notary:{}", hex_hash);
    let bytes = payload.as_bytes();
    if bytes.len() > 75 {
        return None;
    }
    Some(op_return_output(bytes))
}

#[cfg(test)]
mod tests {
    use super::{htlcv1_activation_height, script_from_relay_address};

    #[test]
    fn builds_p2pkh_from_hex() {
        let script = script_from_relay_address("00".repeat(20).as_str()).unwrap();
        // OP_DUP OP_HASH160 push20 <pkh> OP_EQUALVERIFY OP_CHECKSIG
        assert_eq!(script.len(), 25);
        assert_eq!(script[0], 0x76);
        assert_eq!(script[1], 0xa9);
        assert_eq!(script[2], 0x14);
        assert_eq!(script[23], 0x88);
        assert_eq!(script[24], 0xac);
    }

    #[test]
    fn builds_op_return_for_other() {
        let script = script_from_relay_address("relay-address").unwrap();
        assert!(script.starts_with(&[0x6a])); // OP_RETURN
    }

    #[test]
    fn reads_htlcv1_activation_height_from_env() {
        std::env::set_var("IRIUM_HTLCV1_ACTIVATION_HEIGHT", "42");
        assert_eq!(htlcv1_activation_height(), Some(42));
        std::env::remove_var("IRIUM_HTLCV1_ACTIVATION_HEIGHT");
    }

}

fn miner_address_info() -> Option<(String, Vec<u8>)> {
    if let Ok(addr) = env::var("IRIUM_MINER_ADDRESS") {
        if let Some(pkh) = base58_p2pkh_to_hash(&addr) {
            return Some((addr, pkh));
        }
    }

    if let Ok(addr) = env::var("IRIUM_RELAY_ADDRESS") {
        if let Some(pkh) = base58_p2pkh_to_hash(&addr) {
            return Some((addr, pkh));
        }
    }

    if let Ok(hex) = env::var("IRIUM_MINER_PKH") {
        if hex.len() == 40 {
            if let Ok(pkh) = hex::decode(&hex) {
                return Some((format!("pkh:{hex}"), pkh));
            }
        }
    }

    None
}

fn miner_pubkey_hash() -> Option<Vec<u8>> {
    miner_address_info().map(|(_, pkh)| pkh)
}

fn build_coinbase(height: u64, reward: u64) -> Transaction {
    let coinbase_input = TxInput {
        prev_txid: [0u8; 32],
        prev_index: 0xffff_ffff,
        script_sig: format!("Block {}", height).into_bytes(),
        sequence: 0xffff_ffff,
    };

    let script_pubkey = if let Some(pkh) = miner_pubkey_hash() {
        // P2PKH: OP_DUP OP_HASH160 0x14 <20-byte-pkh> OP_EQUALVERIFY OP_CHECKSIG
        let mut s = Vec::with_capacity(25);
        s.push(0x76);
        s.push(0xa9);
        s.push(0x14);
        s.extend_from_slice(&pkh);
        s.push(0x88);
        s.push(0xac);
        s
    } else {
        // Fallback: empty script (unspendable reward)
        Vec::new()
    };

    let coinbase_output = TxOutput {
        value: reward,
        script_pubkey,
    };

    Transaction {
        version: 1,
        inputs: vec![coinbase_input],
        outputs: vec![coinbase_output],
        locktime: 0,
    }
}

#[derive(Deserialize)]
struct LegacyMempoolEntry {
    hex: String,
}

#[derive(Deserialize)]
struct TemplateTx {
    hex: String,
    fee: Option<u64>,
    relay_addresses: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct BlockTemplate {
    height: u64,
    prev_hash: String,
    bits: String,
    time: u32,
    txs: Vec<TemplateTx>,
}

#[derive(Deserialize)]
struct PeerInfo {
    height: Option<u64>,
}

#[derive(Deserialize)]
struct PeersResponse {
    peers: Vec<PeerInfo>,
}

/// Load mempool transactions, accepting either the new structured mempool
/// file or the legacy hex-only format.
fn mempool_entries_from_template(
    chain: &ChainState,
    template: &BlockTemplate,
) -> Vec<irium_node_rs::mempool::MempoolEntry> {
    let mut out = Vec::new();
    for tx in &template.txs {
        let raw = match hex::decode(&tx.hex) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Invalid template tx hex: {e}");
                continue;
            }
        };
        let tx_obj = decode_compact_tx(&raw);
        if let Err(e) = chain.validate_transaction(&tx_obj) {
            eprintln!("Skipping invalid template tx: {e}");
            continue;
        }
        let fee = tx.fee.unwrap_or(0);
        let size = raw.len();
        let fee_per_byte = if size > 0 {
            fee as f64 / size as f64
        } else {
            0.0
        };
        out.push(irium_node_rs::mempool::MempoolEntry {
            tx: tx_obj,
            raw,
            fee,
            size,
            fee_per_byte,
            added: 0,
            relays: Vec::new(),
            relay_addresses: tx.relay_addresses.clone().unwrap_or_default(),
        });
    }
    out
}

fn load_mempool_entries(
    chain: &ChainState,
    template: Option<&BlockTemplate>,
) -> Vec<irium_node_rs::mempool::MempoolEntry> {
    if let Some(template) = template {
        return mempool_entries_from_template(chain, template);
    }
    // First try the structured mempool manager.
    let mgr = MempoolManager::new(mempool_file(), 1000, 1.0);
    let mut out = Vec::new();
    for entry in mgr.ordered_entries() {
        if let Err(e) = chain.validate_transaction(&entry.tx) {
            eprintln!("Skipping invalid mempool tx: {e}");
            continue;
        }
        out.push(entry);
    }
    if !out.is_empty() {
        return out;
    }

    // Fallback to legacy hex list if structured mempool is empty.
    let path = mempool_file();
    let data = match fs::read_to_string(&path) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    let entries: Vec<LegacyMempoolEntry> = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to parse mempool file {}: {e}", path.display());
            return Vec::new();
        }
    };

    for entry in entries {
        let raw = match hex::decode(&entry.hex) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Invalid tx hex in mempool: {e}");
                continue;
            }
        };
        let tx = decode_compact_tx(&raw);
        if let Err(e) = chain.validate_transaction(&tx) {
            eprintln!("Skipping invalid mempool tx: {e}");
            continue;
        }
        let raw_len = raw.len();
        out.push(irium_node_rs::mempool::MempoolEntry {
            tx,
            raw,
            fee: 0,
            size: raw_len,
            fee_per_byte: 0.0,
            added: 0,
            relays: Vec::new(),
            relay_addresses: Vec::new(),
        });
    }
    out
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
    #[serde(skip_serializing_if = "Option::is_none")]
    submit_source: Option<String>,
}

#[derive(Serialize)]
struct SubmitBlockRequest {
    height: u64,
    header: JsonHeader,
    tx_hex: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    submit_source: Option<String>,
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn rpc_client() -> Result<Client, String> {
    let mut builder = Client::builder().timeout(Duration::from_secs(5));
    if let Ok(path) = env::var("IRIUM_RPC_CA") {
        let pem = fs::read(&path).map_err(|e| format!("read CA {path}: {e}"))?;
        let cert = Certificate::from_pem(&pem).map_err(|e| format!("invalid CA {path}: {e}"))?;
        builder = builder.add_root_certificate(cert);
    }
    let insecure = env::var("IRIUM_RPC_INSECURE")
        .ok()
        .map(|v| {
            let v = v.to_lowercase();
            v == "1" || v == "true" || v == "yes"
        })
        .unwrap_or(false);
    let strict = env::var("IRIUM_RPC_STRICT")
        .ok()
        .map(|v| {
            let v = v.to_lowercase();
            v == "1" || v == "true" || v == "yes"
        })
        .unwrap_or(false);
    let base = node_rpc_base();
    let mut allow_insecure = false;
    if !strict && insecure {
        let url = reqwest::Url::parse(&base).map_err(|e| format!("invalid RPC URL {base}: {e}"))?;
        if url.scheme() != "https" {
            eprintln!("[warn] IRIUM_RPC_INSECURE=1 has no effect on non-HTTPS RPC URL");
        } else {
            let host = url
                .host_str()
                .ok_or_else(|| "RPC URL missing host".to_string())?;
            if !is_loopback_host(host) {
                return Err(format!(
                    "Refusing to disable TLS verification for non-local RPC host {host}; set IRIUM_RPC_CA instead"
                ));
            }
            eprintln!("[warn] IRIUM_RPC_INSECURE=1: TLS verification disabled for https://{host}");
            allow_insecure = true;
        }
    }
    if allow_insecure {
        builder = builder.danger_accept_invalid_certs(true);
    }
    builder.build().map_err(|e| format!("build client: {e}"))
}

fn node_rpc_base() -> String {
    env::var("IRIUM_NODE_RPC").unwrap_or_else(|_| "https://127.0.0.1:38300".to_string())
}

static RPC_HINT_SHOWN: AtomicBool = AtomicBool::new(false);

fn maybe_log_rpc_hint(err: &str) {
    if RPC_HINT_SHOWN.load(Ordering::Relaxed) {
        return;
    }
    let lower = err.to_lowercase();
    let is_unreachable = lower.contains("connection refused")
        || lower.contains("error trying to connect")
        || lower.contains("tcp connect")
        || lower.contains("timed out")
        || lower.contains("dns")
        || lower.contains("no such host")
        || lower.contains("network unreachable")
        || lower.contains("failed to lookup address")
        || lower.contains("connection error");
    if is_unreachable {
        let base = node_rpc_base();
        eprintln!(
            "[hint] No node RPC reachable at {}. Start iriumd or set IRIUM_NODE_RPC=http://<node>:38300 (and IRIUM_RPC_TOKEN if required).",
            base
        );
        RPC_HINT_SHOWN.store(true, Ordering::Relaxed);
    }
}

fn miner_thread_count() -> usize {
    let mut threads: Option<usize> = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--threads" || arg == "-t" {
            if let Some(val) = args.next() {
                threads = val.parse::<usize>().ok();
            }
            continue;
        }
        if let Some(val) = arg.strip_prefix("--threads=") {
            threads = val.parse::<usize>().ok();
        }
    }
    if threads.is_none() {
        if let Ok(val) = env::var("IRIUM_MINER_THREADS") {
            threads = val.parse::<usize>().ok();
        }
    }
    let n = threads.unwrap_or(1);
    if n == 0 {
        1
    } else {
        n
    }
}

fn is_tls_mismatch(err: &str) -> bool {
    let lower = err.to_lowercase();
    lower.contains("invalid http version")
}

fn is_https_scheme_mismatch(err: &str) -> bool {
    let lower = err.to_lowercase();
    lower.contains("wrong version number")
        || lower.contains("first record does not look like a tls handshake")
        || lower.contains("received http/0.9 when not allowed")
        || lower.contains("invalid http version")
        || lower.contains("tls handshake")
        || lower.contains("unexpected eof while reading")
}

fn with_rpc_base<T, F>(f: F) -> Result<T, String>
where
    F: Fn(&str) -> Result<T, String>,
{
    fn should_log_https_fallback() -> bool {
        static LAST_LOG: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
        let lock = LAST_LOG.get_or_init(|| Mutex::new(None));
        if let Ok(mut guard) = lock.lock() {
            let now = Instant::now();
            let allow = guard
                .as_ref()
                .map(|t| now.duration_since(*t) >= Duration::from_secs(60))
                .unwrap_or(true);
            if allow {
                *guard = Some(now);
            }
            allow
        } else {
            true
        }
    }

    let base = node_rpc_base();
    match f(&base) {
        Ok(v) => Ok(v),
        Err(e) => {
            if base.starts_with("https://") && is_https_scheme_mismatch(&e) {
                let http = base.replacen("https://", "http://", 1);
                if let Ok(v) = f(&http) {
                    env::set_var("IRIUM_NODE_RPC", &http);
                    if should_log_https_fallback() {
                        eprintln!("[warn] RPC scheme mismatch; switching to {http}");
                    }
                    return Ok(v);
                }
            }
            if base.starts_with("http://") && is_tls_mismatch(&e) {
                let https = base.replacen("http://", "https://", 1);
                if let Ok(v) = f(&https) {
                    env::set_var("IRIUM_NODE_RPC", &https);
                    eprintln!("[warn] RPC scheme mismatch; switching to {https}");
                    return Ok(v);
                }
            }
            Err(e)
        }
    }
}

fn submit_block_to_node(height: u64, block: &Block) -> Result<(), String> {
    let header = &block.header;
    let hash = header.hash();
    let payload = SubmitBlockRequest {
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
        submit_source: Some("direct_node".to_string()),
    };

    let client = rpc_client()?;

    with_rpc_base(|base| submit_block_to_node_with_base(&client, base, &payload))
}

fn submit_block_to_node_with_base(
    client: &Client,
    base: &str,
    payload: &SubmitBlockRequest,
) -> Result<(), String> {
    let url = format!("{}/rpc/submit_block", base.trim_end_matches("/"));
    let mut req = client.post(url).json(payload);
    if let Some(token) = rpc_token() {
        req = req.bearer_auth(token);
    }
    let resp = req.send().map_err(|e| format!("submit failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(rpc_status_error("submit failed", resp.status()));
    }
    Ok(())
}

fn load_persisted_blocks(state: &mut ChainState) {
    let dir = blocks_dir();
    if !dir.exists() {
        return;
    }
    let mut entries: Vec<(u64, std::path::PathBuf)> = Vec::new();
    if let Ok(read_dir) = dir.read_dir() {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if let Some(stripped) = name.strip_prefix("block_") {
                    if let Some(num_part) = stripped.strip_suffix(".json") {
                        if let Ok(h) = num_part.parse::<u64>() {
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
                let version = header_obj
                    .get("version")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1) as u32;
                let time = header_obj.get("time").and_then(|v| v.as_i64()).unwrap_or(0) as u32;
                let bits_str = header_obj
                    .get("bits")
                    .and_then(|v| v.as_str())
                    .unwrap_or("1d00ffff");
                let bits = u32::from_str_radix(bits_str, 16).unwrap_or(0x1d00_ffff);
                let nonce = header_obj
                    .get("nonce")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as u32;

                let txs = match parsed.get("tx_hex").and_then(|v| v.as_array()) {
                    Some(arr) => {
                        let mut out = Vec::new();
                        for t in arr {
                            if let Some(s) = t.as_str() {
                                if let Ok(bytes) = hex::decode(s) {
                                    let tx = decode_compact_tx(&bytes);
                                    out.push(tx);
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
                // Recompute merkle to be safe.
                block.header.merkle_root = block.merkle_root();

                if let Err(e) = state.connect_block(block) {
                    eprintln!("[⚠️] Failed to connect persisted block {}: {}", h, e);
                    let tip = state.tip_height();
                    prune_blocks_above(tip);
                    println!("[🧹] Pruned persisted blocks above height {}", tip);
                    break;
                }
            }
            Err(e) => eprintln!("[⚠️] Failed to read {}: {}", path.display(), e),
        }
    }

    if state.height > 1 {
        if json_log_enabled() {
            println!(
                "{}",
                json!({"event": "resume_height", "height": state.tip_height(), "ts": Utc::now().format("%H:%M:%S").to_string()})
            );
        } else {
            println!(
                "[↩️] Resumed chain height {} from persisted blocks",
                state.height
            );
        }
    }
}

fn node_http_client() -> Result<Client, String> {
    rpc_client()
}

fn strict_rpc_enabled() -> bool {
    env::var("IRIUM_MINER_STRICT_RPC")
        .ok()
        .map(|v| {
            let v = v.to_lowercase();
            v == "1" || v == "true" || v == "yes"
        })
        .unwrap_or(false)
        || env::var("IRIUM_MINER_FAIL_FAST")
            .ok()
            .map(|v| {
                let v = v.to_lowercase();
                v == "1" || v == "true" || v == "yes"
            })
            .unwrap_or(false)
}

fn gbt_longpoll_enabled() -> bool {
    env::var("IRIUM_GBT_LONGPOLL")
        .ok()
        .map(|v| {
            let v = v.to_lowercase();
            v == "1" || v == "true" || v == "yes"
        })
        .unwrap_or(false)
}

fn gbt_query_params(longpoll: bool) -> Vec<(String, String)> {
    let mut params = Vec::new();
    if longpoll {
        params.push(("longpoll".to_string(), "1".to_string()));
    }
    if let Ok(v) = env::var("IRIUM_GBT_LONGPOLL_SECS") {
        params.push(("poll_secs".to_string(), v));
    }
    if let Ok(v) = env::var("IRIUM_GBT_MAX_TXS") {
        params.push(("max_txs".to_string(), v));
    }
    if let Ok(v) = env::var("IRIUM_GBT_MIN_FEE") {
        params.push(("min_fee".to_string(), v));
    }
    params
}

fn fetch_block_template(client: &Client, longpoll: bool) -> Result<BlockTemplate, String> {
    with_rpc_base(|base| fetch_block_template_with_base(client, base, longpoll))
}

fn fetch_block_template_with_base(
    client: &Client,
    base: &str,
    longpoll: bool,
) -> Result<BlockTemplate, String> {
    let url = format!("{}/rpc/getblocktemplate", base.trim_end_matches("/"));
    let mut req = client.get(url).query(&gbt_query_params(longpoll));
    if let Some(token) = rpc_token() {
        req = req.bearer_auth(token);
    }
    let resp = req.send().map_err(|e| format!("template failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(rpc_status_error("template failed", resp.status()));
    }
    resp.json().map_err(|e| format!("template parse: {e}"))
}

fn fetch_block_json(client: &Client, height: u64) -> Result<serde_json::Value, String> {
    with_rpc_base(|base| fetch_block_json_with_base(client, base, height))
}

fn fetch_block_json_with_base(
    client: &Client,
    base: &str,
    height: u64,
) -> Result<serde_json::Value, String> {
    let url = format!("{}/rpc/block?height={}", base.trim_end_matches('/'), height);
    let mut req = client.get(url);
    if let Some(token) = rpc_token() {
        req = req.bearer_auth(token);
    }
    let resp = req
        .send()
        .map_err(|e| format!("get block {height} failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(rpc_status_error(
            &format!("get block {height} failed"),
            resp.status(),
        ));
    }
    resp.json()
        .map_err(|e| format!("block {height} parse: {e}"))
}

fn miner_sync_guard_enabled() -> bool {
    env::var("IRIUM_MINER_SYNC_GUARD")
        .ok()
        .map(|v| {
            let v = v.to_lowercase();
            v == "1" || v == "true" || v == "yes"
        })
        .unwrap_or(true)
}

fn miner_guard_peer_fallback_enabled() -> bool {
    env::var("IRIUM_MINER_GUARD_PEER_FALLBACK")
        .ok()
        .map(|v| {
            let v = v.to_lowercase();
            v == "1" || v == "true" || v == "yes"
        })
        .unwrap_or(false)
}

fn miner_max_behind() -> u64 {
    env::var("IRIUM_MINER_MAX_BEHIND")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
}

fn fetch_best_peer_height(client: &Client) -> Result<Option<u64>, String> {
    with_rpc_base(|base| {
        let url = format!("{}/peers", base.trim_end_matches('/'));
        let resp = client
            .get(url)
            .send()
            .map_err(|e| format!("peers failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(rpc_status_error("peers failed", resp.status()));
        }
        let data: PeersResponse = resp.json().map_err(|e| format!("peers parse: {e}"))?;
        Ok(data.peers.iter().filter_map(|p| p.height).max())
    })
}

fn fetch_best_network_height(client: &Client) -> Result<Option<u64>, String> {
    with_rpc_base(|base| {
        let url = format!("{}/status", base.trim_end_matches('/'));
        let mut req = client.get(url);
        if let Some(token) = rpc_token() {
            req = req.bearer_auth(token);
        }
        let resp = req.send().map_err(|e| format!("status failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(rpc_status_error("status failed", resp.status()));
        }
        let data: serde_json::Value = resp.json().map_err(|e| format!("status parse: {e}"))?;
        let best_header = data
            .get("best_header_tip")
            .and_then(|v| v.get("height"))
            .and_then(|v| v.as_u64());
        let local = data.get("height").and_then(|v| v.as_u64());
        Ok(best_header.or(local))
    })
}

fn guard_miner_sync(client: &Client, local_tip: u64) -> Result<bool, String> {
    if !miner_sync_guard_enabled() {
        return Ok(true);
    }
    let max_behind = miner_max_behind();

    let network_height = match fetch_best_network_height(client) {
        Ok(v) => v,
        Err(e) => {
            if miner_guard_peer_fallback_enabled() {
                eprintln!("[warn] Miner sync guard status fallback to peers enabled: {e}");
                match fetch_best_peer_height(client) {
                    Ok(v) => v,
                    Err(e2) => {
                        eprintln!("[warn] Miner sync guard skipped (peers): {e2}");
                        return Ok(true);
                    }
                }
            } else {
                eprintln!("[warn] Miner sync guard skipped: status unavailable ({e}); set IRIUM_MINER_GUARD_PEER_FALLBACK=true to use peer-height fallback");
                return Ok(true);
            }
        }
    };

    if let Some(network_height) = network_height {
        if network_height > local_tip.saturating_add(max_behind) {
            if json_log_enabled() {
                println!(
                    "{}",
                    json!({"event": "miner_sync_wait", "local_height": local_tip, "network_height": network_height, "ts": Utc::now().format("%H:%M:%S").to_string()})
                );
            } else {
                println!(
                    "[guard] Node behind network (local {} < network {}); waiting...",
                    local_tip, network_height
                );
            }
            return Ok(false);
        }
    }
    Ok(true)
}

fn parse_bits(bits_str: &str) -> Result<u32, String> {
    let trimmed = bits_str.trim_start_matches("0x");
    u32::from_str_radix(trimmed, 16).map_err(|e| format!("invalid bits field: {e}"))
}

fn connect_block_from_json(state: &mut ChainState, v: &serde_json::Value) -> Result<(), String> {
    let header_obj = v.get("header").ok_or("missing header")?;
    let get_hex32 = |key: &str| -> Result<[u8; 32], String> {
        let s = header_obj
            .get(key)
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("missing {key}"))?;
        let bytes = hex::decode(s).map_err(|e| format!("{key} decode: {e}"))?;
        if bytes.len() != 32 {
            return Err(format!("{key} len {} != 32", bytes.len()));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        Ok(out)
    };
    let prev_hash = get_hex32("prev_hash")?;
    let merkle_root = get_hex32("merkle_root")?;
    let version = header_obj
        .get("version")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as u32;
    let time = header_obj.get("time").and_then(|v| v.as_i64()).unwrap_or(0) as u32;
    let bits_str = header_obj
        .get("bits")
        .and_then(|v| v.as_str())
        .unwrap_or("1d00ffff");
    let bits = u32::from_str_radix(bits_str, 16).unwrap_or(0x1d00_ffff);
    let nonce = header_obj
        .get("nonce")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as u32;

    let txs = match v.get("tx_hex").and_then(|v| v.as_array()) {
        Some(arr) => {
            let mut out = Vec::new();
            for t in arr {
                if let Some(s) = t.as_str() {
                    if let Ok(bytes) = hex::decode(s) {
                        let tx = decode_compact_tx(&bytes);
                        out.push(tx);
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
    state.connect_block(block).map(|_| ())
}

fn reconcile_with_template(
    state: &mut ChainState,
    params: &ChainParams,
    template: &BlockTemplate,
    client: &Client,
) {
    let remote_tip = template.height.saturating_sub(1);
    let prev_bytes = match hex::decode(&template.prev_hash) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[warn] Miner template prev_hash decode failed: {e}");
            return;
        }
    };
    if prev_bytes.len() != 32 {
        eprintln!(
            "[warn] Miner template prev_hash length {} != 32",
            prev_bytes.len()
        );
        return;
    }
    let mut remote_prev = [0u8; 32];
    remote_prev.copy_from_slice(&prev_bytes);

    let mut local_tip = state.tip_height();
    let local_hash = state
        .chain
        .last()
        .map(|b| b.header.hash())
        .unwrap_or([0u8; 32]);

    if local_tip > 0 {
        if let Ok(v) = fetch_block_json(client, local_tip) {
            if let Some(remote_hash) = v
                .get("header")
                .and_then(|h| h.get("hash"))
                .and_then(|v| v.as_str())
            {
                let local_hex = hex::encode(local_hash);
                if remote_hash != local_hex {
                    eprintln!(
                        "[warn] Miner chain mismatch at height {} (local {} != remote {}), resetting to node",
                        local_tip,
                        local_hex,
                        remote_hash
                    );
                    prune_blocks_above(0);
                    *state = ChainState::new(params.clone());
                    local_tip = state.tip_height();
                }
            }
        }
    }

    if local_tip == remote_tip && local_hash != remote_prev {
        eprintln!(
            "[warn] Miner chain diverged at height {} (local {} != remote {}), resetting to node",
            local_tip,
            hex::encode(local_hash),
            template.prev_hash
        );
        prune_blocks_above(0);
        *state = ChainState::new(params.clone());
        local_tip = state.tip_height();
    }

    if remote_tip < local_tip {
        let allow_ahead = env::var("IRIUM_MINER_ALLOW_LOCAL_AHEAD")
            .ok()
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
        if !allow_ahead {
            eprintln!(
                "[warn] Miner ahead of node (local {} > remote {}), resetting to node",
                local_tip, remote_tip
            );
            prune_blocks_above(remote_tip);
            *state = ChainState::new(params.clone());
        } else {
            return;
        }
    }

    if remote_tip <= state.tip_height() {
        return;
    }

    let start = state.tip_height().saturating_add(1);
    let target = remote_tip;
    println!(
        "[sync] Miner downloading blocks {}..{} from node",
        start, target
    );

    for h in start..=target {
        match fetch_block_json(client, h as u64) {
            Ok(v) => {
                if let Err(e) = connect_block_from_json(state, &v) {
                    eprintln!("[warn] Miner failed to connect block {}: {}", h, e);
                    if e.contains("does not extend the current tip") {
                        eprintln!("[warn] Miner chain diverged during sync; resetting to node");
                        prune_blocks_above(0);
                        *state = ChainState::new(params.clone());
                    }
                    break;
                }
            }
            Err(e) => {
                eprintln!("[warn] Miner failed to download block {}: {}", h, e);
                break;
            }
        }
    }

    if state.tip_height() < target {
        eprintln!(
            "[warn] Miner sync incomplete (local height {} < remote {})",
            state.tip_height(),
            target
        );
    } else {
        println!(
            "[ok] Miner caught up to node at height {}",
            state.tip_height()
        );
    }
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
        submit_source: Some("direct_node".to_string()),
    };

    let json = serde_json::to_string_pretty(&jb)?;
    fs::write(path, json)
}

fn template_changed(client: &Client, height: u64, prev_hash: &str) -> bool {
    match fetch_block_template(client, false) {
        Ok(next) => next.height != height || next.prev_hash != prev_hash,
        Err(_) => false,
    }
}

fn mine_once(
    chain: &mut ChainState,
    template: &BlockTemplate,
    client: &Client,
    threads: usize,
) -> Result<bool, String> {
    let height = template.height; // next block height
    if chain.height != height {
        return Err(format!(
            "Template height {} does not match local height {}",
            height, chain.height
        ));
    }

    let prev_bytes =
        hex::decode(&template.prev_hash).map_err(|e| format!("template prev_hash decode: {e}"))?;
    if prev_bytes.len() != 32 {
        return Err(format!("template prev_hash len {} != 32", prev_bytes.len()));
    }
    let mut prev_hash = [0u8; 32];
    prev_hash.copy_from_slice(&prev_bytes);

    let local_prev = chain
        .chain
        .last()
        .map(|b| b.header.hash())
        .unwrap_or([0u8; 32]);
    if local_prev != prev_hash {
        return Err("template prev_hash does not match local tip".to_string());
    }

    let bits = parse_bits(&template.bits)?;
    let expected = chain.target_for_height(height);
    if expected.bits != bits {
        eprintln!(
            "[warn] Template bits {:08x} != expected {:08x}",
            bits, expected.bits
        );
    }
    let target = Target { bits };

    let prev_time = chain.chain.last().map(|b| b.header.time).unwrap_or(0);
    let now = Utc::now().timestamp() as u32;
    let header_time = template.time.max(prev_time.saturating_add(1)).max(now);

    let mempool_entries = load_mempool_entries(chain, Some(template));
    println!(
        "Including {} mempool txs in template",
        mempool_entries.len()
    );

    // Compute total fees from mempool transactions by comparing input and
    // output totals against the current UTXO set.
    let mut total_fees: i64 = 0;
    for entry in &mempool_entries {
        let fee = if entry.fee > 0 {
            entry.fee as i64
        } else {
            // Fallback compute if fee not stored.
            let mut input_total: i64 = 0;
            for txin in &entry.tx.inputs {
                let key = irium_node_rs::chain::OutPoint {
                    txid: txin.prev_txid,
                    index: txin.prev_index,
                };
                if let Some(utxo) = chain.utxos.get(&key) {
                    input_total += utxo.output.value as i64;
                }
            }
            let mut output_total: i64 = 0;
            for out in &entry.tx.outputs {
                output_total += out.value as i64;
            }
            input_total.saturating_sub(output_total)
        };
        if fee > 0 {
            total_fees = total_fees.saturating_add(fee);
        }
    }

    // Derive relay reward commitments from total fees:
    // 10% of total_fees goes to relay commitments split 50/30/20 between
    // up to three relay addresses observed from peers.
    let relay_commitments: Vec<RelayCommitment> = {
        let relay_pool = (total_fees as u64) / 10;
        if relay_pool == 0 {
            Vec::new()
        } else {
            let mut relays: Vec<String> = Vec::new();
            for entry in &mempool_entries {
                for r in entry.relay_addresses.iter().chain(entry.relays.iter()) {
                    if !relays.contains(r) {
                        relays.push(r.clone());
                    }
                    if relays.len() >= 3 {
                        break;
                    }
                }
                if relays.len() >= 3 {
                    break;
                }
            }
            let weights = [50u64, 30, 20];
            let mut out = Vec::new();
            for (idx, w) in weights.iter().enumerate() {
                let amt = relay_pool * *w / 100;
                if amt == 0 {
                    continue;
                }
                let addr = relays
                    .get(idx)
                    .cloned()
                    .unwrap_or_else(|| "RELAY_PLACEHOLDER".to_string());
                let memo = format!("relay-{}", idx);
                out.push(RelayCommitment {
                    address: addr,
                    amount: amt,
                    memo: Some(memo),
                });
            }
            out
        }
    };

    let mut txs = Vec::new();
    // Miner gets subsidy plus remaining fees after relay pool.
    let relay_total: u64 = relay_commitments.iter().map(|c| c.amount).sum();
    let reward = block_reward(height as u64);
    let miner_reward = reward + (total_fees as u64).saturating_sub(relay_total);
    let mut coinbase = build_coinbase(height as u64, miner_reward);

    // Append relay commitment outputs to coinbase.
    for rc in relay_commitments {
        let outputs = rc.build_outputs(|addr| script_from_relay_address(addr))?;
        coinbase.outputs.extend(outputs);
    }

    if let Some(output) = coinbase_metadata_output() {
        coinbase.outputs.push(output);
    }

    txs.push(coinbase);
    for entry in mempool_entries {
        txs.push(entry.tx.clone());
    }

    let header = BlockHeader {
        version: 1,
        prev_hash,
        merkle_root: [0u8; 32],
        time: header_time,
        bits,
        nonce: 0,
    };

    let mut block = Block {
        header,
        transactions: txs.clone(),
    };
    let merkle = block.merkle_root();
    block.header.merkle_root = merkle;

    let threads = threads.max(1);
    let start = Instant::now();
    const LOG_EVERY: u64 = 1_000_000;

    let stop = Arc::new(AtomicBool::new(false));
    let found = Arc::new(AtomicBool::new(false));
    let template_changed_flag = Arc::new(AtomicBool::new(false));
    let attempts = Arc::new(AtomicU64::new(0));
    let result = Arc::new(Mutex::new(None::<(Block, [u8; 32], u32)>));
    let prev_hash_str = template.prev_hash.clone();

    let mut handles = Vec::new();
    for tid in 0..threads {
        let stop = Arc::clone(&stop);
        let found = Arc::clone(&found);
        let template_changed_flag = Arc::clone(&template_changed_flag);
        let attempts = Arc::clone(&attempts);
        let result = Arc::clone(&result);
        let mut block = block.clone();
        let txs = txs.clone();
        let client = client.clone();
        let target = target;
        let prev_hash_str = prev_hash_str.clone();
        let height = height;
        let prev_time = prev_time;
        let step = threads as u32;
        let mut nonce = tid as u32;

        handles.push(thread::spawn(move || {
            let mut local_attempts: u64 = 0;
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                block.header.nonce = nonce;
                let h = block.header.hash();
                if meets_target(&h, target) {
                    if !found.swap(true, Ordering::SeqCst) {
                        let mut guard = result.lock().unwrap_or_else(|e| e.into_inner());
                        *guard = Some((block.clone(), h, nonce));
                    }
                    stop.store(true, Ordering::Relaxed);
                    break;
                }

                nonce = nonce.wrapping_add(step);
                local_attempts += 1;

                if local_attempts >= LOG_EVERY {
                    attempts.fetch_add(local_attempts, Ordering::Relaxed);
                    local_attempts = 0;
                    if tid == 0 {
                        let elapsed = start.elapsed().as_secs_f64();
                        let attempts_total = attempts.load(Ordering::Relaxed);
                        if json_log_enabled() {
                            let rate = if elapsed > 0.0 {
                                Some(attempts_total as f64 / elapsed)
                            } else {
                                None
                            };
                            println!(
                                "{}",
                                json!({
                                    "event": "progress",
                                    "height": height,
                                    "tip_height": height.saturating_sub(1),
                                    "nonce": attempts_total,
                                    "rate_hs": rate,
                                    "ts": Utc::now().format("%H:%M:%S").to_string()
                                })
                            );
                        } else if elapsed > 0.0 {
                            println!(
                                "  mining next height {} (tip {}): hashes {} rate {:.2} H/s",
                                height,
                                height.saturating_sub(1),
                                attempts_total,
                                attempts_total as f64 / elapsed
                            );
                        } else {
                            println!(
                                "[⏱️] next height {} tip {} hashes {}",
                                height,
                                height.saturating_sub(1),
                                attempts_total
                            );
                        }

                        if template_changed(&client, height, &prev_hash_str) {
                            if json_log_enabled() {
                                println!("{}", json!({"event": "template_updated", "height": height, "ts": Utc::now().format("%H:%M:%S").to_string()}));
                            } else {
                                println!("[🔄] Template updated; restarting mining");
                            }
                            template_changed_flag.store(true, Ordering::Relaxed);
                            stop.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }

                if nonce < step {
                    let mut new_time = Utc::now().timestamp() as u32;
                    if new_time <= prev_time {
                        new_time = prev_time.saturating_add(1);
                    }
                    block.header.time = new_time;
                    block.transactions = txs.clone();
                    let merkle = block.merkle_root();
                    block.header.merkle_root = merkle;
                }
            }
            if local_attempts > 0 {
                attempts.fetch_add(local_attempts, Ordering::Relaxed);
            }
        }));
    }

    for handle in handles {
        let _ = handle.join();
    }

    if let Some((block, h, nonce)) = result.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let elapsed = start.elapsed().as_secs_f64();
        let attempts_total = attempts.load(Ordering::Relaxed);
        if json_log_enabled() {
            let rate = if elapsed > 0.0 {
                Some(attempts_total as f64 / elapsed)
            } else {
                None
            };
            println!(
                "{}",
                json!({
                    "event": "mined_block",
                    "height": height,
                    "hash": hex::encode(h),
                    "nonce": nonce,
                    "rate_hs": rate,
                    "ts": Utc::now().format("%H:%M:%S").to_string()
                })
            );
        } else {
            println!("[✅] Mined block at height {}", height);
            println!("   🔗 hash   = {}", hex::encode(h));
            println!("   🎯 nonce  = {}", nonce);
            if elapsed > 0.0 {
                println!("   ⚡ rate   = {:.2} H/s", attempts_total as f64 / elapsed);
            }
        }

        chain.connect_block(block.clone())?;
        write_block_json(height as u64, &block).map_err(|e| e.to_string())?;

        match submit_block_to_node(height as u64, &block) {
            Ok(_) => {
                if json_log_enabled() {
                    println!(
                        "{}",
                        json!({"event": "submit_block", "height": height, "status": "accepted"})
                    );
                } else {
                    println!("[✅] Block accepted by node at height {}", height);
                }
            }
            Err(e) => {
                if json_log_enabled() {
                    eprintln!(
                        "{}",
                        json!({"event": "submit_block_failed", "height": height, "error": e})
                    );
                } else {
                    eprintln!("[❌] Block rejected at height {}: {}", height, e);
                }
            }
        }
        return Ok(true);
    }

    if template_changed_flag.load(Ordering::Relaxed) {
        return Ok(false);
    }
    Ok(false)
}

#[derive(Clone)]
struct StratumJob {
    job_id: String,
    prev_hash: String,
    coinbase1: String,
    coinbase2: String,
    merkle_branch: Vec<String>,
    version: String,
    nbits: String,
    ntime: String,
    _clean_jobs: bool,
}

struct StratumState {
    extranonce1: String,
    extranonce2_size: usize,
    difficulty: f64,
    target: Option<BigUint>,
    job: Option<StratumJob>,
}

fn stratum_url() -> Option<String> {
    env::var("IRIUM_STRATUM_URL").ok()
}

fn stratum_user() -> String {
    env::var("IRIUM_STRATUM_USER").unwrap_or_else(|_| "irium".to_string())
}

fn stratum_pass() -> String {
    env::var("IRIUM_STRATUM_PASS").unwrap_or_else(|_| "x".to_string())
}

fn stratum_normalize_url(url: &str) -> String {
    let trimmed = url.trim();
    for prefix in ["stratum+tcp://", "stratum://", "tcp://"].iter() {
        if trimmed.starts_with(prefix) {
            return trimmed[prefix.len()..].to_string();
        }
    }
    trimmed.to_string()
}

fn stratum_send(writer: &Mutex<TcpStream>, value: &serde_json::Value) -> Result<(), String> {
    let mut stream = writer.lock().unwrap_or_else(|e| e.into_inner());
    let line = format!(
        "{}
",
        value.to_string()
    );
    stream
        .write_all(line.as_bytes())
        .map_err(|e| format!("stratum send: {e}"))
}

fn stratum_read_line(reader: &mut BufReader<TcpStream>) -> Result<serde_json::Value, String> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("stratum read: {e}"))?;
    if line.is_empty() {
        return Err("stratum EOF".to_string());
    }
    serde_json::from_str(&line).map_err(|e| format!("stratum json: {e}"))
}

fn stratum_target_from_difficulty(diff: f64) -> BigUint {
    let pow_limit = Target { bits: 0x1d00_ffff }.to_target();
    if diff <= 0.0 {
        return pow_limit;
    }
    let scale: u64 = 1_000_000;
    let scaled = (diff * scale as f64) as u64;
    if scaled == 0 {
        return pow_limit;
    }
    let scale_big = BigUint::from(scale);
    let scaled_big = BigUint::from(scaled);
    pow_limit * scale_big / scaled_big
}

fn stratum_target_from_hex(hex_str: &str) -> Option<BigUint> {
    let bytes = hex::decode(hex_str).ok()?;
    Some(BigUint::from_bytes_be(&bytes))
}

fn merkle_root_from_stratum(
    job: &StratumJob,
    extranonce1: &str,
    extranonce2: &str,
) -> Result<[u8; 32], String> {
    let coinbase_hex = format!(
        "{}{}{}{}",
        job.coinbase1, extranonce1, extranonce2, job.coinbase2
    );
    let coinbase = hex::decode(&coinbase_hex).map_err(|e| format!("coinbase decode: {e}"))?;
    let mut merkle = sha256d(&coinbase);
    for branch in &job.merkle_branch {
        let branch_bytes = hex::decode(branch).map_err(|e| format!("merkle branch decode: {e}"))?;
        let mut data = Vec::with_capacity(64);
        data.extend_from_slice(&merkle);
        data.extend_from_slice(&branch_bytes);
        merkle = sha256d(&data);
    }
    Ok(merkle)
}

fn parse_u32_hex(hex_str: &str) -> Result<u32, String> {
    let trimmed = hex_str.trim_start_matches("0x");
    u32::from_str_radix(trimmed, 16).map_err(|e| format!("invalid hex: {e}"))
}

fn stratum_reader(
    mut reader: BufReader<TcpStream>,
    state: Arc<Mutex<StratumState>>,
    job_version: Arc<AtomicU64>,
) {
    loop {
        let msg = match stratum_read_line(&mut reader) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[warn] Stratum read failed: {e}");
                break;
            }
        };
        let method = msg.get("method").and_then(|m| m.as_str());
        let params = msg.get("params").and_then(|p| p.as_array());
        match (method, params) {
            (Some("mining.set_difficulty"), Some(p)) => {
                if let Some(diff) = p.get(0).and_then(|v| v.as_f64()) {
                    let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
                    guard.difficulty = diff;
                    guard.target = None;
                }
            }
            (Some("mining.set_target"), Some(p)) => {
                if let Some(t) = p.get(0).and_then(|v| v.as_str()) {
                    let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
                    guard.target = stratum_target_from_hex(t);
                }
            }
            (Some("mining.set_extranonce"), Some(p)) => {
                if let (Some(en1), Some(size)) = (
                    p.get(0).and_then(|v| v.as_str()),
                    p.get(1).and_then(|v| v.as_u64()),
                ) {
                    let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
                    guard.extranonce1 = en1.to_string();
                    guard.extranonce2_size = size as usize;
                }
            }
            (Some("mining.notify"), Some(p)) => {
                if p.len() >= 9 {
                    let job = StratumJob {
                        job_id: p[0].as_str().unwrap_or("").to_string(),
                        prev_hash: p[1].as_str().unwrap_or("").to_string(),
                        coinbase1: p[2].as_str().unwrap_or("").to_string(),
                        coinbase2: p[3].as_str().unwrap_or("").to_string(),
                        merkle_branch: p[4]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default(),
                        version: p[5].as_str().unwrap_or("").to_string(),
                        nbits: p[6].as_str().unwrap_or("").to_string(),
                        ntime: p[7].as_str().unwrap_or("").to_string(),
                        _clean_jobs: p[8].as_bool().unwrap_or(false),
                    };
                    let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
                    guard.job = Some(job);
                    job_version.fetch_add(1, Ordering::SeqCst);
                }
            }
            _ => {}
        }
    }
}

fn mine_stratum_job(
    job: &StratumJob,
    extranonce1: &str,
    extranonce2: &str,
    share_target: &BigUint,
    writer: &Mutex<TcpStream>,
    user: &str,
    submit_id: &AtomicU64,
    job_version: u64,
    job_version_ref: &AtomicU64,
) -> Result<bool, String> {
    let prev_bytes = hex::decode(&job.prev_hash).map_err(|e| format!("prev_hash decode: {e}"))?;
    if prev_bytes.len() != 32 {
        return Err(format!("prev_hash len {} != 32", prev_bytes.len()));
    }
    let mut prev_hash = [0u8; 32];
    prev_hash.copy_from_slice(&prev_bytes);

    let merkle_root = merkle_root_from_stratum(job, extranonce1, extranonce2)?;

    let version = parse_u32_hex(&job.version)?;
    let bits = parse_bits(&job.nbits)?;
    let time = parse_u32_hex(&job.ntime)?;

    let network_target = Target { bits }.to_target();

    let mut nonce: u32 = 0;
    let start = Instant::now();

    loop {
        if job_version_ref.load(Ordering::SeqCst) != job_version {
            return Ok(true);
        }
        let header = BlockHeader {
            version,
            prev_hash,
            merkle_root,
            time,
            bits,
            nonce,
        };
        let hash = header.hash();
        let hash_value = BigUint::from_bytes_be(&hash);
        if &hash_value <= share_target {
            let submit = json!({
                "id": submit_id.fetch_add(1, Ordering::SeqCst),
                "method": "mining.submit",
                "params": [user, job.job_id.as_str(), extranonce2, job.ntime.as_str(), format!("{:08x}", nonce)]
            });
            let _ = stratum_send(writer, &submit);
            if hash_value <= network_target {
                println!(
                    "[🏁] Stratum share meets network target at height? hash={}",
                    hex::encode(hash)
                );
            }
        }
        nonce = nonce.wrapping_add(1);
        if nonce == 0 {
            return Ok(false);
        }
        if nonce % 1_000_000 == 0 {
            let elapsed = start.elapsed().as_secs_f64();
            if elapsed > 0.0 {
                println!(
                    "  stratum mining nonce {} rate {:.2} H/s",
                    nonce,
                    nonce as f64 / elapsed
                );
            }
        }
    }
}

fn run_stratum_miner() -> Result<(), String> {
    let url = match stratum_url() {
        Some(u) => u,
        None => return Err("IRIUM_STRATUM_URL not set".to_string()),
    };
    let addr = stratum_normalize_url(&url);
    let stream = TcpStream::connect(&addr).map_err(|e| format!("stratum connect: {e}"))?;
    let _ = stream.set_nodelay(true);
    let writer = Arc::new(Mutex::new(stream));
    let mut reader = BufReader::new(
        writer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_clone()
            .map_err(|e| e.to_string())?,
    );

    let subscribe = json!({"id": 1, "method": "mining.subscribe", "params": ["irium-miner/0.1"]});
    stratum_send(&writer, &subscribe)?;
    let sub_resp = stratum_read_line(&mut reader)?;
    let (extranonce1, extranonce2_size) = match sub_resp.get("result").and_then(|v| v.as_array()) {
        Some(arr) if arr.len() >= 3 => {
            let en1 = arr[1].as_str().unwrap_or("").to_string();
            let size = arr[2].as_u64().unwrap_or(0) as usize;
            (en1, size)
        }
        _ => return Err("stratum subscribe failed".to_string()),
    };

    let user = stratum_user();
    let pass = stratum_pass();
    let auth =
        json!({"id": 2, "method": "mining.authorize", "params": [user.clone(), pass.clone()]});
    stratum_send(&writer, &auth)?;

    let state = Arc::new(Mutex::new(StratumState {
        extranonce1,
        extranonce2_size,
        difficulty: 1.0,
        target: None,
        job: None,
    }));
    let job_version = Arc::new(AtomicU64::new(0));
    let reader_state = Arc::clone(&state);
    let reader_version = Arc::clone(&job_version);

    thread::spawn(move || {
        stratum_reader(reader, reader_state, reader_version);
    });

    let submit_id = AtomicU64::new(10);
    let mut extranonce_counter: u64 = 0;
    let mut last_job_version = 0u64;

    loop {
        let (job, extranonce1, extranonce2_size, share_target) = {
            let guard = state.lock().unwrap_or_else(|e| e.into_inner());
            let job = guard.job.clone();
            let share_target = guard
                .target
                .clone()
                .unwrap_or_else(|| stratum_target_from_difficulty(guard.difficulty));
            (
                job,
                guard.extranonce1.clone(),
                guard.extranonce2_size,
                share_target,
            )
        };

        let job = match job {
            Some(j) => j,
            None => {
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };

        let current_version = job_version.load(Ordering::SeqCst);
        if current_version != last_job_version {
            extranonce_counter = 0;
            last_job_version = current_version;
        }

        let width = extranonce2_size * 2;
        let extranonce2 = format!("{:0width$x}", extranonce_counter, width = width);

        match mine_stratum_job(
            &job,
            &extranonce1,
            &extranonce2,
            &share_target,
            &writer,
            &user,
            &submit_id,
            current_version,
            &job_version,
        ) {
            Ok(true) => {
                // job changed
            }
            Ok(false) => {
                extranonce_counter = extranonce_counter.saturating_add(1);
            }
            Err(e) => {
                eprintln!("[warn] Stratum mining error: {e}");
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

fn main() {
    let loaded_env = load_env_file("/etc/irium/miner.env");
    if loaded_env {
        if json_log_enabled() {
            println!(
                "{}",
                json!({"event": "env_loaded", "path": "/etc/irium/miner.env", "ts": Utc::now().format("%H:%M:%S").to_string()})
            );
        } else {
            println!("[📄] Loaded /etc/irium/miner.env");
        }
    }
    load_env_file("/etc/irium/miner.env");
    let locked = load_locked_genesis().expect("load locked genesis");
    let block = match block_from_locked(&locked) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to build genesis block from locked config: {e}");
            std::process::exit(1);
        }
    };
    let pow_limit = Target { bits: 0x1d00_ffff };
    let params = ChainParams {
        genesis_block: block,
        pow_limit,
        htlcv1_activation_height: htlcv1_activation_height(),
    };

    let mut state = ChainState::new(params.clone());

    if json_log_enabled() {
        println!(
            "{}",
            json!({"event": "miner_start", "height": state.height, "tip_height": state.tip_height(), "ts": Utc::now().format("%H:%M:%S").to_string()})
        );
    } else {
        println!(
            "[⛏️] Irium Rust miner starting at tip {} (next {})",
            state.tip_height(),
            state.height
        );
    }

    // Optionally report anchors digest if anchors.json is available.
    if let Ok(manager) = AnchorManager::from_default_repo_root(PathBuf::from(".")) {
        if json_log_enabled() {
            println!(
                "{}",
                json!({"event": "anchors_digest", "digest": manager.payload_digest(), "ts": Utc::now().format("%H:%M:%S").to_string()})
            );
        } else {
            println!("[🪝] Anchors digest: {}", manager.payload_digest());
        }
    }

    if stratum_url().is_some() {
        if let Err(e) = run_stratum_miner() {
            eprintln!("[warn] Stratum miner exited: {e}");
        }
        return;
    }

    // Load any persisted blocks so we resume from last mined height.
    load_persisted_blocks(&mut state);

    if let Some((addr, pkh)) = miner_address_info() {
        let pkh_hex = hex::encode(pkh);
        if json_log_enabled() {
            println!(
                "{}",
                json!({"event": "miner_address", "address": addr, "pkh": pkh_hex, "ts": Utc::now().format("%H:%M:%S").to_string()})
            );
        } else {
            println!("[💰] Using miner address: {} (pkh={})", addr, pkh_hex);
        }
    } else {
        if json_log_enabled() {
            println!(
                "{}",
                json!({"event": "miner_pkh_missing", "ts": Utc::now().format("%H:%M:%S").to_string()})
            );
        } else {
            println!(
                "[⚠️] WARNING: Set IRIUM_MINER_ADDRESS (base58) or IRIUM_MINER_PKH (40-hex) so rewards are spendable"
            );
        }
    }

    let threads = miner_thread_count();
    if json_log_enabled() {
        println!(
            "{}",
            json!({"event": "miner_threads", "threads": threads, "ts": Utc::now().format("%H:%M:%S").to_string()})
        );
    } else {
        println!("[🧵] Mining threads: {}", threads);
    }

    loop {
        let client = match node_http_client() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[warn] Miner could not build HTTP client: {e}");
                if strict_rpc_enabled() {
                    std::process::exit(1);
                }
                std::thread::sleep(Duration::from_secs(3));
                continue;
            }
        };

        let longpoll = gbt_longpoll_enabled();
        let template = match fetch_block_template(&client, longpoll) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[warn] Miner could not fetch block template: {e}");
                maybe_log_rpc_hint(&e);
                if strict_rpc_enabled() {
                    std::process::exit(1);
                }
                std::thread::sleep(Duration::from_secs(3));
                continue;
            }
        };

        reconcile_with_template(&mut state, &params, &template, &client);

        let local_tip = template.height.saturating_sub(1);
        match guard_miner_sync(&client, local_tip) {
            Ok(true) => {}
            Ok(false) => {
                std::thread::sleep(Duration::from_secs(5));
                continue;
            }
            Err(e) => {
                eprintln!("[warn] Miner sync guard error: {e}");
            }
        }

        if state.height != template.height {
            eprintln!(
                "[warn] Template height {} does not match local height {}",
                template.height, state.height
            );
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }

        if json_log_enabled() {
            println!(
                "{}",
                json!({"event": "mining_start", "height": state.height, "tip_height": state.tip_height(), "ts": Utc::now().format("%H:%M:%S").to_string()})
            );
        } else {
            println!(
                "[▶️] Mining next height {} (tip {})",
                state.height,
                state.tip_height()
            );
        }

        match mine_once(&mut state, &template, &client, threads) {
            Ok(true) => {
                if json_log_enabled() {
                    println!(
                        "{}",
                        json!({"event": "mined_block_written", "height": state.height.saturating_sub(1), "ts": Utc::now().format("%H:%M:%S").to_string()})
                    );
                } else {
                    println!("[💾] Wrote block_{}.json", state.height.saturating_sub(1));
                }
            }
            Ok(false) => {
                // Template changed; restart loop for a fresh template.
            }
            Err(e) => {
                if json_log_enabled() {
                    eprintln!(
                        "{}",
                        json!({"event": "mining_failed", "error": e.to_string(), "height": state.height, "tip_height": state.tip_height(), "ts": Utc::now().format("%H:%M:%S").to_string()})
                    );
                } else {
                    eprintln!(
                        "[⚠️] Mining failed at next height {} (tip {}): {e}",
                        state.height,
                        state.tip_height()
                    );
                }
                if strict_rpc_enabled() {
                    std::process::exit(1);
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        }

        // Immediately proceed to the next height, mirroring the continuous loop in the
        // reference miner screenshot.
    }
}
