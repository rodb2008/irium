use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use irium_node_rs::block::BlockHeader;
use irium_node_rs::pow::Target;
use irium_node_rs::spv::{
    header_level, nipopow_best_level, nipopow_compare_counts, nipopow_compare_proofs,
    nipopow_counts, nipopow_proof_counts, nipopow_prove, nipopow_verify, verify_header_chain,
    verify_merkle_proof, HeaderChain, NipopowProof,
};

const DEFAULT_M: usize = 15;
const DEFAULT_K: usize = 30;

#[derive(Deserialize, Serialize)]
struct JsonHeader {
    version: u32,
    prev_hash: String,
    merkle_root: String,
    time: u32,
    bits: String,
    nonce: u32,
}

#[derive(Deserialize, Serialize)]
struct JsonBlock {
    header: JsonHeader,
}

#[derive(Deserialize, Serialize)]
struct GenesisFile {
    header: JsonHeader,
}

#[derive(Deserialize, Serialize)]
struct JsonNipopowProof {
    m: usize,
    k: usize,
    pi: Vec<JsonHeader>,
    chi: Vec<JsonHeader>,
}

fn default_blocks_dir() -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
    PathBuf::from(home).join(".irium/blocks")
}

fn parse_bits(bits: &str) -> Result<u32, String> {
    u32::from_str_radix(bits.trim_start_matches("0x"), 16)
        .map_err(|e| format!("invalid bits field: {}", e))
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

fn header_to_json(header: &BlockHeader) -> JsonHeader {
    JsonHeader {
        version: header.version,
        prev_hash: hex::encode(header.prev_hash),
        merkle_root: hex::encode(header.merkle_root),
        time: header.time,
        bits: format!("0x{:08x}", header.bits),
        nonce: header.nonce,
    }
}

fn json_to_header(header: &JsonHeader) -> Result<BlockHeader, String> {
    let merkle_root = parse_hex32(&header.merkle_root)?;
    let prev_hash = parse_hex32(&header.prev_hash)?;
    let bits = parse_bits(&header.bits)?;
    Ok(BlockHeader {
        version: header.version,
        prev_hash,
        merkle_root,
        time: header.time,
        bits,
        nonce: header.nonce,
    })
}

impl JsonNipopowProof {
    fn from_proof(proof: &NipopowProof) -> Self {
        JsonNipopowProof {
            m: proof.m,
            k: proof.k,
            pi: proof.pi.iter().map(header_to_json).collect(),
            chi: proof.chi.iter().map(header_to_json).collect(),
        }
    }

    fn to_proof(&self) -> Result<NipopowProof, String> {
        let mut pi = Vec::new();
        for header in &self.pi {
            pi.push(json_to_header(header)?);
        }
        let mut chi = Vec::new();
        for header in &self.chi {
            chi.push(json_to_header(header)?);
        }
        Ok(NipopowProof {
            m: self.m,
            k: self.k,
            pi,
            chi,
        })
    }
}

fn load_genesis_header() -> Result<BlockHeader, String> {
    let candidates = ["configs/genesis-locked.json", "configs/genesis.json"];
    for candidate in candidates.iter() {
        let path = PathBuf::from(candidate);
        if !path.exists() {
            continue;
        }
        let data = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
        let v: Value =
            serde_json::from_str(&data).map_err(|e| format!("failed to parse JSON: {}", e))?;
        let gf: GenesisFile = serde_json::from_value(v)
            .map_err(|e| format!("unexpected genesis JSON format: {}", e))?;
        return json_to_header(&gf.header);
    }
    Err("missing genesis file at configs/genesis-locked.json".to_string())
}

fn load_headers_from_dir(dir: &PathBuf) -> Result<Vec<BlockHeader>, String> {
    let mut entries = Vec::new();
    let mut has_genesis = false;
    for entry in
        fs::read_dir(dir).map_err(|e| format!("failed to read {}: {}", dir.display(), e))?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if let Some(num) = name
            .strip_prefix("block_")
            .and_then(|n| n.strip_suffix(".json"))
        {
            if let Ok(h) = num.parse::<u64>() {
                if h == 0 {
                    has_genesis = true;
                }
                entries.push((h, path));
            }
        }
    }
    entries.sort_by_key(|(h, _)| *h);
    let mut headers = Vec::new();
    if !has_genesis {
        let genesis = load_genesis_header()?;
        headers.push(genesis);
    }
    for (_h, path) in entries {
        let data = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
        let v: Value =
            serde_json::from_str(&data).map_err(|e| format!("failed to parse JSON: {}", e))?;
        let jb: JsonBlock = serde_json::from_value(v)
            .map_err(|e| format!("unexpected block JSON format: {}", e))?;
        headers.push(json_to_header(&jb.header)?);
    }
    Ok(headers)
}

fn load_proof(path: &PathBuf) -> Result<NipopowProof, String> {
    let data = fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
    let json: JsonNipopowProof =
        serde_json::from_str(&data).map_err(|e| format!("failed to parse proof JSON: {}", e))?;
    json.to_proof()
}

fn nipopow_levels(headers: &[BlockHeader]) -> Vec<u32> {
    headers.iter().map(header_level).collect()
}

fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Irium SPV tool (Rust)");
        println!("Usage:");
        println!("  irium-spv verify <height> <txid> <index> <proof_hex_comma_separated>");
        println!("  irium-spv nipopow-score [blocks_dir] [m]");
        println!("  irium-spv nipopow-compare <blocks_dir_a> <blocks_dir_b> [m]");
        println!("  irium-spv nipopow-prove [blocks_dir] [m] [k] [out_json]");
        println!("  irium-spv nipopow-verify <proof_json>");
        println!("  irium-spv nipopow-compare-proofs <proof_a> <proof_b> [m]");
        println!();
        println!("Notes:");
        println!("  blocks_dir defaults to ~/.irium/blocks, m defaults to 15, k defaults to 30");
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
                DEFAULT_M
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
                return Err(
                    "nipopow-compare requires <blocks_dir_a> <blocks_dir_b> [m]".to_string()
                );
            }
            let dir_a = PathBuf::from(&args[2]);
            let dir_b = PathBuf::from(&args[3]);
            let m: usize = if args.len() >= 5 {
                args[4].parse().map_err(|e| format!("invalid m: {}", e))?
            } else {
                DEFAULT_M
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
        "nipopow-prove" => {
            let blocks_dir = if args.len() >= 3 {
                PathBuf::from(&args[2])
            } else {
                default_blocks_dir()
            };
            let m: usize = if args.len() >= 4 {
                args[3].parse().map_err(|e| format!("invalid m: {}", e))?
            } else {
                DEFAULT_M
            };
            let k: usize = if args.len() >= 5 {
                args[4].parse().map_err(|e| format!("invalid k: {}", e))?
            } else {
                DEFAULT_K
            };
            let out_path = if args.len() >= 6 {
                Some(PathBuf::from(&args[5]))
            } else {
                None
            };

            let headers = load_headers_from_dir(&blocks_dir)?;
            verify_header_chain(&headers)?;
            let proof = nipopow_prove(&headers, m, k)?;
            nipopow_verify(&proof)?;
            let json = JsonNipopowProof::from_proof(&proof);
            let text =
                serde_json::to_string_pretty(&json).map_err(|e| format!("serialize: {}", e))?;

            if let Some(path) = out_path {
                fs::write(&path, &text)
                    .map_err(|e| format!("failed to write {}: {}", path.display(), e))?;
                println!("Wrote NiPoPoW proof to {}", path.display());
            } else {
                println!("{}", text);
            }
            Ok(())
        }
        "nipopow-verify" => {
            if args.len() < 3 {
                return Err("nipopow-verify requires <proof_json>".to_string());
            }
            let proof_path = PathBuf::from(&args[2]);
            let proof = load_proof(&proof_path)?;
            nipopow_verify(&proof)?;
            let counts = nipopow_proof_counts(&proof);
            let best = nipopow_best_level(&counts, proof.m);
            println!(
                "NiPoPoW proof valid (m={}, k={}, pi={}, chi={})",
                proof.m,
                proof.k,
                proof.pi.len(),
                proof.chi.len()
            );
            println!("  best level: {}", best);
            Ok(())
        }
        "nipopow-compare-proofs" => {
            if args.len() < 4 {
                return Err("nipopow-compare-proofs requires <proof_a> <proof_b> [m]".to_string());
            }
            let proof_a = load_proof(&PathBuf::from(&args[2]))?;
            let proof_b = load_proof(&PathBuf::from(&args[3]))?;
            nipopow_verify(&proof_a)?;
            nipopow_verify(&proof_b)?;
            let m: usize = if args.len() >= 5 {
                args[4].parse().map_err(|e| format!("invalid m: {}", e))?
            } else {
                std::cmp::min(proof_a.m, proof_b.m)
            };
            use std::cmp::Ordering;
            let winner = match nipopow_compare_proofs(&proof_a, &proof_b, m) {
                Ordering::Greater => "A",
                Ordering::Less => "B",
                Ordering::Equal => "equal",
            };
            println!("NiPoPoW compare proofs (m={}): winner = {}", m, winner);
            Ok(())
        }
        _ => Err(format!("unknown command: {}", cmd)),
    }
}
