use reqwest::blocking::Client;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use std::{env, fs, sync::{OnceLock, Arc, atomic::{AtomicBool, AtomicU32, Ordering}}};
use std::sync::mpsc;
use std::thread;

use bs58;
use chrono::Utc;
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
use irium_node_rs::pow::{meets_target, Target};
use irium_node_rs::relay::RelayCommitment;
use irium_node_rs::tx::{Transaction, TxInput, TxOutput};

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

#[cfg(test)]
mod tests {
    use super::script_from_relay_address;

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





fn miner_threads() -> usize {
    // CLI flags take precedence over env; fallback to available CPUs.
    let mut threads: Option<usize> = None;

    let mut args = env::args();
    let _ = args.next(); // skip binary name
    while let Some(arg) = args.next() {
        if arg == "--threads" || arg == "-t" {
            if let Some(v) = args.next() {
                if let Ok(n) = v.parse::<usize>() {
                    threads = Some(n.max(1));
                }
            }
        } else if let Some(rest) = arg.strip_prefix("--threads=") {
            if let Ok(n) = rest.parse::<usize>() {
                threads = Some(n.max(1));
            }
        }
    }

    if threads.is_none() {
        if let Ok(val) = env::var("IRIUM_MINER_THREADS") {
            if let Ok(n) = val.parse::<usize>() {
                threads = Some(n.max(1));
            }
        }
    }

    threads.unwrap_or_else(|| thread::available_parallelism().map(|n| n.get()).unwrap_or(1))
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

/// Load mempool transactions, accepting either the new structured mempool
/// file or the legacy hex-only format.
fn load_mempool_entries(chain: &ChainState) -> Vec<irium_node_rs::mempool::MempoolEntry> {
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
}

#[derive(Serialize)]
struct SubmitBlockRequest {
    height: u64,
    header: JsonHeader,
    tx_hex: Vec<String>,
}

fn node_rpc_base() -> String {
    env::var("IRIUM_NODE_RPC").unwrap_or_else(|_| "http://127.0.0.1:38300".to_string())
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
    };

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("build client: {e}"))?;

    let base = node_rpc_base();
    let url = format!("{}/rpc/submit_block", base.trim_end_matches("/"));
    let resp = client
        .post(url)
        .json(&payload)
        .send()
        .map_err(|e| format!("submit failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("submit failed: HTTP {}", resp.status()));
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
                let version = header_obj.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                let time = header_obj.get("time").and_then(|v| v.as_i64()).unwrap_or(0) as u32;
                let bits_str = header_obj.get("bits").and_then(|v| v.as_str()).unwrap_or("1d00ffff");
                let bits = u32::from_str_radix(bits_str, 16).unwrap_or(0x1d00_ffff);
                let nonce = header_obj.get("nonce").and_then(|v| v.as_i64()).unwrap_or(0) as u32;

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
                }
            }
            Err(e) => eprintln!("[⚠️] Failed to read {}: {}", path.display(), e),
        }
    }

    if state.height > 1 {
        if json_log_enabled() {
            println!(
                "{}",
                json!({"event": "resume_height", "height": state.height, "ts": Utc::now().format("%H:%M:%S").to_string()})
            );
        } else {
            println!("[↩️] Resumed chain height {} from persisted blocks", state.height);
        }
    }
}


fn node_http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("build client: {e}"))
}

fn fetch_status_height(client: &Client) -> Result<u64, String> {
    #[derive(Deserialize)]
    struct StatusResp {
        height: u64,
    }
    let base = node_rpc_base();
    let url = format!("{}/status", base.trim_end_matches('/'));
    let resp = client
        .get(url)
        .send()
        .map_err(|e| format!("status failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("status failed: HTTP {}", resp.status()));
    }
    let parsed: StatusResp = resp
        .json()
        .map_err(|e| format!("status parse: {e}"))?;
    Ok(parsed.height)
}

fn fetch_block_json(client: &Client, height: u64) -> Result<serde_json::Value, String> {
    let base = node_rpc_base();
    let url = format!("{}/rpc/block?height={}", base.trim_end_matches('/'), height);
    let resp = client
        .get(url)
        .send()
        .map_err(|e| format!("get block {height} failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("get block {height} failed: HTTP {}", resp.status()));
    }
    resp.json()
        .map_err(|e| format!("block {height} parse: {e}"))
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
    let time = header_obj
        .get("time")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as u32;
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

fn sync_from_node(state: &mut ChainState) {
    let client = match node_http_client() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[warn] Miner could not build HTTP client: {e}");
            return;
        }
    };

    let remote_height = match fetch_status_height(&client) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[warn] Miner could not fetch node status: {e}");
            return;
        }
    };

    if remote_height <= state.height {
        return;
    }

    let start = state.height;
    let target = remote_height;
    println!(
        "[sync] Miner downloading blocks {}..{} from node",
        start,
        target.saturating_sub(1)
    );

    for h in start..target {
        match fetch_block_json(&client, h as u64) {
            Ok(v) => {
                if let Err(e) = connect_block_from_json(state, &v) {
                    eprintln!("[warn] Miner failed to connect block {}: {}", h, e);
                    break;
                }
            }
            Err(e) => {
                eprintln!("[warn] Miner failed to download block {}: {}", h, e);
                break;
            }
        }
    }

    if state.height < target {
        eprintln!(
            "[warn] Miner sync incomplete (local height {} < remote {})",
            state.height,
            target
        );
    } else {
        println!("[ok] Miner caught up to node at height {}", state.height);
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
    };

    let json = serde_json::to_string_pretty(&jb)?;
    fs::write(path, json)
}

fn mine_once(chain: &mut ChainState) -> Result<(), String> {
    let height = chain.height; // next block height
    let tip_hash = if let Some(last) = chain.chain.last() {
        last.header.hash()
    } else {
        [0u8; 32]
    };

    let reward = block_reward(height as u64);

    let mempool_entries = load_mempool_entries(chain);
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
    let miner_reward = reward + (total_fees as u64).saturating_sub(relay_total);
    let mut coinbase = build_coinbase(height as u64, miner_reward);

    // Append relay commitment outputs to coinbase.
    for rc in relay_commitments {
        let outputs = rc.build_outputs(|addr| script_from_relay_address(addr))?;
        coinbase.outputs.extend(outputs);
    }

    txs.push(coinbase);
    for entry in mempool_entries {
        txs.push(entry.tx.clone());
    }

    let header = BlockHeader {
        version: 1,
        prev_hash: tip_hash,
        merkle_root: [0u8; 32],
        time: Utc::now().timestamp() as u32,
        bits: chain.target_for_height(height).bits,
        nonce: 0,
    };

    let mut block = Block {
        header,
        transactions: txs.clone(),
    };
    let merkle = block.merkle_root();
    block.header.merkle_root = merkle;

    let target = chain.target_for_height(height);

    let mut nonce: u32 = 0;
    let start = Instant::now();

    loop {
        block.header.nonce = nonce;
        let h = block.header.hash();
        if meets_target(&h, target) {
            let elapsed = start.elapsed().as_secs_f64();
            if json_log_enabled() {
                println!(
                    "{}",
                    json!({
                        "event": "mined_block",
                        "height": height,
                        "hash": hex::encode(h),
                        "nonce": nonce,
                        "rate_hs": if elapsed > 0.0 { Some(nonce as f64 / elapsed) } else { None },
                        "ts": Utc::now().format("%H:%M:%S").to_string()
                    })
                );
            } else {
                println!("[✅] Mined block at height {}", height);
                println!("   🔗 hash   = {}", hex::encode(h));
                println!("   🎯 nonce  = {}", nonce);
                if elapsed > 0.0 {
                    println!("   ⚡ rate   = {:.2} H/s", nonce as f64 / elapsed);
                }
            }

            // Connect block to chain (updates UTXOs, height, etc.)
            chain.connect_block(block.clone())?;

            // Write JSON file
            write_block_json(height as u64, &block).map_err(|e| e.to_string())?;

            // Submit to local node HTTP RPC so the network sees the block.
            match submit_block_to_node(height as u64, &block) {
                Ok(_) => {
                    if json_log_enabled() {
                        println!("{}", json!({"event": "submit_block", "height": height, "status": "accepted"}));
                    } else {
                        println!("[📡] Submitted block {} to local node", height);
                    }
                }
                Err(e) => {
                    if json_log_enabled() {
                        eprintln!("{}", json!({"event": "submit_block_failed", "height": height, "error": e}));
                    } else {
                        eprintln!("[⚠️] Failed to submit block {} to node: {}", height, e);
                    }
                }
            }
            return Ok(());
        }

        nonce = nonce.wrapping_add(1);
        if nonce == 0 {
            // Wrapped; refresh timestamp and merkle root
            block.header.time = Utc::now().timestamp() as u32;
            block.transactions = txs.clone();
            let merkle = block.merkle_root();
            block.header.merkle_root = merkle;
        }

        if nonce % 1_000_000 == 0 {
            let elapsed = start.elapsed().as_secs_f64();
            if json_log_enabled() {
                let rate = if elapsed > 0.0 {
                    Some(nonce as f64 / elapsed)
                } else {
                    None
                };
                println!(
                    "{}",
                    json!({
                        "event": "progress",
                        "height": height,
                        "nonce": nonce,
                        "rate_hs": rate,
                        "ts": Utc::now().format("%H:%M:%S").to_string()
                    })
                );
            } else {
                if elapsed > 0.0 {
                    println!(
                        "  mining height {}: nonce {} rate {:.2} H/s",
                        height,
                        nonce,
                        nonce as f64 / elapsed
                    );
                } else {
                    println!("[⏱️] height {} nonce {}", height, nonce);
                }
            }
        }
    }
}

fn main() {
    let locked = load_locked_genesis().expect("load locked genesis");
    let block = block_from_locked(&locked);
    let pow_limit = Target { bits: 0x1d00_ffff };
    let params = ChainParams {
        genesis_block: block,
        pow_limit,
    };

    let mut state = ChainState::new(params);

    if json_log_enabled() {
        println!(
            "{}",
            json!({"event": "miner_start", "height": state.height, "ts": Utc::now().format("%H:%M:%S").to_string()})
        );
    } else {
        println!("[⛏️] Irium Rust miner starting at height {}", state.height);
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

    // Load any persisted blocks so we resume from last mined height.
    load_persisted_blocks(&mut state);
    sync_from_node(&mut state);

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

    loop {
        if json_log_enabled() {
            println!(
                "{}",
                json!({"event": "mining_start", "height": state.height, "ts": Utc::now().format("%H:%M:%S").to_string()})
            );
        } else {
            println!("[▶️] Mining height {}", state.height);
        }

        if let Err(e) = mine_once(&mut state) {
            if json_log_enabled() {
                eprintln!(
                    "{}",
                    json!({"event": "mining_failed", "error": e.to_string(), "height": state.height, "ts": Utc::now().format("%H:%M:%S").to_string()})
                );
            } else {
                eprintln!("[⚠️] Mining failed at height {}: {e}", state.height);
            }
            break;
        }

        if json_log_enabled() {
            println!(
                "{}",
                json!({"event": "mined_block_written", "height": state.height.saturating_sub(1), "ts": Utc::now().format("%H:%M:%S").to_string()})
            );
        } else {
            println!("[💾] Wrote block_{}.json", state.height.saturating_sub(1));
        }

        // Immediately proceed to the next height, mirroring the continuous loop in the
        // reference miner screenshot.
    }
}
