use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct GenesisHeader {
    pub version: u32,
    pub prev_hash: String,
    pub merkle_root: String,
    pub time: u64,
    pub bits: String,
    pub nonce: u32,
    pub hash: String,
}

#[derive(Debug, Deserialize)]
pub struct LockedGenesis {
    #[allow(dead_code)]
    pub height: u64,
    pub header: GenesisHeader,
    #[serde(default)]
    pub transactions: Vec<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum GenesisError {
    #[error("failed to read genesis file: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to parse genesis JSON: {0}")]
    Json(#[from] serde_json::Error),
}

// Genesis locked at build time so the binary is self-contained on all platforms.
// External file takes precedence (for testing overrides), then falls back to embedded.
const GENESIS_LOCKED_BYTES: &[u8] = include_bytes!("../configs/genesis-locked.json");

pub fn repo_root() -> PathBuf {
    // Try CARGO_MANIFEST_DIR (works during development with source present).
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest_dir.join("configs").join("genesis-locked.json").exists() {
        return manifest_dir;
    }
    // Try relative to the running executable (handles installed binaries).
    if let Ok(exe) = std::env::current_exe() {
        for candidate in exe.ancestors().skip(1).take(3) {
            if candidate.join("configs").join("genesis-locked.json").exists() {
                return candidate.to_path_buf();
            }
        }
    }
    // Try current working directory.
    if let Ok(cwd) = std::env::current_dir() {
        if cwd.join("configs").join("genesis-locked.json").exists() {
            return cwd;
        }
    }
    manifest_dir
}

pub fn locked_genesis_path() -> PathBuf {
    repo_root().join("configs").join("genesis-locked.json")
}

pub fn load_locked_genesis() -> Result<LockedGenesis, GenesisError> {
    // Prefer external file (allows environment-specific overrides for testing).
    let path = locked_genesis_path();
    if path.exists() {
        let data = fs::read_to_string(&path)?;
        return Ok(serde_json::from_str(&data)?);
    }
    // Fall back to genesis compiled into the binary — works with no external files.
    Ok(serde_json::from_slice(GENESIS_LOCKED_BYTES)?)
}
