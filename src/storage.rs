use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    env, fs,
    path::{Component, Path, PathBuf},
};

use serde::Serialize;

use bs58;
use sha2::{Digest, Sha256};

use crate::block::Block;

const IRIUM_P2PKH_VERSION: u8 = 0x39;

fn sanitize_filename_component(name: &std::ffi::OsStr) -> String {
    // This file name ultimately comes from `Path::file_name()`, but we still sanitize
    // to defend against path traversal if future callers ever pass tainted input.
    let s = name.to_string_lossy();
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let ok = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-');
        out.push(if ok { ch } else { '_' });
    }
    let trimmed = out.trim_matches('.');
    if trimmed.is_empty() {
        "file".to_string()
    } else {
        out
    }
}

fn p2pkh_hash_from_script(script: &[u8]) -> Option<[u8; 20]> {
    if script.len() != 25 {
        return None;
    }
    if script[0] != 0x76 || script[1] != 0xa9 || script[2] != 0x14 {
        return None;
    }
    if script[23] != 0x88 || script[24] != 0xac {
        return None;
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&script[3..23]);
    Some(out)
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

fn miner_address_from_block(block: &Block) -> Option<String> {
    let tx = block.transactions.first()?;
    let output = tx.outputs.first()?;
    let pkh = p2pkh_hash_from_script(&output.script_pubkey)?;
    Some(base58_p2pkh_from_hash(&pkh))
}

#[derive(Serialize)]
struct JsonHeader {
    version: u32,
    prev_hash: String,
    merkle_root: String,
    time: u32,
    bits: String,
    nonce: u32,
    hash: String,
}

#[derive(Serialize)]
struct JsonBlock {
    height: u64,
    header: JsonHeader,
    tx_hex: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    miner_address: Option<String>,
}

#[cfg(unix)]
fn os_home_dir() -> PathBuf {
    // Prefer OS account database over $HOME to avoid env-tainted paths.
    unsafe {
        let uid = libc::geteuid();
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let mut buf = vec![0u8; 16 * 1024];
        let rc = libc::getpwuid_r(
            uid,
            &mut pwd,
            buf.as_mut_ptr() as *mut _,
            buf.len(),
            &mut result,
        );
        if rc == 0 && !result.is_null() && !pwd.pw_dir.is_null() {
            if let Ok(dir) = std::ffi::CStr::from_ptr(pwd.pw_dir).to_str() {
                return PathBuf::from(dir);
            }
        }
    }
    PathBuf::from("/")
}

#[cfg(not(unix))]
fn os_home_dir() -> PathBuf {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn normalize_under(base: &Path, input: &Path) -> Option<PathBuf> {
    let mut out = if input.is_absolute() {
        PathBuf::new()
    } else {
        base.to_path_buf()
    };

    let base_depth = base.components().count();
    for comp in input.components() {
        match comp {
            Component::Prefix(_) => return None,
            Component::RootDir => out.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                if out.components().count() <= base_depth {
                    return None;
                }
                out.pop();
            }
            Component::Normal(s) => out.push(s),
        }
    }
    Some(out)
}

fn configured_dir(var: &str, default_rel: &Path) -> PathBuf {
    let home = os_home_dir();

    if let Some(raw) = env::var_os(var) {
        let candidate = PathBuf::from(raw);
        if let Some(normalized) = normalize_under(&home, &candidate) {
            if normalized.starts_with(&home) {
                return normalized;
            }
        }
    }

    home.join(default_rel)
}

pub fn blocks_dir() -> PathBuf {
    configured_dir("IRIUM_BLOCKS_DIR", Path::new(".irium/blocks"))
}

pub fn state_dir() -> PathBuf {
    configured_dir("IRIUM_STATE_DIR", Path::new(".irium/state"))
}

pub fn ensure_runtime_dirs() -> std::io::Result<(PathBuf, PathBuf)> {
    let blocks = blocks_dir();
    fs::create_dir_all(&blocks)?;
    let state = state_dir();
    fs::create_dir_all(&state)?;
    Ok((blocks, state))
}

fn maybe_quarantine_existing_block(path: &Path, new_hash: &str) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let existing = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    let existing_hash = serde_json::from_str::<serde_json::Value>(&existing)
        .ok()
        .and_then(|v| {
            v.get("header")
                .and_then(|h| h.get("hash"))
                .and_then(|h| h.as_str())
                .map(|s| s.to_string())
        });

    if existing_hash.as_deref() == Some(new_hash) {
        return Ok(());
    }

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let backup_dir = dir.join(format!("orphaned_{}", stamp));
    fs::create_dir_all(&backup_dir)?;

    let name = path.file_name().unwrap_or_default();
    let safe_name = sanitize_filename_component(name);
    let mut dest = backup_dir.join(&safe_name);
    if dest.exists() {
        // Avoid clobbering an existing quarantine file.
        let mut n = 1u32;
        loop {
            let candidate = backup_dir.join(format!("{safe_name}.dup{n}"));
            if !candidate.exists() {
                dest = candidate;
                break;
            }
            n += 1;
        }
    }

    // Best-effort quarantine; if rename fails, keep the existing file.
    let _ = fs::rename(path, dest);
    Ok(())
}

pub fn write_block_json(height: u64, block: &Block) -> std::io::Result<()> {
    let dir = blocks_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("block_{}.json", height));

    let header = &block.header;
    let hash = header.hash();

    let new_hash = hex::encode(hash);
    let _ = maybe_quarantine_existing_block(&path, &new_hash);

    let jb = JsonBlock {
        height,
        header: JsonHeader {
            version: header.version,
            prev_hash: hex::encode(header.prev_hash),
            merkle_root: hex::encode(header.merkle_root),
            time: header.time,
            bits: format!("{:08x}", header.bits),
            nonce: header.nonce,
            hash: new_hash.clone(),
        },
        tx_hex: block
            .transactions
            .iter()
            .map(|tx| hex::encode(tx.serialize()))
            .collect(),
        miner_address: miner_address_from_block(block),
    };

    let json = serde_json::to_string_pretty(&jb)?;
    fs::write(path, json)
}
