use reqwest::blocking::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::env;
use std::time::Duration;

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

fn usage() {
    eprintln!("Usage:");
    eprintln!("  irium-wallet address-to-pkh <base58_addr>");
    eprintln!("  irium-wallet balance <base58_addr> [--rpc <url>]");
}

fn default_rpc_url() -> String {
    env::var("IRIUM_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:38300".to_string())
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

#[derive(Deserialize)]
struct BalanceResponse {
    balance: u64,
    utxo_count: usize,
}

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        usage();
        std::process::exit(1);
    }

    match args[0].as_str() {
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
            let client = match Client::builder().timeout(Duration::from_secs(10)).build() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let resp = match client.get(&url).send() {
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
            println!(
                "balance {} blocks mined {}",
                balance_display,
                payload.utxo_count
            );
        }
        _ => {
            usage();
            std::process::exit(1);
        }
    }
}
