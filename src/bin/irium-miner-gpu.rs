//! GPU miner for Irium using OpenCL SHA-256d.
//!
//! Build: cargo build --release --features gpu --bin irium-miner-gpu
//!
//! System requirements (Linux):
//!   apt install ocl-icd-opencl-dev   # OpenCL headers + ICD loader
//!   # Plus your GPU's OpenCL runtime:
//!   #   NVIDIA: nvidia-opencl-dev  or the proprietary driver
//!   #   AMD:    amdgpu-opencl-icd  or ROCm
//!   #   Intel:  intel-opencl-icd
//!
//! Env vars (same as irium-miner):
//!   IRIUM_NODE_RPC, IRIUM_RPC_TOKEN, IRIUM_MINER_ADDRESS, IRIUM_RPC_CA,
//!   IRIUM_RPC_INSECURE, IRIUM_JSON_LOG, IRIUM_COINBASE_METADATA
//!
//! GPU-specific env vars:
//!   IRIUM_GPU_BATCH     nonces per GPU dispatch                       (default: 4194304 = 2^22)
//!   IRIUM_GPU_PLATFORM  OpenCL platform index or vendor name substring (default: auto, prefers NVIDIA/AMD)
//!   IRIUM_GPU_DEVICE    OpenCL device index within selected platform   (default: 0)
//!   IRIUM_GPU_DEVICES   comma-separated device indices within platform (overrides IRIUM_GPU_DEVICE)
//!
//! CLI flags:
//!   --platform <n|name>  select OpenCL platform by index or vendor name substring
//!   --list-platforms     print all detected OpenCL platforms and devices, then exit

fn fmt_rate(hs: f64) -> String {
    if hs >= 1_000_000_000.0 {
        format!("{:.2} GH/s", hs / 1_000_000_000.0)
    } else if hs >= 1_000_000.0 {
        format!("{:.2} MH/s", hs / 1_000_000.0)
    } else if hs >= 1_000.0 {
        format!("{:.2} KH/s", hs / 1_000.0)
    } else {
        format!("{:.0} H/s", hs)
    }
}

use bs58;
use chrono::Utc;
use irium_node_rs::block::{Block, BlockHeader};
use irium_node_rs::chain::decode_compact_tx;
use irium_node_rs::constants::block_reward;
use irium_node_rs::pow::{meets_target, sha256d, Target};
use irium_node_rs::relay::RelayCommitment;
use irium_node_rs::tx::{Transaction, TxInput, TxOutput};
use num_bigint::BigUint;
use ocl::{flags, Buffer, Context, Device, Kernel, Platform, Program, Queue};
use reqwest::blocking::Client;
use reqwest::Certificate;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use std::{env, fs};

// =============================================================================
// SHA-256 — CPU implementation for midstate computation
// =============================================================================
//
// The block header is 80 bytes.  SHA-256 processes it in two 64-byte blocks:
//   block 1 → bytes 0–63  (version + prev_hash + first 28 bytes of merkle)
//   block 2 → bytes 64–79 + padding
//
// The nonce lives in bytes 76–79 (block 2), so the SHA-256 state after block 1
// is the same for every nonce candidate.  We compute this "midstate" once on
// the CPU and hand it to the GPU; each GPU thread only processes block 2.
//
// The `sha2` crate does not expose intermediate state, so we implement the
// compression function directly.

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const SHA256_H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

fn sha256_compress(state: &mut [u32; 8], msg: &[u32; 16]) {
    let mut w = [0u32; 64];
    w[..16].copy_from_slice(msg);
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA256_K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

/// SHA-256 state after processing the first 64 bytes of the block header.
fn sha256_midstate(first64: &[u8; 64]) -> [u32; 8] {
    let mut msg = [0u32; 16];
    for i in 0..16 {
        msg[i] = u32::from_be_bytes(first64[i * 4..i * 4 + 4].try_into().unwrap());
    }
    let mut state = SHA256_H0;
    sha256_compress(&mut state, &msg);
    state
}

// =============================================================================
// OpenCL kernel
// =============================================================================
//
// Each work-item handles one nonce candidate.  The kernel:
//   1. Builds the padded second SHA-256 block (bytes 64–127 of the padded msg).
//   2. Completes SHA-256 from the pre-computed midstate.
//   3. Runs a second SHA-256 on the 32-byte inner hash.
//   4. Compares the result against the target.
//   5. If valid, records the nonce atomically (first finder wins).
//
// Layout of header_bytes[64..80] in wire format:
//   bytes 64–67: last 4 bytes of merkle_root (reversed)  → tail[0]  (big-endian u32)
//   bytes 68–71: time  (little-endian u32 on wire)        → tail[1]  (big-endian u32)
//   bytes 72–75: bits  (little-endian u32 on wire)        → tail[2]  (big-endian u32)
//   bytes 76–79: nonce (little-endian u32 on wire)        → BSWAP32(nonce_le)
//
// SHA-256 treats the message as big-endian 32-bit words, so every 4-byte group
// of the serialised header becomes a big-endian word in the message schedule.

const KERNEL_SRC: &str = r#"
/* SHA-256 round constants */
constant uint K[64] = {
    0x428a2f98u, 0x71374491u, 0xb5c0fbcfu, 0xe9b5dba5u,
    0x3956c25bu, 0x59f111f1u, 0x923f82a4u, 0xab1c5ed5u,
    0xd807aa98u, 0x12835b01u, 0x243185beu, 0x550c7dc3u,
    0x72be5d74u, 0x80deb1feu, 0x9bdc06a7u, 0xc19bf174u,
    0xe49b69c1u, 0xefbe4786u, 0x0fc19dc6u, 0x240ca1ccu,
    0x2de92c6fu, 0x4a7484aau, 0x5cb0a9dcu, 0x76f988dau,
    0x983e5152u, 0xa831c66du, 0xb00327c8u, 0xbf597fc7u,
    0xc6e00bf3u, 0xd5a79147u, 0x06ca6351u, 0x14292967u,
    0x27b70a85u, 0x2e1b2138u, 0x4d2c6dfcu, 0x53380d13u,
    0x650a7354u, 0x766a0abbu, 0x81c2c92eu, 0x92722c85u,
    0xa2bfe8a1u, 0xa81a664bu, 0xc24b8b70u, 0xc76c51a3u,
    0xd192e819u, 0xd6990624u, 0xf40e3585u, 0x106aa070u,
    0x19a4c116u, 0x1e376c08u, 0x2748774cu, 0x34b0bcb5u,
    0x391c0cb3u, 0x4ed8aa4au, 0x5b9cca4fu, 0x682e6ff3u,
    0x748f82eeu, 0x78a5636fu, 0x84c87814u, 0x8cc70208u,
    0x90befffau, 0xa4506cebu, 0xbef9a3f7u, 0xc67178f2u
};

/* Byte-swap a 32-bit word */
#define BSWAP32(x) ( \
    (((x) & 0xFF000000u) >> 24u) | \
    (((x) & 0x00FF0000u) >>  8u) | \
    (((x) & 0x0000FF00u) <<  8u) | \
    (((x) & 0x000000FFu) << 24u))

/* SHA-256 compression function.
 * state[8]: in/out hash state.
 * w[16]:    message block (16 big-endian 32-bit words). */
void sha256_compress(uint state[8], uint w[16]) {
    uint ws[64];
    for (int i = 0;  i < 16; i++) ws[i] = w[i];
    for (int i = 16; i < 64; i++) {
        uint s0 = rotate(ws[i-15], 25u) ^ rotate(ws[i-15], 14u) ^ (ws[i-15] >> 3u);
        uint s1 = rotate(ws[i- 2], 15u) ^ rotate(ws[i- 2], 13u) ^ (ws[i- 2] >> 10u);
        ws[i] = ws[i-16] + s0 + ws[i-7] + s1;
    }
    uint a=state[0], b=state[1], c=state[2], d=state[3];
    uint e=state[4], f=state[5], g=state[6], h=state[7];
    for (int i = 0; i < 64; i++) {
        uint S1    = rotate(e, 26u) ^ rotate(e, 21u) ^ rotate(e,  7u);
        uint ch    = (e & f) ^ (~e & g);
        uint temp1 = h + S1 + ch + K[i] + ws[i];
        uint S0    = rotate(a, 30u) ^ rotate(a, 19u) ^ rotate(a, 10u);
        uint maj   = (a & b) ^ (a & c) ^ (b & c);
        uint temp2 = S0 + maj;
        h=g; g=f; f=e; e=d+temp1; d=c; c=b; b=a; a=temp1+temp2;
    }
    state[0]+=a; state[1]+=b; state[2]+=c; state[3]+=d;
    state[4]+=e; state[5]+=f; state[6]+=g; state[7]+=h;
}

kernel void sha256d_mine(
    global const uint*    midstate,   /* 8 words: SHA-256 state after first 64 header bytes */
    global const uint*    tail,       /* 3 words: header bytes 64-75 as big-endian uint32   */
    uint                  nonce_base, /* first nonce of this batch (little-endian u32)       */
    global const uint*    target,     /* 8 words: PoW target as big-endian uint32            */
    global volatile uint* result      /* [0] = found flag (0/1), [1] = winning nonce         */
) {
    uint gid      = (uint)get_global_id(0);
    uint nonce_le = nonce_base + gid;

    /* Build the second 64-byte SHA-256 block.
     * The padded 80-byte message has length 640 bits = 0x280. */
    uint w2[16];
    w2[ 0] = tail[0];
    w2[ 1] = tail[1];
    w2[ 2] = tail[2];
    w2[ 3] = BSWAP32(nonce_le); /* nonce is LE on the wire; SHA-256 words are BE */
    w2[ 4] = 0x80000000u;       /* padding bit */
    w2[ 5] = 0u; w2[ 6] = 0u; w2[ 7] = 0u;
    w2[ 8] = 0u; w2[ 9] = 0u; w2[10] = 0u;
    w2[11] = 0u; w2[12] = 0u; w2[13] = 0u;
    w2[14] = 0u;
    w2[15] = 0x00000280u; /* message length: 640 bits */

    /* Complete the first SHA-256 from the midstate */
    uint state1[8];
    for (int i = 0; i < 8; i++) state1[i] = midstate[i];
    sha256_compress(state1, w2);

    /* Second SHA-256: compress the 32-byte inner hash.
     * Input = state1 (8 words) + padding for 256-bit message. */
    uint state2[8] = {
        0x6a09e667u, 0xbb67ae85u, 0x3c6ef372u, 0xa54ff53au,
        0x510e527fu, 0x9b05688cu, 0x1f83d9abu, 0x5be0cd19u
    };
    uint w3[16];
    for (int i = 0; i < 8; i++) w3[i] = state1[i];
    w3[ 8] = 0x80000000u;
    w3[ 9] = 0u; w3[10] = 0u; w3[11] = 0u;
    w3[12] = 0u; w3[13] = 0u; w3[14] = 0u;
    w3[15] = 0x00000100u; /* 256 bits */
    sha256_compress(state2, w3);

    /* state2 = raw SHA-256d output (8 big-endian uint32 words).
     *
     * BlockHeader::hash() reverses the bytes before returning, so the value
     * that meets_target() sees is: [bswap32(state2[7]), ..., bswap32(state2[0])].
     * Compare it word-by-word against target[] (also big-endian uint32). */
    bool ok = false;
    for (int i = 0; i < 8; i++) {
        uint h = BSWAP32(state2[7 - i]);
        uint t = target[i];
        if (h < t) { ok = true;  break; }
        if (h > t) { ok = false; break; }
        if (i == 7)  ok = true; /* all words equal → hash == target → valid */
    }

    if (ok) {
        /* Atomically claim the slot; first thread to flip 0→1 writes the nonce. */
        int old = atomic_cmpxchg((volatile global int*)result, 0, 1);
        if (old == 0) result[1] = nonce_le;
    }
}
"#;

// =============================================================================
// RPC / environment helpers
// =============================================================================

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

fn node_rpc_base() -> String {
    env::var("IRIUM_NODE_RPC").unwrap_or_default()
}

fn json_log_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        env::var("IRIUM_JSON_LOG")
            .ok()
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false)
    })
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn rpc_client() -> Result<Client, String> {
    let mut builder = Client::builder().timeout(Duration::from_secs(5));
    if let Ok(path) = env::var("IRIUM_RPC_CA") {
        let pem = fs::read(&path).map_err(|e| format!("read CA {path}: {e}"))?;
        let cert = Certificate::from_pem(&pem).map_err(|e| format!("invalid CA: {e}"))?;
        builder = builder.add_root_certificate(cert);
    }
    // IRIUM_RPC_INSECURE is a development-only escape hatch that disables TLS
    // certificate verification. It is intentionally restricted to loopback
    // addresses (localhost / 127.0.0.1 / ::1) to prevent accidental use against
    // remote nodes where certificate bypass would expose the connection to MITM.
    // For production use with self-signed certs, use IRIUM_RPC_CA instead.
    let insecure = env::var("IRIUM_RPC_INSECURE")
        .ok()
        .map(|v| {
            let v = v.to_lowercase();
            v == "1" || v == "true" || v == "yes"
        })
        .unwrap_or(false);
    if insecure {
        let base = node_rpc_base();
        match reqwest::Url::parse(&base) {
            Ok(url) if is_loopback_host(url.host_str().unwrap_or("")) => {
                // Loopback-only: permit certificate bypass for local dev nodes.
                builder = builder.danger_accept_invalid_certs(true);
            }
            Ok(_) => {
                // Non-loopback: refuse to disable TLS verification regardless of
                // the env flag. Use IRIUM_RPC_CA to trust a self-signed cert instead.
                eprintln!(
                    "[warn] IRIUM_RPC_INSECURE is set but the RPC URL ({}) is not a                      loopback address; TLS certificate verification will NOT be disabled.                      Use IRIUM_RPC_CA to trust a custom CA certificate.",
                    base
                );
            }
            Err(_) => {
                // URL parse failure: refuse to disable TLS verification.
                eprintln!(
                    "[warn] IRIUM_RPC_INSECURE is set but the RPC URL could not be parsed;                      TLS certificate verification will NOT be disabled."
                );
            }
        }
    }
    builder.build().map_err(|e| format!("HTTP client: {e}"))
}

// =============================================================================
// Block template / submission types
// =============================================================================

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
struct SubmitBlockRequest {
    height: u64,
    header: JsonHeader,
    tx_hex: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    submit_source: Option<String>,
}

fn fetch_template(client: &Client) -> Result<BlockTemplate, String> {
    let url = format!(
        "{}/rpc/getblocktemplate",
        node_rpc_base().trim_end_matches('/')
    );
    let mut req = client.get(&url);
    if let Some(token) = rpc_token() {
        req = req.bearer_auth(token);
    }
    let resp = req.send().map_err(|e| format!("fetch template: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("fetch template: HTTP {}", resp.status()));
    }
    resp.json::<BlockTemplate>()
        .map_err(|e| format!("parse template: {e}"))
}

fn submit_block(client: &Client, height: u64, block: &Block) -> Result<(), String> {
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
        submit_source: Some("gpu".to_string()),
    };
    let url = format!("{}/rpc/submit_block", node_rpc_base().trim_end_matches('/'));
    let mut req = client.post(&url).json(&payload);
    if let Some(token) = rpc_token() {
        req = req.bearer_auth(token);
    }
    let resp = req.send().map_err(|e| format!("submit: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("submit: HTTP {}", resp.status()));
    }
    Ok(())
}

// =============================================================================
// Address / coinbase helpers
// =============================================================================

fn base58_p2pkh_to_hash(addr: &str) -> Option<Vec<u8>> {
    let data = bs58::decode(addr).into_vec().ok()?;
    if data.len() < 25 {
        return None;
    }
    let (body, checksum) = data.split_at(data.len() - 4);
    let mut h = Sha256::new();
    h.update(body);
    let first = h.finalize_reset();
    h.update(first);
    let second = h.finalize();
    if &second[0..4] != checksum {
        return None;
    }
    let payload = &body[1..];
    if payload.len() != 20 {
        return None;
    }
    Some(payload.to_vec())
}

fn miner_pubkey_hash() -> Option<Vec<u8>> {
    if let Ok(addr) = env::var("IRIUM_MINER_ADDRESS") {
        if let Some(pkh) = base58_p2pkh_to_hash(&addr) {
            return Some(pkh);
        }
    }
    if let Ok(addr) = env::var("IRIUM_RELAY_ADDRESS") {
        if let Some(pkh) = base58_p2pkh_to_hash(&addr) {
            return Some(pkh);
        }
    }
    if let Ok(hex_str) = env::var("IRIUM_MINER_PKH") {
        if hex_str.len() == 40 {
            if let Ok(pkh) = hex::decode(&hex_str) {
                return Some(pkh);
            }
        }
    }
    None
}

fn op_return_output(data: &[u8]) -> TxOutput {
    let mut script = Vec::with_capacity(2 + data.len());
    script.push(0x6a);
    script.push(data.len() as u8);
    script.extend_from_slice(data);
    TxOutput {
        value: 0,
        script_pubkey: script,
    }
}

fn coinbase_metadata_output() -> Option<TxOutput> {
    let raw = env::var("IRIUM_COINBASE_METADATA")
        .ok()
        .or_else(|| env::var("IRIUM_NOTARY_HASH").ok())?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let hex_hash = if raw.len() == 64 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
        raw.to_lowercase()
    } else {
        let mut h = Sha256::new();
        h.update(raw.as_bytes());
        hex::encode(h.finalize())
    };
    let payload = format!("notary:{hex_hash}");
    let bytes = payload.as_bytes();
    if bytes.len() > 75 {
        return None;
    }
    Some(op_return_output(bytes))
}

fn script_from_relay_address(addr: &str) -> Result<Vec<u8>, String> {
    if addr.len() == 40 {
        if let Ok(pkh) = hex::decode(addr) {
            if pkh.len() == 20 {
                let mut s = Vec::with_capacity(25);
                s.extend_from_slice(&[0x76, 0xa9, 0x14]);
                s.extend_from_slice(&pkh);
                s.extend_from_slice(&[0x88, 0xac]);
                return Ok(s);
            }
        }
    }
    let data = addr.as_bytes();
    if data.len() > 75 {
        return Err("relay address too long for OP_RETURN".into());
    }
    let mut script = Vec::with_capacity(2 + data.len());
    script.push(0x6a);
    script.push(data.len() as u8);
    script.extend_from_slice(data);
    Ok(script)
}

fn build_coinbase(height: u64, reward: u64) -> Result<Transaction, String> {
    let pkh = miner_pubkey_hash().ok_or_else(|| {
        "missing or invalid miner payout address; set IRIUM_MINER_ADDRESS to a valid Irium address"
            .to_string()
    })?;
    let mut s = Vec::with_capacity(25);
    s.extend_from_slice(&[0x76, 0xa9, 0x14]);
    s.extend_from_slice(&pkh);
    s.extend_from_slice(&[0x88, 0xac]);
    Ok(Transaction {
        version: 1,
        inputs: vec![TxInput {
            prev_txid: [0u8; 32],
            prev_index: 0xffff_ffff,
            script_sig: format!("Block {height}").into_bytes(),
            sequence: 0xffff_ffff,
        }],
        outputs: vec![TxOutput {
            value: reward,
            script_pubkey: s,
        }],
        locktime: 0,
    })
}

// =============================================================================
// Target → 8 big-endian u32 words
// =============================================================================

fn target_to_words(target: Target) -> [u32; 8] {
    let bigint = target.to_target();
    let bytes = bigint.to_bytes_be();
    let mut padded = [0u8; 32];
    let n = bytes.len().min(32);
    padded[32 - n..].copy_from_slice(&bytes[bytes.len() - n..]);
    std::array::from_fn(|i| u32::from_be_bytes(padded[i * 4..i * 4 + 4].try_into().unwrap()))
}

// =============================================================================
// GPU miner
// =============================================================================

// Buffers and kernel are allocated once and reused across batches to eliminate
// per-batch GPU memory allocation overhead (was causing ~11% idle time).
struct GpuMiner {
    queue: Queue,
    kernel: Kernel,
    midstate_buf: Buffer<u32>,
    tail_buf: Buffer<u32>,
    target_buf: Buffer<u32>,
    result_buf: Buffer<u32>,
    batch_size: usize,
}

// GpuMiner owns its own OpenCL context/queue/kernel — no shared mutable state between
// instances.  The ocl crate uses raw CL handles which are not auto-Send; we assert Send
// because each GpuMiner is moved into exactly one thread and never accessed concurrently.
// Sync is intentionally NOT implemented: all mutating methods take &mut self, so the
// compiler enforces exclusive access.
unsafe impl Send for GpuMiner {}

impl GpuMiner {
    fn new(platform: Platform, platform_idx: usize, device_idx: usize, batch_size: usize) -> Result<Self, String> {
        let devices = Device::list_all(platform).map_err(|e| format!("OpenCL device list: {e}"))?;
        if devices.is_empty() {
            return Err(format!(
                "No OpenCL devices found on platform {platform_idx}.\n\
                 Install your GPU driver and the ICD loader:\n\
                 apt install ocl-icd-opencl-dev"
            ));
        }
        let device = *devices.get(device_idx).ok_or_else(|| {
            format!(
                "device index {device_idx} out of range on platform {platform_idx} \
                 (found {} device(s)); run --list-platforms to see available devices",
                devices.len()
            )
        })?;

        let plat_name = platform.name().unwrap_or_else(|_| "?".into());
        let dev_name  = device.name().unwrap_or_else(|_| "?".into());
        println!("[GPU] Platform {platform_idx} ({plat_name}), Device {device_idx}: {dev_name}");

        let context = Context::builder()
            .platform(platform)
            .devices(device)
            .build()
            .map_err(|e| format!("OpenCL context: {e}"))?;
        let queue = Queue::new(&context, device, None).map_err(|e| format!("OpenCL queue: {e}"))?;
        let program = Program::builder()
            .src(KERNEL_SRC)
            .devices(device)
            .build(&context)
            .map_err(|e| format!("OpenCL compile error:\n{e}"))?;

        let ocl_err = |e: ocl::Error| e.to_string();

        // Allocate persistent buffers once
        let midstate_buf = Buffer::<u32>::builder()
            .queue(queue.clone())
            .flags(flags::MEM_READ_WRITE)
            .len(8)
            .build()
            .map_err(ocl_err)?;
        let tail_buf = Buffer::<u32>::builder()
            .queue(queue.clone())
            .flags(flags::MEM_READ_WRITE)
            .len(3)
            .build()
            .map_err(ocl_err)?;
        let target_buf = Buffer::<u32>::builder()
            .queue(queue.clone())
            .flags(flags::MEM_READ_WRITE)
            .len(8)
            .build()
            .map_err(ocl_err)?;
        let result_buf = Buffer::<u32>::builder()
            .queue(queue.clone())
            .flags(flags::MEM_READ_WRITE)
            .len(2)
            .build()
            .map_err(ocl_err)?;

        // Build kernel once, referencing the persistent buffers
        let kernel = Kernel::builder()
            .program(&program)
            .name("sha256d_mine")
            .queue(queue.clone())
            .global_work_size(batch_size)
            .arg(&midstate_buf) // arg 0
            .arg(&tail_buf) // arg 1
            .arg(0u32) // arg 2: nonce_base (updated each batch via set_arg)
            .arg(&target_buf) // arg 3
            .arg(&result_buf) // arg 4
            .build()
            .map_err(ocl_err)?;

        println!("[GPU] Kernel compiled successfully.");
        Ok(Self {
            queue,
            kernel,
            midstate_buf,
            tail_buf,
            target_buf,
            result_buf,
            batch_size,
        })
    }

    /// Upload a new midstate + tail + target (call once per template).
    fn update_template(
        &mut self,
        midstate: &[u32; 8],
        tail: &[u32; 3],
        target: &[u32; 8],
    ) -> Result<(), String> {
        let e = |e: ocl::Error| e.to_string();
        self.midstate_buf
            .write(midstate as &[u32])
            .enq()
            .map_err(e)?;
        self.tail_buf.write(tail as &[u32]).enq().map_err(e)?;
        self.target_buf.write(target as &[u32]).enq().map_err(e)?;
        Ok(())
    }

    /// Upload an updated tail only (call when timestamp changes).
    fn update_tail(&mut self, tail: &[u32; 3]) -> Result<(), String> {
        self.tail_buf
            .write(tail as &[u32])
            .enq()
            .map_err(|e: ocl::Error| e.to_string())
    }

    /// Test nonces [nonce_base, nonce_base + batch_size).
    /// Returns `Some(nonce)` on a hit, `None` if no valid nonce was found.
    fn mine_batch(&mut self, nonce_base: u32) -> Result<Option<u32>, String> {
        let e = |e: ocl::Error| e.to_string();

        // Reset result flag (only 2 words — minimal write)
        self.result_buf
            .write(&[0u32, 0u32] as &[u32])
            .enq()
            .map_err(e)?;

        // Update the nonce_base scalar arg in the already-built kernel
        self.kernel.set_arg(2, nonce_base).map_err(e)?;

        // Safety: the kernel was built from verified source, all buffer args are valid
        // and alive for the duration of this call, and queue.finish() below ensures the
        // GPU work completes before we read back results.
        unsafe {
            self.kernel.enq().map_err(e)?;
        }
        self.queue.finish().map_err(e)?;

        let mut result = [0u32; 2];
        self.result_buf.read(&mut result[..]).enq().map_err(e)?;

        Ok(if result[0] != 0 {
            Some(result[1])
        } else {
            None
        })
    }
}

// =============================================================================
// Helpers: extract tail words from a serialised header
// =============================================================================

fn tail_from_header(ser: &[u8]) -> [u32; 3] {
    // bytes 64–75 of the serialised header as big-endian uint32 words
    [
        u32::from_be_bytes(ser[64..68].try_into().unwrap()),
        u32::from_be_bytes(ser[68..72].try_into().unwrap()),
        u32::from_be_bytes(ser[72..76].try_into().unwrap()),
    ]
}

// =============================================================================
// Multi-GPU initialisation
// =============================================================================

// =============================================================================
// Platform and device enumeration
// =============================================================================

/// Returns true if the platform vendor looks like a discrete GPU (NVIDIA or AMD).
fn vendor_is_discrete(vendor: &str) -> bool {
    let v = vendor.to_lowercase();
    v.contains("nvidia") || v.contains("amd") || v.contains("advanced micro")
}

/// Enumerate every OpenCL platform and the display names of its devices.
fn enumerate_platforms() -> Vec<(Platform, String, Vec<String>)> {
    Platform::list()
        .into_iter()
        .map(|p| {
            let vendor = p.name().unwrap_or_else(|_| "Unknown".into());
            let dev_names = Device::list_all(p)
                .unwrap_or_default()
                .into_iter()
                .map(|d| d.name().unwrap_or_else(|_| "Unknown".into()))
                .collect();
            (p, vendor, dev_names)
        })
        .collect()
}

/// Print all detected platforms and devices (for --list-platforms).
fn print_platforms(platforms: &[(Platform, String, Vec<String>)]) {
    if platforms.is_empty() {
        println!("[GPU] No OpenCL platforms found.");
        println!("      Install your GPU driver and: apt install ocl-icd-opencl-dev");
        return;
    }
    println!("[GPU] OpenCL platforms detected:");
    for (i, (_, vendor, devices)) in platforms.iter().enumerate() {
        println!("  Platform {i}: {vendor} ({} device(s))", devices.len());
        for (j, name) in devices.iter().enumerate() {
            println!("    Device {j}: {name}");
        }
    }
}

/// Auto-select the best platform: prefer NVIDIA/AMD discrete GPUs over Intel iGPU.
fn auto_select_platform(platforms: &[(Platform, String, Vec<String>)]) -> usize {
    for (i, (_, vendor, devs)) in platforms.iter().enumerate() {
        if !devs.is_empty() && vendor_is_discrete(vendor) {
            return i;
        }
    }
    for (i, (_, _, devs)) in platforms.iter().enumerate() {
        if !devs.is_empty() {
            return i;
        }
    }
    0
}

/// Resolve a platform selection: numeric index or vendor name substring.
fn resolve_platform_idx(
    platforms: &[(Platform, String, Vec<String>)],
    sel: Option<&str>,
) -> Result<usize, String> {
    match sel {
        None => Ok(auto_select_platform(platforms)),
        Some(s) => {
            if let Ok(n) = s.parse::<usize>() {
                if n >= platforms.len() {
                    return Err(format!(
                        "platform index {n} out of range (found {} platform(s)); \
                         run --list-platforms to see available platforms",
                        platforms.len()
                    ));
                }
                Ok(n)
            } else {
                let lo = s.to_lowercase();
                platforms
                    .iter()
                    .position(|(_, vendor, _)| vendor.to_lowercase().contains(&lo))
                    .ok_or_else(|| format!("no OpenCL platform matching '{s}'; run --list-platforms to see options"))
            }
        }
    }
}

/// Resolve which (platform_idx, device_idx) pairs to mine on.
fn resolve_devices(
    platforms: &[(Platform, String, Vec<String>)],
    platform_sel: Option<&str>,
) -> Result<Vec<(usize, usize)>, String> {
    if platforms.is_empty() {
        return Err(
            "No OpenCL platforms found.\n\
             Install your GPU driver and the ICD loader:\n\
             apt install ocl-icd-opencl-dev"
                .into(),
        );
    }
    let plat_idx = resolve_platform_idx(platforms, platform_sel)?;
    let dev_count = platforms[plat_idx].2.len();
    if dev_count == 0 {
        return Err(format!(
            "Platform {plat_idx} ({}) has no devices; run --list-platforms to see options",
            platforms[plat_idx].1
        ));
    }
    if let Ok(val) = env::var("IRIUM_GPU_DEVICES") {
        let idxs: Vec<usize> = val
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if !idxs.is_empty() {
            for &d in &idxs {
                if d >= dev_count {
                    return Err(format!(
                        "device index {d} out of range (platform {plat_idx} has {dev_count} device(s))"
                    ));
                }
            }
            return Ok(idxs.into_iter().map(|d| (plat_idx, d)).collect());
        }
    }
    if let Ok(val) = env::var("IRIUM_GPU_DEVICE") {
        if let Ok(d) = val.trim().parse::<usize>() {
            if d >= dev_count {
                return Err(format!(
                    "device index {d} out of range (platform {plat_idx} has {dev_count} device(s))"
                ));
            }
            return Ok(vec![(plat_idx, d)]);
        }
    }
    Ok((0..dev_count).map(|d| (plat_idx, d)).collect())
}

/// Initialise one GpuMiner per resolved (platform_idx, device_idx) pair.
fn init_gpus(
    platforms: &[(Platform, String, Vec<String>)],
    device_refs: &[(usize, usize)],
    batch_size: usize,
) -> Result<Vec<GpuMiner>, String> {
    device_refs
        .iter()
        .map(|&(plat_idx, dev_idx)| GpuMiner::new(platforms[plat_idx].0, plat_idx, dev_idx, batch_size))
        .collect()
}

// =============================================================================
// BigUint → [u32; 8] big-endian words (for share targets from Stratum)
// =============================================================================

fn bigint_to_words(n: &BigUint) -> [u32; 8] {
    let bytes = n.to_bytes_be();
    let mut padded = [0u8; 32];
    let start = 32usize.saturating_sub(bytes.len());
    padded[start..].copy_from_slice(&bytes[bytes.len().saturating_sub(32)..]);
    std::array::from_fn(|i| u32::from_be_bytes(padded[i * 4..i * 4 + 4].try_into().unwrap()))
}

// =============================================================================
// Stratum protocol
// =============================================================================

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
    // Use the mining address as the Stratum username (pool convention)
    env::var("IRIUM_STRATUM_USER")
        .or_else(|_| env::var("IRIUM_MINER_ADDRESS"))
        .unwrap_or_else(|_| "irium".to_string())
}

fn stratum_pass() -> String {
    env::var("IRIUM_STRATUM_PASS").unwrap_or_else(|_| "x".to_string())
}

fn stratum_normalize_url(url: &str) -> String {
    let s = url.trim();
    for prefix in ["stratum+tcp://", "stratum://", "tcp://"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    s.to_string()
}

fn stratum_send(writer: &Mutex<TcpStream>, value: &serde_json::Value) -> Result<(), String> {
    let mut stream = writer.lock().unwrap_or_else(|e| e.into_inner());
    let line = format!("{}\n", value);
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
        return Err("stratum EOF".into());
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
    pow_limit * BigUint::from(scale) / BigUint::from(scaled)
}

fn stratum_target_from_hex(hex_str: &str) -> Option<BigUint> {
    hex::decode(hex_str)
        .ok()
        .map(|b| BigUint::from_bytes_be(&b))
}

fn parse_u32_hex(s: &str) -> Result<u32, String> {
    u32::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|e| format!("hex u32: {e}"))
}

fn parse_bits(s: &str) -> Result<u32, String> {
    u32::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|e| format!("bits: {e}"))
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
        let b = hex::decode(branch).map_err(|e| format!("merkle branch: {e}"))?;
        let mut data = Vec::with_capacity(64);
        data.extend_from_slice(&merkle);
        data.extend_from_slice(&b);
        merkle = sha256d(&data);
    }
    Ok(merkle)
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
                eprintln!("[Stratum] Read error: {e}");
                break;
            }
        };
        let method = msg.get("method").and_then(|m| m.as_str());
        let params = msg.get("params").and_then(|p| p.as_array());
        match (method, params) {
            (Some("mining.set_difficulty"), Some(p)) => {
                if let Some(diff) = p.first().and_then(|v| v.as_f64()) {
                    let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
                    g.difficulty = diff;
                    g.target = None;
                    println!("[Stratum] Difficulty: {diff}");
                }
            }
            (Some("mining.set_target"), Some(p)) => {
                if let Some(t) = p.first().and_then(|v| v.as_str()) {
                    let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
                    g.target = stratum_target_from_hex(t);
                }
            }
            (Some("mining.set_extranonce"), Some(p)) => {
                if let (Some(en1), Some(size)) = (
                    p.first().and_then(|v| v.as_str()),
                    p.get(1).and_then(|v| v.as_u64()),
                ) {
                    let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
                    g.extranonce1 = en1.to_string();
                    g.extranonce2_size = size as usize;
                }
            }
            (Some("mining.notify"), Some(p)) if p.len() >= 9 => {
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
                                .collect()
                        })
                        .unwrap_or_default(),
                    version: p[5].as_str().unwrap_or("").to_string(),
                    nbits: p[6].as_str().unwrap_or("").to_string(),
                    ntime: p[7].as_str().unwrap_or("").to_string(),
                };
                println!("[Stratum] New job: {}", job.job_id);
                let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
                g.job = Some(job);
                job_version.fetch_add(1, Ordering::SeqCst);
            }
            _ => {}
        }
    }
}

/// GPU Stratum mining for one job/extranonce2 combination.
/// Returns Ok(true) if the job changed (new job available), Ok(false) if nonces exhausted.
///
/// `gpu_idx` / `num_gpus` partition the 32-bit nonce space so that each GPU covers
/// a disjoint sub-range (nonce_base starts at gpu_idx·batch_size, strides by num_gpus·batch_size).
fn mine_stratum_job_gpu(
    gpu: &mut GpuMiner,
    gpu_idx: usize,
    num_gpus: usize,
    job: &StratumJob,
    extranonce1: &str,
    extranonce2: &str,
    share_target: &BigUint,
    writer: &Mutex<TcpStream>,
    user: &str,
    submit_id: &AtomicU64,
    job_version: u64,
    job_version_ref: &Arc<AtomicU64>,
    total_hashes: &AtomicU64,
    rate_start: &Instant,
    last_log: &Mutex<Instant>,
) -> Result<bool, String> {
    let merkle_root = merkle_root_from_stratum(job, extranonce1, extranonce2)?;
    let version = parse_u32_hex(&job.version)?;
    let bits = parse_bits(&job.nbits)?;
    let time = parse_u32_hex(&job.ntime)?;

    let prev_bytes = hex::decode(&job.prev_hash).map_err(|e| format!("prev_hash: {e}"))?;
    if prev_bytes.len() != 32 {
        return Err(format!("prev_hash len {} != 32", prev_bytes.len()));
    }
    let mut prev_hash = [0u8; 32];
    prev_hash.copy_from_slice(&prev_bytes);

    let header = BlockHeader {
        version,
        prev_hash,
        merkle_root,
        time,
        bits,
        nonce: 0,
    };
    let ser = header.serialize();
    let midstate = sha256_midstate(ser[..64].try_into().unwrap());
    let mut tail = tail_from_header(&ser);
    let share_words = bigint_to_words(share_target);
    let network_target = Target { bits }.to_target();

    gpu.update_template(&midstate, &tail, &share_words)?;

    // Each GPU mines a disjoint sub-range of the nonce space.
    // GPU i starts at offset i·batch_size and advances by num_gpus·batch_size.
    let stride = (num_gpus as u32).wrapping_mul(gpu.batch_size as u32);
    let mut nonce_base: u32 = (gpu_idx as u32).wrapping_mul(gpu.batch_size as u32);
    let mut current_time = time;
    let mut local_log = Instant::now();

    loop {
        // Check for new job
        if job_version_ref.load(Ordering::SeqCst) != job_version {
            return Ok(true);
        }

        match gpu.mine_batch(nonce_base)? {
            Some(nonce) => {
                // Submit share
                let submit = json!({
                    "id": submit_id.fetch_add(1, Ordering::SeqCst),
                    "method": "mining.submit",
                    "params": [user, job.job_id.as_str(), extranonce2,
                               job.ntime.as_str(), format!("{:08x}", nonce)]
                });
                let _ = stratum_send(writer, &submit);

                // Check if it also meets network target
                let header_found = BlockHeader {
                    version,
                    prev_hash,
                    merkle_root,
                    time: current_time,
                    bits,
                    nonce,
                };
                let hash = header_found.hash();
                let hash_val = BigUint::from_bytes_be(&hash);
                if hash_val <= network_target {
                    println!(
                        "[GPU {gpu_idx}/Stratum] ✅ Share meets NETWORK target! hash={}",
                        hex::encode(hash)
                    );
                } else {
                    println!("[GPU {gpu_idx}/Stratum] Share submitted: nonce={nonce:08x}");
                }

                total_hashes.fetch_add(gpu.batch_size as u64, Ordering::Relaxed);
            }
            None => {
                total_hashes.fetch_add(gpu.batch_size as u64, Ordering::Relaxed);
            }
        }

        // Progress log — GPU 0 is responsible for the shared rate display.
        // Other GPUs still track local time to avoid fighting over the mutex.
        if gpu_idx == 0 && local_log.elapsed() >= Duration::from_secs(10) {
            if let Ok(mut guard) = last_log.try_lock() {
                if guard.elapsed() >= Duration::from_secs(10) {
                    let elapsed = rate_start.elapsed().as_secs_f64();
                    let hashes = total_hashes.load(Ordering::Relaxed);
                    let rate = if elapsed > 0.0 {
                        hashes as f64 / elapsed
                    } else {
                        0.0
                    };
                    if json_log_enabled() {
                        println!(
                            "{}",
                            json!({"event":"progress","rate_hs":rate,"hashes":hashes,"ts":Utc::now().format("%H:%M:%S").to_string()})
                        );
                    } else {
                        println!(
                            "[GPU/Stratum] {}  ({} MH total)",
                            fmt_rate(rate),
                            hashes / 1_000_000
                        );
                    }
                    *guard = Instant::now();
                }
            }
            local_log = Instant::now();
        }

        // Advance nonce window; bump timestamp when this GPU's sub-range is exhausted.
        let (next, overflow) = nonce_base.overflowing_add(stride);
        if overflow {
            current_time = (Utc::now().timestamp() as u32).max(current_time + 1);
            let header_t = BlockHeader {
                version,
                prev_hash,
                merkle_root,
                time: current_time,
                bits,
                nonce: 0,
            };
            let new_ser = header_t.serialize();
            tail = tail_from_header(&new_ser);
            gpu.update_tail(&tail)?;
            // Reset to this GPU's starting offset
            nonce_base = (gpu_idx as u32).wrapping_mul(gpu.batch_size as u32);
        } else {
            nonce_base = next;
        }
    }
}

fn run_stratum_gpu(gpus: &mut [GpuMiner]) -> Result<(), String> {
    let url = stratum_url().ok_or("IRIUM_STRATUM_URL not set")?;
    let addr = stratum_normalize_url(&url);
    let num_gpus = gpus.len();

    println!("[Stratum] Connecting to {addr}… ({num_gpus} GPU(s))");
    let stream = TcpStream::connect(&addr).map_err(|e| format!("connect: {e}"))?;
    let _ = stream.set_nodelay(true);
    let writer = Arc::new(Mutex::new(stream));
    let mut reader = BufReader::new(
        writer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_clone()
            .map_err(|e| e.to_string())?,
    );

    // Subscribe
    stratum_send(
        &writer,
        &json!({"id":1,"method":"mining.subscribe","params":["irium-miner-gpu/0.1"]}),
    )?;
    let sub_resp = stratum_read_line(&mut reader)?;
    let (extranonce1, extranonce2_size) = match sub_resp.get("result").and_then(|v| v.as_array()) {
        Some(arr) if arr.len() >= 3 => (
            arr[1].as_str().unwrap_or("").to_string(),
            arr[2].as_u64().unwrap_or(4) as usize,
        ),
        _ => return Err("stratum subscribe failed".into()),
    };
    println!("[Stratum] extranonce1={extranonce1} extranonce2_size={extranonce2_size}");

    // Authorize
    let user = stratum_user();
    let pass = stratum_pass();
    stratum_send(
        &writer,
        &json!({"id":2,"method":"mining.authorize","params":[user.clone(), pass]}),
    )?;
    println!("[Stratum] Authorized as {user}");

    let state = Arc::new(Mutex::new(StratumState {
        extranonce1,
        extranonce2_size,
        difficulty: 1.0,
        target: None,
        job: None,
    }));
    let job_version = Arc::new(AtomicU64::new(0));
    std::thread::spawn({
        let s = Arc::clone(&state);
        let jv = Arc::clone(&job_version);
        move || stratum_reader(reader, s, jv)
    });

    let submit_id = Arc::new(AtomicU64::new(10));
    let mut extranonce_counter: u64 = 0;
    let mut last_job_version = u64::MAX;
    let total_hashes = Arc::new(AtomicU64::new(0));
    let mut rate_start = Instant::now();
    let last_log = Arc::new(Mutex::new(Instant::now()));

    loop {
        let (job, extranonce1, extranonce2_size, share_target) = {
            let g = state.lock().unwrap_or_else(|e| e.into_inner());
            let tgt = g
                .target
                .clone()
                .unwrap_or_else(|| stratum_target_from_difficulty(g.difficulty));
            (
                g.job.clone(),
                g.extranonce1.clone(),
                g.extranonce2_size,
                tgt,
            )
        };

        let job = match job {
            Some(j) => j,
            None => {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
        };

        let current_version = job_version.load(Ordering::SeqCst);
        if current_version != last_job_version {
            extranonce_counter = 0;
            last_job_version = current_version;
            total_hashes.store(0, Ordering::Relaxed);
            rate_start = Instant::now();
        }

        let width = extranonce2_size * 2;
        let extranonce2 = format!("{:0width$x}", extranonce_counter, width = width);

        // Run all GPUs in parallel; each mines its own nonce sub-range.
        // All threads exit when the job changes (job_version_ref flips).
        // iter_mut() gives each thread exclusive (&mut GpuMiner) access — no Sync required.
        let any_error = Arc::new(AtomicBool::new(false));
        std::thread::scope(|s| {
            for (gpu_idx, gpu) in gpus.iter_mut().enumerate() {
                let job = &job;
                let extranonce1 = extranonce1.as_str();
                let extranonce2 = extranonce2.as_str();
                let share_target = &share_target;
                let writer = &writer;
                let user = user.as_str();
                let submit_id = &*submit_id;
                let job_version_ref = &job_version;
                let total_hashes = &*total_hashes;
                let rate_start = &rate_start;
                let last_log = &*last_log;
                let any_error = &*any_error;

                s.spawn(move || {
                    if let Err(e) = mine_stratum_job_gpu(
                        gpu,
                        gpu_idx,
                        num_gpus,
                        job,
                        extranonce1,
                        extranonce2,
                        share_target,
                        writer,
                        user,
                        submit_id,
                        current_version,
                        job_version_ref,
                        total_hashes,
                        rate_start,
                        last_log,
                    ) {
                        eprintln!("[GPU {gpu_idx}/Stratum] Error: {e}");
                        any_error.store(true, Ordering::SeqCst);
                    }
                });
            }
        });

        if any_error.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_secs(1));
        }
        // If job changed, the outer loop will pick it up; otherwise nonces exhausted →
        // increment extranonce_counter (though in practice the loop runs until job changes).
        let new_version = job_version.load(Ordering::SeqCst);
        if new_version == current_version {
            extranonce_counter = extranonce_counter.saturating_add(1);
        }
    }
}

// =============================================================================

fn print_usage() {
    eprintln!(
        "Usage: irium-miner-gpu [OPTIONS]



Options:

  --pool            <url>     Stratum pool URL

  --wallet          <addr>    Mining/payout address

  --platform        <n|name>  OpenCL platform index or vendor name substring

                              (default: auto, prefers NVIDIA/AMD over Intel)

  --device          <n>       Device index within selected platform (default: 0)

  --devices         <n,n,...> Comma-separated device indices (multi-GPU)

  --batch           <n>       Nonces per GPU dispatch (default: 4194304)

  --rpc             <url>     Node RPC URL for solo mining (env: IRIUM_NODE_RPC)

  --list-platforms            List all OpenCL platforms and devices, then exit

  --help                      Show this message



Environment variables:

  IRIUM_STRATUM_URL, IRIUM_MINER_ADDRESS, IRIUM_GPU_PLATFORM,

  IRIUM_GPU_DEVICE, IRIUM_GPU_DEVICES, IRIUM_GPU_BATCH, IRIUM_NODE_RPC



CLI flags take priority over environment variables."
    );
}

fn main() {
    // Load .env file if present (same search order as the CPU miner)
    for path in [".env", "miner.env", "irium.env"] {
        if load_env_file(path) {
            break;
        }
    }

    // Parse CLI args and override env vars
    let mut args = std::env::args().skip(1).peekable();
    let mut list_platforms_flag = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--pool" => {
                let val = args.next().unwrap_or_else(|| {
                    eprintln!("--pool requires a value");
                    std::process::exit(1);
                });
                env::set_var("IRIUM_STRATUM_URL", val);
            }
            "--wallet" => {
                let val = args.next().unwrap_or_else(|| {
                    eprintln!("--wallet requires a value");
                    std::process::exit(1);
                });
                env::set_var("IRIUM_MINER_ADDRESS", val);
            }
            "--device" | "--devices" => {
                let val = args.next().unwrap_or_else(|| {
                    eprintln!("{arg} requires a value");
                    std::process::exit(1);
                });
                // If the value contains a comma, treat it as a multi-GPU list.
                if val.contains(',') {
                    env::set_var("IRIUM_GPU_DEVICES", val);
                } else {
                    env::set_var("IRIUM_GPU_DEVICE", val);
                }
            }
            "--platform" => {
                let val = args.next().unwrap_or_else(|| {
                    eprintln!("--platform requires a value (index or vendor name substring)");
                    std::process::exit(1);
                });
                env::set_var("IRIUM_GPU_PLATFORM", val);
            }
            "--list-platforms" => {
                list_platforms_flag = true;
            }
            "--batch" => {
                let val = args.next().unwrap_or_else(|| {
                    eprintln!("--batch requires a value");
                    std::process::exit(1);
                });
                env::set_var("IRIUM_GPU_BATCH", val);
            }
            "--rpc" => {
                let val = args.next().unwrap_or_else(|| {
                    eprintln!("--rpc requires a value");
                    std::process::exit(1);
                });
                env::set_var("IRIUM_NODE_RPC", val);
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                print_usage();
                std::process::exit(1);
            }
        }
    }

    // --list-platforms does not need a mining address; check it before the address guard.
    let platforms = enumerate_platforms();
    if list_platforms_flag {
        print_platforms(&platforms);
        std::process::exit(0);
    }

    if miner_pubkey_hash().is_none() {
        eprintln!(
            "error: missing or invalid miner payout address; set IRIUM_MINER_ADDRESS (base58) or IRIUM_MINER_PKH (40-hex)"
        );
        std::process::exit(1);
    }

    let batch_size: usize = env::var("IRIUM_GPU_BATCH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1 << 22); // 4 194 304 nonces per dispatch

    println!(
        "[GPU] Batch size: {} ({:.1}M nonces/dispatch)",
        batch_size,
        batch_size as f64 / 1_000_000.0
    );

    let platform_sel = env::var("IRIUM_GPU_PLATFORM").ok();
    let device_refs = match resolve_devices(&platforms, platform_sel.as_deref()) {
        Ok(refs) => refs,
        Err(e) => {
            eprintln!("[GPU] Device selection error: {e}");
            std::process::exit(1);
        }
    };
    let mut gpus = match init_gpus(&platforms, &device_refs, batch_size) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("[GPU] Initialisation failed: {e}");
            std::process::exit(1);
        }
    };
    println!("[GPU] {} device(s) initialised.", gpus.len());

    // If IRIUM_STRATUM_URL is set, use pool/Stratum mode; otherwise solo GBT mode.
    if stratum_url().is_some() {
        loop {
            if let Err(e) = run_stratum_gpu(&mut gpus) {
                eprintln!("[Stratum] Disconnected: {e}. Reconnecting in 5 s…");
                std::thread::sleep(Duration::from_secs(5));
            }
        }
    }

    let client = match rpc_client() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[GPU] RPC client error: {e}");
            std::process::exit(1);
        }
    };

    let mut total_hashes: u64 = 0;
    let mut rate_start = Instant::now();
    // Refresh the block template at least every 30 s (or immediately after a
    // found block).
    let template_ttl = Duration::from_secs(30);

    loop {
        // ── Fetch block template ──────────────────────────────────────────────
        let template = match fetch_template(&client) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[GPU] {e}; retrying in 5 s…");
                std::thread::sleep(Duration::from_secs(5));
                continue;
            }
        };

        let height = template.height;
        let bits =
            u32::from_str_radix(template.bits.trim_start_matches("0x"), 16).unwrap_or(0x1d00_ffff);
        let target = Target { bits };
        let target_words = target_to_words(target);

        let prev_hash: [u8; 32] = match hex::decode(&template.prev_hash) {
            Ok(b) if b.len() == 32 => b.try_into().unwrap(),
            _ => {
                eprintln!("[GPU] Malformed prev_hash; retrying…");
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
        };

        // ── Build the block ───────────────────────────────────────────────────
        let mut total_fees: i64 = 0;
        let mut relay_addrs: Vec<String> = Vec::new();
        let mut mempool_txs: Vec<Transaction> = Vec::new();

        for tx in &template.txs {
            if let Ok(raw) = hex::decode(&tx.hex) {
                total_fees = total_fees.saturating_add(tx.fee.unwrap_or(0) as i64);
                if let Some(addrs) = &tx.relay_addresses {
                    for a in addrs {
                        if relay_addrs.len() < 3 && !relay_addrs.contains(a) {
                            relay_addrs.push(a.clone());
                        }
                    }
                }
                mempool_txs.push(decode_compact_tx(&raw));
            }
        }

        // Relay commitments (10 % of fees, split 50/30/20 across up to 3 relayers)
        let relay_pool = (total_fees as u64) / 10;
        let relay_commitments: Vec<RelayCommitment> = if relay_pool > 0 {
            [50u64, 30, 20]
                .iter()
                .enumerate()
                .filter_map(|(i, w)| {
                    let amt = relay_pool * w / 100;
                    if amt == 0 {
                        return None;
                    }
                    let addr = relay_addrs
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| "RELAY_PLACEHOLDER".to_string());
                    Some(RelayCommitment {
                        address: addr,
                        amount: amt,
                        memo: Some(format!("relay-{i}")),
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        let relay_total: u64 = relay_commitments.iter().map(|c| c.amount).sum();
        let reward = block_reward(height);
        let miner_reward = reward + (total_fees as u64).saturating_sub(relay_total);

        let mut coinbase = match build_coinbase(height, miner_reward) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[GPU] fatal: {e}");
                std::process::exit(1);
            }
        };
        for rc in &relay_commitments {
            if let Ok(outputs) = rc.build_outputs(|addr| script_from_relay_address(addr)) {
                coinbase.outputs.extend(outputs);
            }
        }
        if let Some(meta) = coinbase_metadata_output() {
            coinbase.outputs.push(meta);
        }

        let mut txs = vec![coinbase];
        txs.extend(mempool_txs);

        let header_time = template.time.max(Utc::now().timestamp() as u32);
        let mut block = Block {
            header: BlockHeader {
                version: 1,
                prev_hash,
                merkle_root: [0u8; 32],
                time: header_time,
                bits,
                nonce: 0,
            },
            transactions: txs.clone(),
        };
        block.header.merkle_root = block.merkle_root();

        // ── Pre-compute midstate (constant for the whole template) ────────────
        let ser = block.header.serialize();
        let midstate = sha256_midstate(ser[..64].try_into().unwrap());
        let ser = Arc::new(ser); // shared across GPU threads

        // Upload the template to every GPU.
        let mut upload_ok = true;
        for (i, gpu) in gpus.iter_mut().enumerate() {
            if let Err(e) = gpu.update_template(&midstate, &tail_from_header(&ser), &target_words) {
                eprintln!("[GPU {i}] update_template error: {e}");
                upload_ok = false;
            }
        }
        if !upload_ok {
            continue;
        }

        if json_log_enabled() {
            println!(
                "{}",
                json!({"event":"mining","height":height,"bits":template.bits,"ts":Utc::now().format("%H:%M:%S").to_string()})
            );
        } else {
            println!(
                "[GPU] Mining height {} (prev {}…) — {} GPU(s)",
                height,
                &template.prev_hash[..8.min(template.prev_hash.len())],
                gpus.len()
            );
        }

        // ── Multi-GPU mining loop ─────────────────────────────────────────────
        // Shared state between GPU worker threads.
        let template_fetched_at = Instant::now();
        let round_start = Instant::now(); // per-round timer for accurate hashrate display
        let stop = Arc::new(AtomicBool::new(false));
        // (nonce, time_used) — set by the winning GPU thread.
        let found_result: Arc<Mutex<Option<(u32, u32)>>> = Arc::new(Mutex::new(None));
        let solo_hashes = Arc::new(AtomicU64::new(0));
        let solo_log = Arc::new(Mutex::new(Instant::now()));
        let num_gpus = gpus.len();

        // iter_mut() gives each scoped thread exclusive (&mut GpuMiner) access — no Sync required.
        std::thread::scope(|s| {
            for (gpu_idx, gpu) in gpus.iter_mut().enumerate() {
                let stop = Arc::clone(&stop);
                let found_result = Arc::clone(&found_result);
                let solo_hashes = Arc::clone(&solo_hashes);
                let solo_log = Arc::clone(&solo_log);
                let ser = Arc::clone(&ser);
                let round_start = &round_start;
                let template_fetched_at = &template_fetched_at;

                s.spawn(move || {
                    let stride = (num_gpus as u32).wrapping_mul(gpu.batch_size as u32);
                    let mut nonce_base = (gpu_idx as u32).wrapping_mul(gpu.batch_size as u32);
                    let mut current_time = block.header.time;

                    loop {
                        if stop.load(Ordering::SeqCst) { break; }
                        if template_fetched_at.elapsed() > template_ttl {
                            stop.store(true, Ordering::SeqCst);
                            break;
                        }

                        match gpu.mine_batch(nonce_base) {
                            Err(e) => {
                                eprintln!("[GPU {gpu_idx}] Kernel error: {e}");
                                stop.store(true, Ordering::SeqCst);
                                break;
                            }
                            Ok(Some(nonce)) => {
                                *found_result.lock().unwrap() = Some((nonce, current_time));
                                stop.store(true, Ordering::SeqCst);
                                break;
                            }
                            Ok(None) => {
                                solo_hashes.fetch_add(gpu.batch_size as u64, Ordering::Relaxed);

                                // GPU 0 handles the shared progress log.
                                if gpu_idx == 0 {
                                    if let Ok(mut guard) = solo_log.try_lock() {
                                        if guard.elapsed() >= Duration::from_secs(10) {
                                            let elapsed = round_start.elapsed().as_secs_f64();
                                            let hashes = solo_hashes.load(Ordering::Relaxed);
                                            let rate = if elapsed > 0.0 { hashes as f64 / elapsed } else { 0.0 };
                                            if json_log_enabled() {
                                                println!("{}", json!({"event":"progress","height":height,"hashes":hashes,"rate_hs":rate,"ts":Utc::now().format("%H:%M:%S").to_string()}));
                                            } else {
                                                println!("[GPU] Height {height}: {hashes} hashes, {}", fmt_rate(rate));
                                            }
                                            *guard = Instant::now();
                                        }
                                    }
                                }

                                // Advance nonce window; bump timestamp when sub-range exhausted.
                                let (next, overflow) = nonce_base.overflowing_add(stride);
                                if overflow {
                                    current_time = (Utc::now().timestamp() as u32).max(current_time + 1);
                                    let header_t = BlockHeader {
                                        version: block.header.version,
                                        prev_hash: block.header.prev_hash,
                                        merkle_root: block.header.merkle_root,
                                        time: current_time,
                                        bits: block.header.bits,
                                        nonce: 0,
                                    };
                                    let new_ser = header_t.serialize();
                                    let _ = gpu.update_tail(&tail_from_header(&new_ser));
                                    nonce_base = (gpu_idx as u32).wrapping_mul(gpu.batch_size as u32);
                                } else {
                                    nonce_base = next;
                                }
                            }
                        }
                    }
                });
            }
        });

        // Collect accumulated hashes into the outer counter.
        total_hashes += solo_hashes.load(Ordering::Relaxed);

        // Check if any GPU found a valid nonce.
        let found = if let Some((nonce, found_time)) = found_result.lock().unwrap().take() {
            block.header.nonce = nonce;
            block.header.time = found_time;
            let hash = block.header.hash();
            if meets_target(&hash, target) {
                if json_log_enabled() {
                    println!(
                        "{}",
                        json!({"event":"mined_block","height":height,"hash":hex::encode(hash),"nonce":nonce,"ts":Utc::now().format("%H:%M:%S").to_string()})
                    );
                } else {
                    println!("[GPU] ✅ Mined block at height {height}!");
                    println!("       hash  = {}", hex::encode(hash));
                    println!("       nonce = {nonce}");
                    let elapsed = rate_start.elapsed().as_secs_f64();
                    if elapsed > 0.0 {
                        println!("       rate  = {}", fmt_rate(total_hashes as f64 / elapsed));
                    }
                }
                match submit_block(&client, height, &block) {
                    Ok(_) => println!("[GPU] Block submitted successfully."),
                    Err(e) => eprintln!("[GPU] Submit error: {e}"),
                }
                total_hashes = 0;
                rate_start = Instant::now();
            } else {
                eprintln!(
                    "[GPU] Warning: returned nonce {nonce} failed CPU verification \
                     — possible kernel bug"
                );
            }
            true
        } else {
            false
        };

        if !found {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
