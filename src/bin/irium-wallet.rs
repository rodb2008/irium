use std::env;
use sha2::{Digest, Sha256};

// Base58 P2PKH decoder (version byte + 20-byte hash + 4-byte checksum)
fn base58_p2pkh_to_hash(addr: &str) -> Option<Vec<u8>> {
    let data = bs58::decode(addr).into_vec().ok()?;
    if data.len() < 25 { return None; }
    let (body, checksum) = data.split_at(data.len() - 4);
    // double SHA256
    let first = Sha256::digest(body);
    let second = Sha256::digest(&first);
    if &second[0..4] != checksum { return None; }
    if body.len() < 21 { return None; }
    let payload = &body[1..];
    if payload.len() != 20 { return None; }
    Some(payload.to_vec())
}

fn usage() {
    eprintln!("Usage: irium-wallet address-to-pkh <base58_addr>");
}

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 || args[0] != "address-to-pkh" {
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
