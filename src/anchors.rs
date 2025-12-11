#![allow(dead_code)]
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

/// Error type for anchor loading and validation.
#[derive(thiserror::Error, Debug)]
pub enum AnchorError {
    #[error("failed to read anchors file: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to parse anchors JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("anchor signature verification failed: {0}")]
    Verification(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnchorHeader {
    pub height: u64,
    pub hash: String,
    pub timestamp: u64,
    pub prev_hash: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnchorMetadata {
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnchorSignature {
    pub signer: String,
    pub public_key: String,
    pub namespace: String,
    pub algorithm: String,
    pub signature: String,
    #[serde(default)]
    pub signed_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnchorFile {
    pub anchors: Vec<AnchorHeader>,
    #[serde(default)]
    pub trusted_signers: Vec<String>,
    #[serde(default)]
    pub signatures: Vec<AnchorSignature>,
    #[serde(default)]
    pub metadata: Option<AnchorMetadata>,
}

/// Lightweight anchor manager for Rust node and miner.
///
/// This mirrors the read-only aspects of the Python `AnchorManager`:
/// - tracks anchors and trusted_signers
/// - exposes the canonical payload digest
/// - provides helpers to verify blocks against anchors or check tip consistency
///
/// SSH signatures are verified at load time against `bootstrap/trust/allowed_anchor_signers`.
#[derive(Debug, Clone)]
pub struct AnchorManager {
    anchors: Vec<AnchorHeader>,
    #[allow(dead_code)]
    trusted_signers: Vec<String>,
    payload_digest: String,
}

fn min_anchor_signers() -> usize {
    std::env::var("IRIUM_ANCHOR_MIN_SIGNERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(1)
}

fn anchor_expired(meta: &Option<AnchorMetadata>) -> Result<(), AnchorError> {
    if let Some(m) = meta {
        if let Some(exp) = &m.expires_at {
            let ts = chrono::DateTime::parse_from_rfc3339(exp)
                .map_err(|e| AnchorError::Verification(format!("invalid anchor expiry: {}", e)))?
                .with_timezone(&chrono::Utc);
            if chrono::Utc::now() > ts {
                return Err(AnchorError::Verification("anchor file expired".to_string()));
            }
        }
    }
    Ok(())
}

fn verify_anchor_signatures(
    canonical_bytes: &[u8],
    parsed: &AnchorFile,
    base_dir: &Path,
) -> Result<(), AnchorError> {
    if parsed.signatures.is_empty() {
        return Err(AnchorError::Verification(
            "no anchor signatures present".to_string(),
        ));
    }
    if parsed.trusted_signers.is_empty() {
        return Err(AnchorError::Verification(
            "no trusted signers declared".to_string(),
        ));
    }
    let allowlist = base_dir.join("trust").join("allowed_anchor_signers");
    if !allowlist.exists() {
        return Err(AnchorError::Verification(format!(
            "missing allowlist {}",
            allowlist.display()
        )));
    }
    let mut verified_signers = HashSet::new();
    let required = min_anchor_signers();
    for sig in &parsed.signatures {
        if !parsed.trusted_signers.iter().any(|s| s == &sig.signer) {
            continue;
        }
        if verified_signers.contains(&sig.signer) {
            continue;
        }
        let mut payload_path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        payload_path.push(format!("irium-anchor-{}.payload", nanos));
        let mut sig_path = payload_path.clone();
        sig_path.set_extension("sig");
        let _ = fs::write(&payload_path, canonical_bytes);
        let body = format!(
            "-----BEGIN SSH SIGNATURE-----
{}
-----END SSH SIGNATURE-----
",
            sig.signature.trim()
        );
        let _ = fs::write(&sig_path, body);

        let mut child = match Command::new("ssh-keygen")
            .arg("-Y")
            .arg("verify")
            .arg("-f")
            .arg(&allowlist)
            .arg("-I")
            .arg(&sig.signer)
            .arg("-n")
            .arg(&sig.namespace)
            .arg("-s")
            .arg(&sig_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = fs::remove_file(&payload_path);
                let _ = fs::remove_file(&sig_path);
                return Err(AnchorError::Verification(format!(
                    "ssh-keygen spawn failed: {}",
                    e
                )));
            }
        };

        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(canonical_bytes);
        }
        let status = match child.wait() {
            Ok(s) => s,
            Err(e) => {
                let _ = fs::remove_file(&payload_path);
                let _ = fs::remove_file(&sig_path);
                return Err(AnchorError::Verification(format!(
                    "ssh-keygen wait failed: {}",
                    e
                )));
            }
        };
        let _ = fs::remove_file(&payload_path);
        let _ = fs::remove_file(&sig_path);
        if status.success() {
            verified_signers.insert(sig.signer.clone());
            if verified_signers.len() >= required {
                break;
            }
        }
    }

    if verified_signers.len() >= required {
        Ok(())
    } else {
        Err(AnchorError::Verification(format!(
            "valid anchor signatures below threshold: {}/{}",
            verified_signers.len(),
            required
        )))
    }
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
        let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
        anchor_expired(&parsed.metadata)?;
        verify_anchor_signatures(&canonical, &parsed, base_dir)?;

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
