use irium_node_rs::constants::COINBASE_MATURITY;
use irium_node_rs::pow::sha256d;
use irium_node_rs::qr::{render_ascii, render_svg};
use irium_node_rs::tx::{Transaction, TxInput, TxOutput};
use k256::ecdsa::signature::hazmat::PrehashSigner;
use k256::ecdsa::{Signature, SigningKey};
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::SecretKey;
use rand_core::{OsRng, RngCore};
use reqwest::blocking::Client;
use ripemd::Ripemd160;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const IRIUM_P2PKH_VERSION: u8 = 0x39;
const DEFAULT_FEE_PER_BYTE: u64 = 1;

#[derive(Serialize, Deserialize)]
struct WalletFile {
    version: u32,
    #[serde(default)]
    seed_hex: Option<String>,
    #[serde(default)]
    next_index: u32,
    keys: Vec<WalletKey>,
}

#[derive(Serialize, Deserialize, Clone)]
struct WalletKey {
    address: String,
    pkh: String,
    pubkey: String,
    privkey: String,
}

#[derive(Deserialize)]
struct LegacyWalletFile {
    keys: HashMap<String, String>,
    #[allow(dead_code)]
    addresses: Option<Vec<String>>,
}

fn base58check_decode(s: &str) -> Option<Vec<u8>> {
    let data = bs58::decode(s).into_vec().ok()?;
    if data.len() < 5 {
        return None;
    }
    let (body, checksum) = data.split_at(data.len() - 4);
    let first = Sha256::digest(body);
    let second = Sha256::digest(&first);
    if &second[0..4] != checksum {
        return None;
    }
    Some(body.to_vec())
}

fn wif_to_secret_and_compression(wif: &str) -> Result<([u8; 32], bool), String> {
    let data = base58check_decode(wif).ok_or_else(|| "invalid WIF".to_string())?;

    // Standard WIF payload: 0x80 || 32-byte secret [|| 0x01 if compressed]
    if data.len() != 33 && data.len() != 34 {
        return Err("invalid WIF length".to_string());
    }
    if data[0] != 0x80 {
        return Err("unsupported WIF version".to_string());
    }

    let compressed = if data.len() == 34 {
        if data[33] != 0x01 {
            return Err("invalid WIF compression flag".to_string());
        }
        true
    } else {
        false
    };

    let mut out = [0u8; 32];
    out.copy_from_slice(&data[1..33]);
    Ok((out, compressed))
}

fn maybe_migrate_legacy_wallet(path: &Path, data: &str) -> Result<Option<WalletFile>, String> {
    let legacy: LegacyWalletFile = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    if legacy.keys.is_empty() {
        return Ok(Some(WalletFile {
            version: 1,
            seed_hex: None,
            next_index: 0,
            keys: Vec::new(),
        }));
    }

    let mut entries: Vec<(String, String)> = legacy.keys.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut keys = Vec::with_capacity(entries.len());
    for (address, wif) in entries {
        let (priv_bytes, compressed) = wif_to_secret_and_compression(&wif)
            .map_err(|e| format!("legacy key for {address}: {e}"))?;

        let secret = SecretKey::from_slice(&priv_bytes)
            .map_err(|e| format!("legacy key for {address}: invalid secret key: {e}"))?;
        let public = secret.public_key();
        let pubkey = public.to_encoded_point(compressed);
        let pkh = hash160(pubkey.as_bytes());
        let derived = base58_p2pkh_from_hash(&pkh);
        if derived != address {
            return Err(format!(
                "legacy key address mismatch: file has {address}, derived {derived}"
            ));
        }

        keys.push(WalletKey {
            address,
            pkh: hex::encode(pkh),
            pubkey: hex::encode(pubkey.as_bytes()),
            privkey: hex::encode(priv_bytes),
        });
    }

    let wallet = WalletFile {
        version: 1,
        seed_hex: None,
        next_index: keys.len() as u32,
        keys,
    };

    // Backup the legacy file before rewriting it.
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup = if let Some(name) = path.file_name() {
        path.with_file_name(format!("{}.legacy.bak.{}", name.to_string_lossy(), ts))
    } else {
        PathBuf::from(format!("{}.legacy.bak.{}", path.display(), ts))
    };

    fs::copy(path, &backup).map_err(|e| format!("backup legacy wallet: {e}"))?;
    eprintln!(
        "[warn] Migrated legacy wallet format to v1; backup saved at: {}",
        backup.display()
    );
    save_wallet(path, &wallet)?;

    Ok(Some(wallet))
}

#[derive(Deserialize)]
struct BalanceResponse {
    balance: u64,
    utxo_count: usize,
    mined_blocks: Option<usize>,
}

#[derive(Deserialize)]
struct UtxosResponse {
    height: u64,
    utxos: Vec<UtxoItem>,
}

#[derive(Deserialize)]
struct HistoryResponse {
    height: u64,
    txs: Vec<HistoryItem>,
}

#[derive(Deserialize)]
struct HistoryItem {
    txid: String,
    height: u64,
    received: u64,
    spent: u64,
    net: i64,
    is_coinbase: bool,
}

#[derive(Deserialize)]
struct FeeEstimateResponse {
    min_fee_per_byte: f64,
    mempool_size: usize,
}

#[derive(Deserialize, Clone)]
struct UtxoItem {
    txid: String,
    index: u32,
    value: u64,
    height: u64,
    is_coinbase: bool,
    script_pubkey: String,
}

#[derive(Serialize)]
struct SubmitTxRequest {
    tx_hex: String,
}

// Base58 P2PKH decoder (version byte + 20-byte hash + 4-byte checksum)
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
    if body[0] != IRIUM_P2PKH_VERSION {
        return None;
    }
    let payload = &body[1..];
    if payload.len() != 20 {
        return None;
    }
    Some(payload.to_vec())
}

fn hash160(data: &[u8]) -> [u8; 20] {
    let sha = Sha256::digest(data);
    let rip = Ripemd160::digest(&sha);
    let mut out = [0u8; 20];
    out.copy_from_slice(&rip);
    out
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

fn wallet_path() -> PathBuf {
    if let Ok(path) = env::var("IRIUM_WALLET_FILE") {
        return PathBuf::from(path);
    }
    let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
    PathBuf::from(home).join(".irium/wallet.json")
}

fn load_wallet(path: &Path) -> Result<WalletFile, String> {
    let data = fs::read_to_string(path).map_err(|e| format!("read wallet: {e}"))?;
    match serde_json::from_str::<WalletFile>(&data) {
        Ok(w) => Ok(w),
        Err(e) => {
            if let Some(w) = maybe_migrate_legacy_wallet(path, &data)? {
                return Ok(w);
            }
            Err(format!("parse wallet: {e}"))
        }
    }
}

fn save_wallet(path: &Path, wallet: &WalletFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create wallet dir: {e}"))?;
    }
    let data =
        serde_json::to_string_pretty(wallet).map_err(|e| format!("serialize wallet: {e}"))?;
    fs::write(path, data).map_err(|e| format!("write wallet: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, perms).map_err(|e| format!("chmod wallet: {e}"))?;
    }
    Ok(())
}

fn ensure_wallet(path: &Path) -> Result<WalletFile, String> {
    let mut wallet = if path.exists() {
        load_wallet(path)?
    } else {
        WalletFile {
            version: 1,
            seed_hex: None,
            next_index: 0,
            keys: Vec::new(),
        }
    };
    if wallet.seed_hex.is_some() && wallet.next_index < wallet.keys.len() as u32 {
        wallet.next_index = wallet.keys.len() as u32;
    }
    Ok(wallet)
}

fn wallet_key_from_secret(secret: &SecretKey, compressed: bool) -> WalletKey {
    let public = secret.public_key();
    let pubkey = public.to_encoded_point(compressed);
    let pkh = hash160(pubkey.as_bytes());
    let address = base58_p2pkh_from_hash(&pkh);
    WalletKey {
        address,
        pkh: hex::encode(pkh),
        pubkey: hex::encode(pubkey.as_bytes()),
        privkey: hex::encode(secret.to_bytes()),
    }
}

fn generate_key() -> WalletKey {
    let secret = SecretKey::random(&mut OsRng);
    wallet_key_from_secret(&secret, true)
}

fn base58check_encode(body: &[u8]) -> String {
    let first = Sha256::digest(body);
    let second = Sha256::digest(&first);
    let mut full = Vec::with_capacity(body.len() + 4);
    full.extend_from_slice(body);
    full.extend_from_slice(&second[0..4]);
    bs58::encode(full).into_string()
}

fn secret_to_wif(secret: &[u8; 32], compressed: bool) -> String {
    let mut body = Vec::with_capacity(34);
    body.push(0x80);
    body.extend_from_slice(secret);
    if compressed {
        body.push(0x01);
    }
    base58check_encode(&body)
}

fn parse_seed_hex(seed_hex: &str) -> Result<[u8; 32], String> {
    let raw = hex::decode(seed_hex).map_err(|_| "seed must be 64-char hex".to_string())?;
    if raw.len() != 32 {
        return Err("seed must be 64-char hex".to_string());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&raw);
    Ok(out)
}

fn generate_seed_hex() -> String {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    hex::encode(seed)
}

fn derive_secret_from_seed_hex(seed_hex: &str, index: u32) -> Result<SecretKey, String> {
    let seed = parse_seed_hex(seed_hex)?;
    let mut material = Vec::with_capacity(36);
    material.extend_from_slice(&seed);
    material.extend_from_slice(&index.to_le_bytes());
    for ctr in 0u32..1024 {
        let mut data = material.clone();
        data.extend_from_slice(&ctr.to_le_bytes());
        let digest = Sha256::digest(&data);
        if let Ok(secret) = SecretKey::from_slice(&digest) {
            return Ok(secret);
        }
    }
    Err("failed to derive valid key from seed".to_string())
}

fn find_key<'a>(wallet: &'a WalletFile, addr: &str) -> Option<&'a WalletKey> {
    wallet.keys.iter().find(|k| k.address == addr)
}

fn usage() {
    eprintln!("Usage:");
    eprintln!("  irium-wallet init [--seed <64hex>]");
    eprintln!("  irium-wallet new-address");
    eprintln!("  irium-wallet list-addresses");
    eprintln!("  irium-wallet export-wif <base58_addr> --out <file>");
    eprintln!("  irium-wallet import-wif <wif>");
    eprintln!("  irium-wallet export-seed --out <file>");
    eprintln!("  irium-wallet import-seed <64hex> [--force]");
    eprintln!("  irium-wallet backup [--out <file>]");
    eprintln!("  irium-wallet restore-backup <file> [--force]");
    eprintln!("  irium-wallet address-to-pkh <base58_addr>");
    eprintln!("  irium-wallet qr <base58_addr> [--svg] [--out <file>]");
    eprintln!("  irium-wallet balance <base58_addr> [--rpc <url>]");
    eprintln!("  irium-wallet list-unspent <base58_addr> [--rpc <url>]");
    eprintln!("  irium-wallet history <base58_addr> [--rpc <url>]");
    eprintln!("  irium-wallet estimate-fee [--rpc <url>]");
    eprintln!("  irium-wallet send <from_addr> <to_addr> <amount_irm> [--fee <irm>] [--coin-select smallest|largest] [--rpc <url>]");
}

fn node_rpc_base() -> String {
    env::var("IRIUM_NODE_RPC").unwrap_or_else(|_| "https://127.0.0.1:38300".to_string())
}

fn default_rpc_url() -> String {
    env::var("IRIUM_NODE_RPC")
        .or_else(|_| env::var("IRIUM_RPC_URL"))
        .unwrap_or_else(|_| node_rpc_base())
}

fn color_enabled() -> bool {
    env::var("NO_COLOR").is_err()
}

fn format_irm(amount: u64) -> String {
    let whole = amount / 100_000_000;
    let frac = amount % 100_000_000;
    if frac == 0 {
        format!("{}", whole)
    } else {
        format!("{}.{}", whole, format!("{:08}", frac))
    }
}

fn parse_irm(s: &str) -> Result<u64, String> {
    if s.trim().is_empty() {
        return Err("empty amount".to_string());
    }
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() > 2 {
        return Err("invalid amount".to_string());
    }
    let whole: u64 = parts[0].parse().map_err(|_| "invalid amount".to_string())?;
    let frac = if parts.len() == 2 {
        let frac_str = parts[1];
        if frac_str.len() > 8 {
            return Err("too many decimals".to_string());
        }
        let mut frac_val: u64 = frac_str.parse().map_err(|_| "invalid amount".to_string())?;
        for _ in frac_str.len()..8 {
            frac_val *= 10;
        }
        frac_val
    } else {
        0
    };
    Ok(whole.saturating_mul(100_000_000).saturating_add(frac))
}

fn estimate_tx_size(inputs: usize, outputs: usize) -> u64 {
    10 + inputs as u64 * 148 + outputs as u64 * 34
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn https_to_http(base: &str) -> Option<String> {
    if let Some(rest) = base.strip_prefix("https://") {
        Some(format!("http://{}", rest))
    } else {
        None
    }
}

fn send_with_https_fallback<F>(
    base: &str,
    f: F,
) -> Result<reqwest::blocking::Response, reqwest::Error>
where
    F: Fn(&str) -> Result<reqwest::blocking::Response, reqwest::Error>,
{
    match f(base) {
        Ok(v) => Ok(v),
        Err(e) => {
            if let Some(http) = https_to_http(base) {
                eprintln!("HTTPS RPC failed, retrying over HTTP: {}", http);
                if let Ok(v) = f(&http) {
                    return Ok(v);
                }
            }
            Err(e)
        }
    }
}

fn rpc_client(base: &str) -> Result<Client, String> {
    let mut builder = Client::builder().timeout(Duration::from_secs(10));
    let ca_path = env::var("IRIUM_RPC_CA").ok().or_else(|| {
        let fallback = Path::new("/etc/irium/tls/irium-ca.crt");
        if fallback.exists() {
            Some(fallback.display().to_string())
        } else {
            None
        }
    });
    if let Some(path) = ca_path {
        let pem = fs::read(&path).map_err(|e| format!("read CA {path}: {e}"))?;
        let cert =
            reqwest::Certificate::from_pem(&pem).map_err(|e| format!("invalid CA {path}: {e}"))?;
        builder = builder.add_root_certificate(cert);
    }
    let insecure = env::var("IRIUM_RPC_INSECURE")
        .ok()
        .map(|v| {
            let v = v.to_lowercase();
            v == "1" || v == "true" || v == "yes"
        })
        .unwrap_or(false);
    if insecure {
        let url = reqwest::Url::parse(base).map_err(|e| format!("invalid RPC URL {base}: {e}"))?;
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
            builder = builder.danger_accept_invalid_certs(true);
        }
    }
    builder.build().map_err(|e| format!("build client: {e}"))
}

fn p2pkh_script(pkh: &[u8; 20]) -> Vec<u8> {
    let mut script = Vec::with_capacity(25);
    script.push(0x76);
    script.push(0xa9);
    script.push(0x14);
    script.extend_from_slice(pkh);
    script.push(0x88);
    script.push(0xac);
    script
}

fn signature_digest(tx: &Transaction, input_index: usize, script_pubkey: &[u8]) -> [u8; 32] {
    let mut tx_copy = tx.clone();
    for (idx, input) in tx_copy.inputs.iter_mut().enumerate() {
        if idx == input_index {
            input.script_sig = script_pubkey.to_vec();
        } else {
            input.script_sig.clear();
        }
    }
    let mut data = tx_copy.serialize();
    data.extend_from_slice(&1u32.to_le_bytes());
    sha256d(&data)
}

fn hex_to_32(s: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(s).map_err(|_| "invalid hex".to_string())?;
    if bytes.len() != 32 {
        return Err("invalid txid length".to_string());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn fetch_balance(client: &Client, base: &str, addr: &str) -> Result<BalanceResponse, String> {
    let resp = send_with_https_fallback(base, |b| {
        let url = format!("{}/rpc/balance?address={}", b, addr);
        let mut req = client.get(&url);
        if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
            req = req.bearer_auth(token);
        }
        req.send()
    })
    .map_err(|e| format!("balance request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("balance request failed: {}", resp.status()));
    }
    resp.json::<BalanceResponse>()
        .map_err(|e| format!("parse balance response: {e}"))
}

fn fetch_utxos(client: &Client, base: &str, addr: &str) -> Result<UtxosResponse, String> {
    let resp = send_with_https_fallback(base, |b| {
        let url = format!("{}/rpc/utxos?address={}", b, addr);
        let mut req = client.get(&url);
        if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
            req = req.bearer_auth(token);
        }
        req.send()
    })
    .map_err(|e| format!("utxos request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("utxos request failed: {}", resp.status()));
    }
    resp.json::<UtxosResponse>()
        .map_err(|e| format!("parse utxos response: {e}"))
}

fn fetch_history(client: &Client, base: &str, addr: &str) -> Result<HistoryResponse, String> {
    let resp = send_with_https_fallback(base, |b| {
        let url = format!("{}/rpc/history?address={}", b, addr);
        let mut req = client.get(&url);
        if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
            req = req.bearer_auth(token);
        }
        req.send()
    })
    .map_err(|e| format!("history request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("history request failed: {}", resp.status()));
    }
    resp.json::<HistoryResponse>()
        .map_err(|e| format!("parse history response: {e}"))
}

fn fetch_fee_estimate(client: &Client, base: &str) -> Result<FeeEstimateResponse, String> {
    let resp = send_with_https_fallback(base, |b| {
        let url = format!("{}/rpc/fee_estimate", b);
        let mut req = client.get(&url);
        if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
            req = req.bearer_auth(token);
        }
        req.send()
    })
    .map_err(|e| format!("fee estimate failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("fee estimate failed: {}", resp.status()));
    }
    resp.json::<FeeEstimateResponse>()
        .map_err(|e| format!("parse fee estimate response: {e}"))
}

fn submit_tx(client: &Client, base: &str, tx: &Transaction) -> Result<(), String> {
    let raw = tx.serialize();
    let req_body = SubmitTxRequest {
        tx_hex: hex::encode(raw),
    };

    let resp = send_with_https_fallback(base, |b| {
        let url = format!("{}/rpc/submit_tx", b);
        let mut req = client.post(&url).json(&req_body);
        if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
            req = req.bearer_auth(token);
        }
        req.send()
    })
    .map_err(|e| format!("submit tx failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("submit tx failed: {}", resp.status()));
    }
    Ok(())
}

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        usage();
        std::process::exit(1);
    }

    match args[0].as_str() {
        "init" => {
            let path = wallet_path();
            if path.exists() {
                eprintln!("Wallet already exists: {}", path.display());
                std::process::exit(1);
            }
            let seed_hex = if args.len() == 3 {
                if args[1] != "--seed" {
                    usage();
                    std::process::exit(1);
                }
                if let Err(e) = parse_seed_hex(&args[2]) {
                    eprintln!("Invalid seed: {}", e);
                    std::process::exit(1);
                }
                args[2].clone()
            } else if args.len() == 1 {
                generate_seed_hex()
            } else {
                usage();
                std::process::exit(1);
            };
            let secret = match derive_secret_from_seed_hex(&seed_hex, 0) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Failed to derive key from seed: {}", e);
                    std::process::exit(1);
                }
            };
            let key = wallet_key_from_secret(&secret, true);
            let wallet = WalletFile {
                version: 1,
                seed_hex: Some(seed_hex.clone()),
                next_index: 1,
                keys: vec![key.clone()],
            };
            println!("wallet initialized");
            println!("seed saved in wallet metadata; export with: irium-wallet export-seed --out <file>");
            if let Err(e) = save_wallet(&path, &wallet) {
                eprintln!("Failed to save wallet: {}", e);
                std::process::exit(1);
            }
            println!("wallet {}", path.display());
        }
        "new-address" => {
            let path = wallet_path();
            let mut wallet = match ensure_wallet(&path) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to load wallet: {}", e);
                    std::process::exit(1);
                }
            };
            let key = if let Some(seed_hex) = wallet.seed_hex.as_deref() {
                let index = wallet.next_index;
                let secret = match derive_secret_from_seed_hex(seed_hex, index) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("Failed to derive key from seed: {}", e);
                        std::process::exit(1);
                    }
                };
                wallet.next_index = wallet.next_index.saturating_add(1);
                wallet_key_from_secret(&secret, true)
            } else {
                generate_key()
            };
            wallet.keys.push(key);
            println!("new address added; use list-addresses to view");
            if let Err(e) = save_wallet(&path, &wallet) {
                eprintln!("Failed to save wallet: {}", e);
                std::process::exit(1);
            }
            println!("wallet {}", path.display());
        }
        "export-wif" => {
            if args.len() != 4 || args[2] != "--out" {
                usage();
                std::process::exit(1);
            }
            let out = PathBuf::from(&args[3]);
            let path = wallet_path();
            let wallet = match load_wallet(&path) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to load wallet: {}", e);
                    std::process::exit(1);
                }
            };
            let key = match find_key(&wallet, &args[1]) {
                Some(k) => k,
                None => {
                    eprintln!("Address not found in wallet");
                    std::process::exit(1);
                }
            };
            let priv_bytes = match hex::decode(&key.privkey) {
                Ok(v) if v.len() == 32 => v,
                _ => {
                    eprintln!("Wallet key is invalid");
                    std::process::exit(1);
                }
            };
            let mut sec = [0u8; 32];
            sec.copy_from_slice(&priv_bytes);
            let wif = secret_to_wif(&sec, true);
            if let Some(parent) = out.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!("Failed to create output dir: {}", e);
                    std::process::exit(1);
                }
            }
            if let Err(e) = fs::write(&out, format!("{}\n", wif)) {
                eprintln!("Failed to write WIF file: {}", e);
                std::process::exit(1);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&out, fs::Permissions::from_mode(0o600));
            }
            println!("wif exported to {}", out.display());
        }
        "import-wif" => {
            if args.len() != 2 {
                usage();
                std::process::exit(1);
            }
            let path = wallet_path();
            let mut wallet = match ensure_wallet(&path) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to load wallet: {}", e);
                    std::process::exit(1);
                }
            };
            let (priv_bytes, compressed) = match wif_to_secret_and_compression(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Invalid WIF: {}", e);
                    std::process::exit(1);
                }
            };
            let secret = match SecretKey::from_slice(&priv_bytes) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Invalid WIF secret: {}", e);
                    std::process::exit(1);
                }
            };
            let key = wallet_key_from_secret(&secret, compressed);
            if wallet.keys.iter().any(|k| k.address == key.address) {
                println!("key already exists in wallet");
                std::process::exit(0);
            }
            wallet.keys.push(key);
            println!("key imported into wallet");
            if let Err(e) = save_wallet(&path, &wallet) {
                eprintln!("Failed to save wallet: {}", e);
                std::process::exit(1);
            }
            println!("wallet {}", path.display());
        }
        "export-seed" => {
            if args.len() != 3 || args[1] != "--out" {
                usage();
                std::process::exit(1);
            }
            let out = PathBuf::from(&args[2]);
            let path = wallet_path();
            let wallet = match load_wallet(&path) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to load wallet: {}", e);
                    std::process::exit(1);
                }
            };
            let seed = match wallet.seed_hex {
                Some(seed) => seed,
                None => {
                    eprintln!("No seed stored in wallet (legacy/imported key-only wallet)");
                    std::process::exit(1);
                }
            };
            if let Some(parent) = out.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!("Failed to create output dir: {}", e);
                    std::process::exit(1);
                }
            }
            if let Err(e) = fs::write(&out, format!("{}\n", seed)) {
                eprintln!("Failed to write seed file: {}", e);
                std::process::exit(1);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&out, fs::Permissions::from_mode(0o600));
            }
            println!("seed exported to {}", out.display());
        }
        "import-seed" => {
            if args.len() != 2 && args.len() != 3 {
                usage();
                std::process::exit(1);
            }
            let force = args.len() == 3 && args[2] == "--force";
            if args.len() == 3 && !force {
                usage();
                std::process::exit(1);
            }
            if let Err(e) = parse_seed_hex(&args[1]) {
                eprintln!("Invalid seed: {}", e);
                std::process::exit(1);
            }
            let path = wallet_path();
            let mut wallet = match ensure_wallet(&path) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to load wallet: {}", e);
                    std::process::exit(1);
                }
            };
            if !wallet.keys.is_empty() && !force {
                eprintln!(
                    "Wallet already has keys. Re-run with --force to replace wallet keys from seed."
                );
                std::process::exit(1);
            }
            let seed_hex = args[1].clone();
            let secret = match derive_secret_from_seed_hex(&seed_hex, 0) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Failed to derive key from seed: {}", e);
                    std::process::exit(1);
                }
            };
            let key = wallet_key_from_secret(&secret, true);
            wallet.version = 1;
            wallet.seed_hex = Some(seed_hex);
            wallet.next_index = 1;
            wallet.keys = vec![key.clone()];
            if let Err(e) = save_wallet(&path, &wallet) {
                eprintln!("Failed to save wallet: {}", e);
                std::process::exit(1);
            }
            println!("seed imported into wallet");
            println!("wallet {}", path.display());
        }
        "backup" => {
            if args.len() != 1 && args.len() != 3 {
                usage();
                std::process::exit(1);
            }
            let path = wallet_path();
            if !path.exists() {
                eprintln!("Wallet does not exist: {}", path.display());
                std::process::exit(1);
            }
            let out = if args.len() == 3 {
                if args[1] != "--out" {
                    usage();
                    std::process::exit(1);
                }
                PathBuf::from(&args[2])
            } else {
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
                PathBuf::from(home)
                    .join(".irium/wallet-backups")
                    .join(format!("wallet.json.bak.{ts}"))
            };
            if let Some(parent) = out.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!("Failed to create backup dir: {}", e);
                    std::process::exit(1);
                }
            }
            if let Err(e) = fs::copy(&path, &out) {
                eprintln!("Failed to backup wallet: {}", e);
                std::process::exit(1);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&out, fs::Permissions::from_mode(0o600));
            }
            println!("backup {}", out.display());
        }
        "restore-backup" => {
            if args.len() != 2 && args.len() != 3 {
                usage();
                std::process::exit(1);
            }
            let force = args.len() == 3 && args[2] == "--force";
            if args.len() == 3 && !force {
                usage();
                std::process::exit(1);
            }
            let src = PathBuf::from(&args[1]);
            if !src.exists() {
                eprintln!("Backup file not found: {}", src.display());
                std::process::exit(1);
            }
            let dst = wallet_path();
            if dst.exists() && !force {
                eprintln!(
                    "Wallet already exists at {}. Re-run with --force to overwrite.",
                    dst.display()
                );
                std::process::exit(1);
            }
            if let Some(parent) = dst.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!("Failed to create wallet dir: {}", e);
                    std::process::exit(1);
                }
            }
            if let Err(e) = fs::copy(&src, &dst) {
                eprintln!("Failed to restore wallet: {}", e);
                std::process::exit(1);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&dst, fs::Permissions::from_mode(0o600));
            }
            println!("wallet {}", dst.display());
        }
        "list-addresses" => {
            let path = wallet_path();
            let wallet = match load_wallet(&path) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to load wallet: {}", e);
                    std::process::exit(1);
                }
            };
            for key in wallet.keys {
                println!("{}", key.address);
            }
        }
        "address-to-pkh" => {
            if args.len() != 2 {
                usage();
                std::process::exit(1);
            }
            let addr = &args[1];
            match base58_p2pkh_to_hash(addr) {
                Some(pkh) => println!("{}", hex::encode(pkh)),
                None => {
                    eprintln!("Invalid address or checksum");
                    std::process::exit(1);
                }
            }
        }
        "qr" => {
            if args.len() < 2 {
                usage();
                std::process::exit(1);
            }
            let addr = &args[1];
            if base58_p2pkh_to_hash(addr).is_none() {
                eprintln!("Invalid address or checksum");
                std::process::exit(1);
            }
            let mut output_path: Option<String> = None;
            let mut use_svg = false;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--svg" => {
                        use_svg = true;
                        i += 1;
                    }
                    "--out" => {
                        if i + 1 >= args.len() {
                            eprintln!("Missing --out value");
                            std::process::exit(1);
                        }
                        output_path = Some(args[i + 1].clone());
                        i += 2;
                    }
                    _ => {
                        usage();
                        std::process::exit(1);
                    }
                }
            }

            let rendered = if use_svg {
                render_svg(addr, 8, 2).unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                })
            } else {
                render_ascii(addr).unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                })
            };

            if let Some(path) = output_path {
                if let Err(e) = fs::write(&path, rendered) {
                    eprintln!("Failed to write {}: {}", path, e);
                    std::process::exit(1);
                }
            } else {
                print!("{}", rendered);
            }
        }
        "balance" => {
            if args.len() != 2 && args.len() != 4 {
                usage();
                std::process::exit(1);
            }
            let addr = &args[1];
            if base58_p2pkh_to_hash(addr).is_none() {
                eprintln!("Invalid address or checksum");
                std::process::exit(1);
            }
            let mut rpc_url = default_rpc_url();
            if args.len() == 4 {
                if args[2] != "--rpc" {
                    usage();
                    std::process::exit(1);
                }
                rpc_url = args[3].clone();
            }
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let payload = match fetch_balance(&client, base, addr) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let use_color = color_enabled();
            let irm_display = format_irm(payload.balance);
            let balance_display = if use_color {
                format!("\x1b[32m{} IRM\x1b[0m", irm_display)
            } else {
                format!("{} IRM", irm_display)
            };
            let mined_blocks = payload.mined_blocks.unwrap_or(payload.utxo_count);
            println!("balance {} blocks mined {}", balance_display, mined_blocks);
        }
        "list-unspent" => {
            if args.len() != 2 && args.len() != 4 {
                usage();
                std::process::exit(1);
            }
            let addr = &args[1];
            if base58_p2pkh_to_hash(addr).is_none() {
                eprintln!("Invalid address or checksum");
                std::process::exit(1);
            }
            let mut rpc_url = default_rpc_url();
            if args.len() == 4 {
                if args[2] != "--rpc" {
                    usage();
                    std::process::exit(1);
                }
                rpc_url = args[3].clone();
            }
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let payload = match fetch_utxos(&client, base, addr) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            for utxo in payload.utxos {
                let confirmations = payload.height.saturating_sub(utxo.height);
                if utxo.is_coinbase && confirmations < COINBASE_MATURITY {
                    continue;
                }
                let val = format_irm(utxo.value);
                println!(
                    "{}:{} {} IRM height {} coinbase {}",
                    utxo.txid, utxo.index, val, utxo.height, utxo.is_coinbase
                );
            }
        }

        "history" => {
            if args.len() != 2 && args.len() != 4 {
                usage();
                std::process::exit(1);
            }
            let addr = &args[1];
            if base58_p2pkh_to_hash(addr).is_none() {
                eprintln!("Invalid address or checksum");
                std::process::exit(1);
            }
            let mut rpc_url = default_rpc_url();
            if args.len() == 4 {
                if args[2] != "--rpc" {
                    usage();
                    std::process::exit(1);
                }
                rpc_url = args[3].clone();
            }
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let payload = match fetch_history(&client, base, addr) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let _height = payload.height;
            for item in payload.txs {
                let received = format_irm(item.received);
                let spent = format_irm(item.spent);
                let net = if item.net >= 0 {
                    format!("+{}", format_irm(item.net as u64))
                } else {
                    format!("-{}", format_irm((-item.net) as u64))
                };
                println!(
                    "{} height {} net {} recv {} spent {} coinbase {}",
                    item.txid, item.height, net, received, spent, item.is_coinbase
                );
            }
        }
        "estimate-fee" => {
            let mut rpc_url = default_rpc_url();
            if args.len() == 3 {
                if args[1] != "--rpc" {
                    usage();
                    std::process::exit(1);
                }
                rpc_url = args[2].clone();
            } else if args.len() != 1 {
                usage();
                std::process::exit(1);
            }
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let payload = match fetch_fee_estimate(&client, base) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            println!(
                "min_fee_per_byte {} mempool_size {}",
                payload.min_fee_per_byte, payload.mempool_size
            );
        }
        "send" => {
            if args.len() < 4 {
                usage();
                std::process::exit(1);
            }
            let from_addr = &args[1];
            let to_addr = &args[2];
            let amount = match parse_irm(&args[3]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Invalid amount: {}", e);
                    std::process::exit(1);
                }
            };
            if base58_p2pkh_to_hash(from_addr).is_none() || base58_p2pkh_to_hash(to_addr).is_none()
            {
                eprintln!("Invalid address or checksum");
                std::process::exit(1);
            }
            let mut fee_override: Option<u64> = None;
            let mut rpc_url = default_rpc_url();
            let mut coin_select = String::from("smallest");
            let mut i = 4;
            while i < args.len() {
                match args[i].as_str() {
                    "--fee" => {
                        if i + 1 >= args.len() {
                            eprintln!("Missing --fee value");
                            std::process::exit(1);
                        }
                        fee_override = Some(match parse_irm(&args[i + 1]) {
                            Ok(v) => v,
                            Err(e) => {
                                eprintln!("Invalid fee: {}", e);
                                std::process::exit(1);
                            }
                        });
                        i += 2;
                    }
                    "--coin-select" => {
                        if i + 1 >= args.len() {
                            eprintln!("Missing --coin-select value");
                            std::process::exit(1);
                        }
                        let mode = &args[i + 1];
                        if mode != "smallest" && mode != "largest" {
                            eprintln!("Invalid --coin-select value: {}", mode);
                            std::process::exit(1);
                        }
                        coin_select = mode.clone();
                        i += 2;
                    }
                    "--rpc" => {
                        if i + 1 >= args.len() {
                            eprintln!("Missing --rpc value");
                            std::process::exit(1);
                        }
                        rpc_url = args[i + 1].clone();
                        i += 2;
                    }
                    _ => {
                        usage();
                        std::process::exit(1);
                    }
                }
            }

            let path = wallet_path();
            let wallet = match load_wallet(&path) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to load wallet: {}", e);
                    std::process::exit(1);
                }
            };
            let key = match find_key(&wallet, from_addr) {
                Some(k) => k.clone(),
                None => {
                    eprintln!("From address not found in wallet");
                    std::process::exit(1);
                }
            };

            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let payload = match fetch_utxos(&client, base, from_addr) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut utxos = payload.utxos.clone();
            match coin_select.as_str() {
                "smallest" => utxos.sort_by_key(|u| u.value),
                "largest" => utxos.sort_by_key(|u| Reverse(u.value)),
                _ => {}
            }

            let mut fee_per_byte = DEFAULT_FEE_PER_BYTE;
            if fee_override.is_none() {
                if let Ok(est) = fetch_fee_estimate(&client, base) {
                    let est_fee = est.min_fee_per_byte.ceil() as u64;
                    if est_fee > fee_per_byte {
                        fee_per_byte = est_fee;
                    }
                }
            }

            let mut selected = Vec::new();
            let mut total = 0u64;
            let mut fee = fee_override.unwrap_or(0);
            for utxo in utxos.iter() {
                let confirmations = payload.height.saturating_sub(utxo.height);
                if utxo.is_coinbase && confirmations < COINBASE_MATURITY {
                    continue;
                }
                selected.push(utxo.clone());
                total = total.saturating_add(utxo.value);
                if fee_override.is_none() {
                    let outputs = if total > amount { 2 } else { 1 };
                    fee = estimate_tx_size(selected.len(), outputs).saturating_mul(fee_per_byte);
                }
                if total >= amount.saturating_add(fee) {
                    break;
                }
            }

            if total < amount.saturating_add(fee) {
                eprintln!("Insufficient funds");
                std::process::exit(1);
            }

            let to_pkh = match base58_p2pkh_to_hash(to_addr) {
                Some(v) => v,
                None => {
                    eprintln!("Invalid destination address");
                    std::process::exit(1);
                }
            };
            let mut to_arr = [0u8; 20];
            to_arr.copy_from_slice(&to_pkh);
            let to_script = p2pkh_script(&to_arr);

            let from_pkh = match base58_p2pkh_to_hash(from_addr) {
                Some(v) => v,
                None => {
                    eprintln!("Invalid source address");
                    std::process::exit(1);
                }
            };
            let mut from_arr = [0u8; 20];
            from_arr.copy_from_slice(&from_pkh);
            let change_script = p2pkh_script(&from_arr);

            let priv_bytes = match hex::decode(&key.privkey) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Invalid private key hex: {}", e);
                    std::process::exit(1);
                }
            };
            let signing_key = match SigningKey::from_bytes(priv_bytes.as_slice().into()) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Invalid signing key: {}", e);
                    std::process::exit(1);
                }
            };
            // Derive pubkey from private key at send-time so stale wallet metadata
            // cannot produce invalid signatures.
            let from_pkh_arr = {
                let mut arr = [0u8; 20];
                arr.copy_from_slice(&from_pkh);
                arr
            };
            let vk = signing_key.verifying_key();
            let pk_comp = vk.to_encoded_point(true);
            let pk_uncomp = vk.to_encoded_point(false);
            let pub_bytes = if hash160(pk_comp.as_bytes()) == from_pkh_arr {
                pk_comp.as_bytes().to_vec()
            } else if hash160(pk_uncomp.as_bytes()) == from_pkh_arr {
                pk_uncomp.as_bytes().to_vec()
            } else {
                eprintln!("Wallet key mismatch: source address does not match derived private key");
                std::process::exit(1);
            };

            let mut inputs: Vec<TxInput> = Vec::new();
            for utxo in &selected {
                let txid = match hex_to_32(&utxo.txid) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("Invalid utxo txid: {}", e);
                        std::process::exit(1);
                    }
                };
                inputs.push(TxInput {
                    prev_txid: txid,
                    prev_index: utxo.index,
                    script_sig: Vec::new(),
                    sequence: 0xffff_ffff,
                });
            }

            let mut outputs = vec![TxOutput {
                value: amount,
                script_pubkey: to_script,
            }];

            let mut change = total.saturating_sub(amount).saturating_sub(fee);
            if change > 0 {
                outputs.push(TxOutput {
                    value: change,
                    script_pubkey: change_script.clone(),
                });
            }

            let mut tx = Transaction {
                version: 1,
                inputs,
                outputs,
                locktime: 0,
            };

            for _ in 0..2 {
                for (idx, utxo) in selected.iter().enumerate() {
                    let script_pubkey = match hex::decode(&utxo.script_pubkey) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("Invalid utxo script_pubkey hex: {}", e);
                            std::process::exit(1);
                        }
                    };
                    let digest = signature_digest(&tx, idx, &script_pubkey);
                    let sig: Signature = match signing_key.sign_prehash(&digest) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("Failed to sign prehash: {}", e);
                            std::process::exit(1);
                        }
                    };
                    let sig = sig.normalize_s().unwrap_or(sig);
                    let mut sig_bytes = sig.to_der().as_bytes().to_vec();
                    sig_bytes.push(0x01);

                    let mut script = Vec::new();
                    script.push(sig_bytes.len() as u8);
                    script.extend_from_slice(&sig_bytes);
                    script.push(pub_bytes.len() as u8);
                    script.extend_from_slice(&pub_bytes);
                    tx.inputs[idx].script_sig = script;
                }

                let size = tx.serialize().len() as u64;
                if fee_override.is_some() {
                    break;
                }
                let needed_fee = size.saturating_mul(fee_per_byte);
                if needed_fee > fee {
                    let extra = needed_fee - fee;
                    if change >= extra {
                        fee = needed_fee;
                        change = change.saturating_sub(extra);
                        if tx.outputs.len() > 1 {
                            tx.outputs[1].value = change;
                        } else if change > 0 {
                            tx.outputs.push(TxOutput {
                                value: change,
                                script_pubkey: change_script.clone(),
                            });
                        }
                        continue;
                    } else {
                        eprintln!("Insufficient funds for fee");
                        std::process::exit(1);
                    }
                }
                break;
            }

            if let Err(e) = submit_tx(&client, base, &tx) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            println!("txid {}", hex::encode(tx.txid()));
        }
        _ => {
            usage();
            std::process::exit(1);
        }
    }
}
