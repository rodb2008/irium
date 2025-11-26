use std::env;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use irium_node_rs::block::BlockHeader;
use irium_node_rs::pow::Target;
use irium_node_rs::spv::{verify_merkle_proof, HeaderChain};

#[derive(Deserialize)]
struct JsonHeader {
    version: u32,
    prev_hash: String,
    merkle_root: String,
    time: u32,
    bits: String,
    nonce: u32,
}

#[derive(Deserialize)]
struct JsonBlock {
    header: JsonHeader,
}

fn default_blocks_dir() -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
    PathBuf::from(home).join(".irium/blocks")
}

fn parse_hex32(s: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(s).map_err(|e| e.to_string())?;
    if bytes.len() != 32 {
        return Err("expected 32-byte hex value".to_string());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        println!("Irium SPV tool (Rust)");
        println!("Usage:");
        println!("  irium-spv verify <height> <txid> <index> <proof_hex_comma_separated>");
        println!();
        println!("Example:");
        println!("  irium-spv verify 1 <txid> 0 <sibling1,sibling2,...>");
        return Ok(());
    }

    let cmd = args[1].as_str();
    match cmd {
        "verify" => {
            if args.len() < 6 {
                return Err(
                    "verify requires: <height> <txid> <index> <proof_hex_comma_separated>"
                        .to_string(),
                );
            }
            let height: u64 = args[2]
                .parse()
                .map_err(|e| format!("invalid height: {}", e))?;
            let txid_hex = &args[3];
            let index: usize = args[4]
                .parse()
                .map_err(|e| format!("invalid index: {}", e))?;
            let proof_arg = &args[5];

            let blocks_dir = default_blocks_dir();
            let path = blocks_dir.join(format!("block_{}.json", height));
            let data = fs::read_to_string(&path)
                .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;

            let v: Value =
                serde_json::from_str(&data).map_err(|e| format!("failed to parse JSON: {}", e))?;
            let jb: JsonBlock = serde_json::from_value(v)
                .map_err(|e| format!("unexpected block JSON format: {}", e))?;

            let merkle_root = parse_hex32(&jb.header.merkle_root)?;
            let txid = parse_hex32(txid_hex)?;

            let proof_hashes: Result<Vec<[u8; 32]>, String> = proof_arg
                .split(',')
                .filter(|s| !s.is_empty())
                .map(parse_hex32)
                .collect();
            let proof = proof_hashes?;

            let valid = verify_merkle_proof(&txid, &merkle_root, proof, index);
            if valid {
                println!("SPV proof valid for txid {} at height {}", txid_hex, height);
            } else {
                println!(
                    "SPV proof INVALID for txid {} at height {}",
                    txid_hex, height
                );
            }

            // Demonstrate header-chain validation for this single header.
            let header_bits = u32::from_str_radix(jb.header.bits.trim_start_matches("0x"), 16)
                .map_err(|e| format!("invalid bits field: {}", e))?;
            let prev_hash = parse_hex32(&jb.header.prev_hash)?;
            // BlockHeader stores hashes in internal order; caller is responsible
            // for matching the same convention used by the main chain code.
            let header = BlockHeader {
                version: jb.header.version,
                prev_hash,
                merkle_root: merkle_root,
                time: jb.header.time,
                bits: header_bits,
                nonce: jb.header.nonce,
            };

            let target = Target { bits: header_bits };
            let mut chain = HeaderChain::new(header.clone());

            // For a single-header chain this always succeeds if PoW is valid.
            if let Err(e) = chain.append(header, target) {
                println!("Header-chain validation failed: {}", e);
            }

            Ok(())
        }
        _ => Err(format!("unknown command: {}", cmd)),
    }
}
