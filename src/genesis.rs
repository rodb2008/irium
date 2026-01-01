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

pub fn repo_root() -> PathBuf {
    // Prefer the manifest dir if it already contains configs/.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest_dir.join("configs").join("genesis-locked.json").exists() {
        return manifest_dir;
    }
    manifest_dir
        .parent()
        .unwrap_or_else(|| manifest_dir.as_path())
        .to_path_buf()
}

pub fn locked_genesis_path() -> PathBuf {
    repo_root().join("configs").join("genesis-locked.json")
}

pub fn load_locked_genesis() -> Result<LockedGenesis, GenesisError> {
    let path = locked_genesis_path();
    let data = fs::read_to_string(&path)?;
    let genesis: LockedGenesis = serde_json::from_str(&data)?;
    Ok(genesis)
}
