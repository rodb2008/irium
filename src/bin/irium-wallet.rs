use irium_node_rs::constants::COINBASE_MATURITY;
use irium_node_rs::pow::sha256d;
use irium_node_rs::qr::{render_ascii, render_svg};
use irium_node_rs::tx::{Transaction, TxInput, TxOutput};
use k256::ecdsa::signature::hazmat::PrehashSigner;
use k256::ecdsa::{Signature, SigningKey};
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::SecretKey;
use rand_core::OsRng;
use reqwest::blocking::Client;
use ripemd::Ripemd160;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Reverse;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const IRIUM_P2PKH_VERSION: u8 = 0x39;
const DEFAULT_FEE_PER_BYTE: u64 = 1;

#[derive(Serialize, Deserialize)]
struct WalletFile {
    version: u32,
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
    serde_json::from_str(&data).map_err(|e| format!("parse wallet: {e}"))
}

fn save_wallet(path: &Path, wallet: &WalletFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create wallet dir: {e}"))?;
    }
    let data = serde_json::to_string_pretty(wallet).map_err(|e| format!("serialize wallet: {e}"))?;
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
    if path.exists() {
        load_wallet(path)
    } else {
        Ok(WalletFile {
            version: 1,
            keys: Vec::new(),
        })
    }
}

fn generate_key() -> WalletKey {
    let secret = SecretKey::random(&mut OsRng);
    let public = secret.public_key();
    let pubkey = public.to_encoded_point(true);
    let pkh = hash160(pubkey.as_bytes());
    let address = base58_p2pkh_from_hash(&pkh);
    WalletKey {
        address,
        pkh: hex::encode(pkh),
        pubkey: hex::encode(pubkey.as_bytes()),
        privkey: hex::encode(secret.to_bytes()),
    }
}

fn find_key<'a>(wallet: &'a WalletFile, addr: &str) -> Option<&'a WalletKey> {
    wallet.keys.iter().find(|k| k.address == addr)
}

fn usage() {
    eprintln!("Usage:");
    eprintln!("  irium-wallet init");
    eprintln!("  irium-wallet new-address");
    eprintln!("  irium-wallet list-addresses");
    eprintln!("  irium-wallet address-to-pkh <base58_addr>");
    eprintln!("  irium-wallet qr <base58_addr> [--svg] [--out <file>]");
    eprintln!("  irium-wallet balance <base58_addr> [--rpc <url>]");
    eprintln!("  irium-wallet list-unspent <base58_addr> [--rpc <url>]");
    eprintln!("  irium-wallet history <base58_addr> [--rpc <url>]");
    eprintln!("  irium-wallet estimate-fee [--rpc <url>]");
    eprintln!("  irium-wallet send <from_addr> <to_addr> <amount_irm> [--fee <irm>] [--coin-select smallest|largest] [--rpc <url>]");
}

fn default_rpc_url() -> String {
    env::var("IRIUM_NODE_RPC")
        .or_else(|_| env::var("IRIUM_RPC_URL"))
        .unwrap_or_else(|_| "http://127.0.0.1:38300".to_string())
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
    Ok(whole
        .saturating_mul(100_000_000)
        .saturating_add(frac))
}

fn estimate_tx_size(inputs: usize, outputs: usize) -> u64 {
    10 + inputs as u64 * 148 + outputs as u64 * 34
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
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
        let cert = reqwest::Certificate::from_pem(&pem)
            .map_err(|e| format!("invalid CA {path}: {e}"))?;
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
        let url = reqwest::Url::parse(base)
            .map_err(|e| format!("invalid RPC URL {base}: {e}"))?;
        if url.scheme() != "https" {
            eprintln!("[warn] IRIUM_RPC_INSECURE=1 has no effect on non-HTTPS RPC URL");
        } else {
            let host = url.host_str().ok_or_else(|| "RPC URL missing host".to_string())?;
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
    let url = format!("{}/rpc/balance?address={}", base, addr);
    let mut req = client.get(&url);
    if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
        req = req.bearer_auth(token);
    }
    let resp = req.send().map_err(|e| format!("balance request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("balance request failed: {}", resp.status()));
    }
    resp.json::<BalanceResponse>()
        .map_err(|e| format!("parse balance response: {e}"))
}

fn fetch_utxos(client: &Client, base: &str, addr: &str) -> Result<UtxosResponse, String> {
    let url = format!("{}/rpc/utxos?address={}", base, addr);
    let mut req = client.get(&url);
    if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
        req = req.bearer_auth(token);
    }
    let resp = req.send().map_err(|e| format!("utxos request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("utxos request failed: {}", resp.status()));
    }
    resp.json::<UtxosResponse>()
        .map_err(|e| format!("parse utxos response: {e}"))
}


fn fetch_history(client: &Client, base: &str, addr: &str) -> Result<HistoryResponse, String> {
    let url = format!("{}/rpc/history?address={}", base, addr);
    let mut req = client.get(&url);
    if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
        req = req.bearer_auth(token);
    }
    let resp = req.send().map_err(|e| format!("history request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("history request failed: {}", resp.status()));
    }
    resp.json::<HistoryResponse>()
        .map_err(|e| format!("parse history response: {e}"))
}

fn fetch_fee_estimate(client: &Client, base: &str) -> Result<FeeEstimateResponse, String> {
    let url = format!("{}/rpc/fee_estimate", base);
    let mut req = client.get(&url);
    if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
        req = req.bearer_auth(token);
    }
    let resp = req.send().map_err(|e| format!("fee estimate failed: {e}"))?;
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
    let url = format!("{}/rpc/submit_tx", base);
    let mut req = client.post(&url).json(&req_body);
    if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
        req = req.bearer_auth(token);
    }
    let resp = req.send().map_err(|e| format!("submit tx failed: {e}"))?;
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
            let mut wallet = WalletFile {
                version: 1,
                keys: Vec::new(),
            };
            let key = generate_key();
            println!("address {}", key.address);
            wallet.keys.push(key);
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
            let key = generate_key();
            println!("address {}", key.address);
            wallet.keys.push(key);
            if let Err(e) = save_wallet(&path, &wallet) {
                eprintln!("Failed to save wallet: {}", e);
                std::process::exit(1);
            }
            println!("wallet {}", path.display());
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
                    utxo.txid,
                    utxo.index,
                    val,
                    utxo.height,
                    utxo.is_coinbase
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
                    item.txid,
                    item.height,
                    net,
                    received,
                    spent,
                    item.is_coinbase
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
                payload.min_fee_per_byte,
                payload.mempool_size
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
            if base58_p2pkh_to_hash(from_addr).is_none() || base58_p2pkh_to_hash(to_addr).is_none() {
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
            let pub_bytes = match hex::decode(&key.pubkey) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Invalid public key hex: {}", e);
                    std::process::exit(1);
                }
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
