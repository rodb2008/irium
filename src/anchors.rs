#![allow(dead_code)]
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Error type for anchor loading and validation.
#[derive(thiserror::Error, Debug)]
pub enum AnchorError {
    #[error("failed to read anchors file: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to parse anchors JSON: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnchorHeader {
    pub height: u64,
    pub hash: String,
    pub timestamp: u64,
    pub prev_hash: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnchorFile {
    pub anchors: Vec<AnchorHeader>,
    #[serde(default)]
    pub trusted_signers: Vec<String>,
    #[serde(default)]
    pub signatures: Vec<serde_json::Value>,
}

/// Lightweight anchor manager for Rust node and miner.
///
/// This mirrors the read-only aspects of the Python `AnchorManager`:
/// - tracks anchors and trusted_signers
/// - exposes the canonical payload digest
/// - provides helpers to verify blocks against anchors or check tip consistency
///
/// It deliberately does *not* verify SSH signatures; on mainnet that
/// verification is performed off-line and the signed file is shipped
/// with releases.
#[derive(Debug, Clone)]
pub struct AnchorManager {
    anchors: Vec<AnchorHeader>,
    #[allow(dead_code)]
    trusted_signers: Vec<String>,
    payload_digest: String,
}

impl AnchorManager {
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, AnchorError> {
        let path = path.as_ref();
        let data = fs::read_to_string(path)?;

        // Compute canonical payload digest (anchors + trusted_signers + signatures),
        // mirroring Python's `_canonical_payload_bytes` logic.
        let raw_value: serde_json::Value = serde_json::from_str(&data)?;
        let mut obj = match raw_value {
            serde_json::Value::Object(map) => map,
            _ => {
                let io_err =
                    io::Error::new(io::ErrorKind::InvalidData, "anchors file must be an object");
                return Err(AnchorError::Json(serde_json::Error::io(io_err)));
            }
        };
        obj.remove("signatures");
        let canonical = serde_json::to_vec(&serde_json::Value::Object(obj))?;
        let digest = Sha256::digest(&canonical);
        let payload_digest = hex::encode(digest);

        let parsed: AnchorFile = serde_json::from_str(&data)?;

        Ok(AnchorManager {
            anchors: {
                let mut v = parsed.anchors;
                v.sort_by_key(|a| a.height);
                v
            },
            trusted_signers: parsed.trusted_signers,
            payload_digest,
        })
    }

    pub fn from_default_repo_root(repo_root: PathBuf) -> Result<Self, AnchorError> {
        let default_path = repo_root.join("bootstrap").join("anchors.json");
        AnchorManager::load_from_path(default_path)
    }

    pub fn payload_digest(&self) -> &str {
        &self.payload_digest
    }

    pub fn anchors(&self) -> &[AnchorHeader] {
        &self.anchors
    }

    pub fn get_anchor_at_height(&self, height: u64) -> Option<&AnchorHeader> {
        self.anchors.iter().find(|a| a.height == height)
    }

    pub fn get_latest_anchor(&self) -> Option<&AnchorHeader> {
        self.anchors.last()
    }

    /// Return true if the given block hash matches the anchor for that height,
    /// or if there is no anchor at that height.
    pub fn verify_block_against_anchors(&self, height: u64, block_hash: &str) -> bool {
        match self.get_anchor_at_height(height) {
            Some(anchor) => anchor.hash.eq_ignore_ascii_case(block_hash),
            None => true,
        }
    }

    /// Return true if the tip is consistent with the most recent anchor at or
    /// below the given height.
    pub fn is_chain_valid(&self, chain_tip_height: u64, chain_tip_hash: &str) -> bool {
        let mut relevant_anchor: Option<&AnchorHeader> = None;
        for anchor in self.anchors.iter().rev() {
            if anchor.height <= chain_tip_height {
                relevant_anchor = Some(anchor);
                break;
            }
        }

        let Some(anchor) = relevant_anchor else {
            return true;
        };

        if chain_tip_height == anchor.height {
            anchor.hash.eq_ignore_ascii_case(chain_tip_hash)
        } else {
            // Full header-by-header validation is handled elsewhere; as in Python,
            // we accept tips that extend beyond the last anchor height.
            true
        }
    }
}
