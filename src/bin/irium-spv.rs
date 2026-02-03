use std::env;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use irium_node_rs::block::BlockHeader;
use irium_node_rs::pow::Target;
use irium_node_rs::spv::{
    header_level, nipopow_best_level, nipopow_compare_counts, nipopow_counts,
    verify_header_chain, verify_merkle_proof, HeaderChain,
};

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

fn parse_bits(bits: &str) -> Result<u32, String> {
    u32::from_str_radix(bits.trim_start_matches("0x"), 16)
        .map_err(|e| format!("invalid bits field: {}", e))
}

fn load_headers_from_dir(dir: &PathBuf) -> Result<Vec<BlockHeader>, String> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("failed to read {}: {}", dir.display(), e))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if let Some(num) = name.strip_prefix("block_").and_then(|n| n.strip_suffix(".json")) {
            if let Ok(h) = num.parse::<u64>() {
                entries.push((h, path));
            }
        }
    }
    entries.sort_by_key(|(h, _)| *h);
    let mut headers = Vec::new();
    for (_h, path) in entries {
        let data = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
        let v: Value = serde_json::from_str(&data)
            .map_err(|e| format!("failed to parse JSON: {}", e))?;
        let jb: JsonBlock = serde_json::from_value(v)
            .map_err(|e| format!("unexpected block JSON format: {}", e))?;
        let merkle_root = parse_hex32(&jb.header.merkle_root)?;
        let prev_hash = parse_hex32(&jb.header.prev_hash)?;
        let bits = parse_bits(&jb.header.bits)?;
        let header = BlockHeader {
            version: jb.header.version,
            prev_hash,
            merkle_root,
            time: jb.header.time,
            bits,
            nonce: jb.header.nonce,
        };
        headers.push(header);
    }
    Ok(headers)
}

fn nipopow_levels(headers: &[BlockHeader]) -> Vec<u32> {
    headers.iter().map(header_level).collect()
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
    if args.len() < 2 {
        println!("Irium SPV tool (Rust)");
        println!("Usage:");
        println!("  irium-spv verify <height> <txid> <index> <proof_hex_comma_separated>");
        println!("  irium-spv nipopow-score [blocks_dir] [m]");
        println!("  irium-spv nipopow-compare <blocks_dir_a> <blocks_dir_b> [m]");
        println!();
        println!("Notes:");
        println!("  blocks_dir defaults to ~/.irium/blocks, m defaults to 15");
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
            let header_bits = parse_bits(&jb.header.bits)?;
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
        "nipopow-score" => {
            let blocks_dir = if args.len() >= 3 {
                PathBuf::from(&args[2])
            } else {
                default_blocks_dir()
            };
            let m: usize = if args.len() >= 4 {
                args[3].parse().map_err(|e| format!("invalid m: {}", e))?
            } else {
                15
            };
            let headers = load_headers_from_dir(&blocks_dir)?;
            verify_header_chain(&headers)?;
            let levels = nipopow_levels(&headers);
            let counts = nipopow_counts(&levels);
            let best = nipopow_best_level(&counts, m);
            println!("NiPoPoW score for {} headers (m={}):", headers.len(), m);
            println!("  best level: {}", best);
            for (mu, cnt) in counts.iter().enumerate().rev() {
                if *cnt > 0 {
                    println!("  mu={} -> |C_mu|={}", mu, cnt);
                }
            }
            Ok(())
        }
        "nipopow-compare" => {
            if args.len() < 4 {
                return Err("nipopow-compare requires <blocks_dir_a> <blocks_dir_b> [m]".to_string());
            }
            let dir_a = PathBuf::from(&args[2]);
            let dir_b = PathBuf::from(&args[3]);
            let m: usize = if args.len() >= 5 {
                args[4].parse().map_err(|e| format!("invalid m: {}", e))?
            } else {
                15
            };
            let headers_a = load_headers_from_dir(&dir_a)?;
            let headers_b = load_headers_from_dir(&dir_b)?;
            verify_header_chain(&headers_a)?;
            verify_header_chain(&headers_b)?;
            let counts_a = nipopow_counts(&nipopow_levels(&headers_a));
            let counts_b = nipopow_counts(&nipopow_levels(&headers_b));
            use std::cmp::Ordering;
            let winner = match nipopow_compare_counts(&counts_a, &counts_b, m) {
                Ordering::Greater => "A",
                Ordering::Less => "B",
                Ordering::Equal => "equal",
            };
            println!("NiPoPoW compare (m={}): winner = {}", m, winner);
            Ok(())
        }
        _ => Err(format!("unknown command: {}", cmd)),
    }
}
