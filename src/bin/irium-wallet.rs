use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::SecretKey;
use rand_core::OsRng;
use reqwest::blocking::Client;
use ripemd::Ripemd160;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::time::Duration;

const IRIUM_P2PKH_VERSION: u8 = 0x39;

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

fn usage() {
    eprintln!("Usage:");
    eprintln!("  irium-wallet new-address");
    eprintln!("  irium-wallet address-to-pkh <base58_addr>");
    eprintln!("  irium-wallet balance <base58_addr> [--rpc <url>]");
}

fn default_rpc_url() -> String {
    env::var("IRIUM_RPC_URL").unwrap_or_else(|_| "https://127.0.0.1:38300".to_string())
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

fn rpc_client() -> Result<Client, String> {
    let mut builder = Client::builder().timeout(Duration::from_secs(10));
    if let Ok(path) = env::var("IRIUM_RPC_CA") {
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
        builder = builder.danger_accept_invalid_certs(true);
    }
    builder.build().map_err(|e| format!("build client: {e}"))
}

#[derive(Deserialize)]
struct BalanceResponse {
    balance: u64,
    utxo_count: usize,
    mined_blocks: Option<usize>,
}

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        usage();
        std::process::exit(1);
    }

    match args[0].as_str() {
        "new-address" => {
            if args.len() != 1 {
                usage();
                std::process::exit(1);
            }
            let secret = SecretKey::random(&mut OsRng);
            let public = secret.public_key();
            let pubkey = public.to_encoded_point(true);
            let pkh = hash160(pubkey.as_bytes());
            let address = base58_p2pkh_from_hash(&pkh);
            println!("address {}", address);
            println!("pubkey {}", hex::encode(pubkey.as_bytes()));
            println!("privkey {}", hex::encode(secret.to_bytes()));
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
            let url = format!("{}/rpc/balance?address={}", base, addr);
            let client = match rpc_client() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let mut req = client.get(&url);
            if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
                req = req.bearer_auth(token);
            }
            let resp = match req.send() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Balance request failed: {}", e);
                    std::process::exit(1);
                }
            };
            if !resp.status().is_success() {
                eprintln!("Balance request failed: {}", resp.status());
                std::process::exit(1);
            }
            let payload: BalanceResponse = match resp.json() {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Failed to parse balance response: {}", e);
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
            println!(
                "balance {} blocks mined {}",
                balance_display,
                mined_blocks
            );
        }
        _ => {
            usage();
            std::process::exit(1);
        }
    }
}
