use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    mpsc::{sync_channel, SyncSender},
    OnceLock,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{
    env, fs,
    path::{Component, Path, PathBuf},
};

use serde::Serialize;

use bs58;
use sha2::{Digest, Sha256};

use crate::block::Block;

const IRIUM_P2PKH_VERSION: u8 = 0x39;

#[derive(Clone)]
struct PersistJob {
    height: u64,
    block: Block,
}

struct PersistWriter {
    sender: Option<SyncSender<PersistJob>>,
    async_mode: bool,
}

static PERSIST_WRITER: OnceLock<PersistWriter> = OnceLock::new();
static PERSIST_QUEUE_LEN: AtomicUsize = AtomicUsize::new(0);
static PERSISTED_HEIGHT: AtomicU64 = AtomicU64::new(0);
static PERSISTED_CONTIGUOUS_HEIGHT: AtomicU64 = AtomicU64::new(0);
static PERSISTED_MAX_HEIGHT_ON_DISK: AtomicU64 = AtomicU64::new(0);
static QUARANTINE_COUNT: AtomicU64 = AtomicU64::new(0);

fn persist_async_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        env::var("IRIUM_PERSIST_ASYNC")
            .ok()
            .map(|v| {
                let v = v.to_ascii_lowercase();
                !(v == "0" || v == "false" || v == "off")
            })
            .unwrap_or(true)
    })
}

fn persist_queue_capacity() -> usize {
    static CAP: OnceLock<usize> = OnceLock::new();
    *CAP.get_or_init(|| {
        env::var("IRIUM_PERSIST_QUEUE_CAP")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .map(|v| v.clamp(64, 65536))
            .unwrap_or(4096)
    })
}

pub fn persisted_height() -> u64 {
    PERSISTED_HEIGHT.load(Ordering::Relaxed)
}

pub fn set_persisted_height(height: u64) {
    let mut current = PERSISTED_HEIGHT.load(Ordering::Relaxed);
    while height > current {
        match PERSISTED_HEIGHT.compare_exchange_weak(
            current,
            height,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(v) => current = v,
        }
    }
}

pub fn persist_queue_len() -> usize {
    PERSIST_QUEUE_LEN.load(Ordering::Relaxed)
}

pub fn persisted_contiguous_height() -> u64 {
    PERSISTED_CONTIGUOUS_HEIGHT.load(Ordering::Relaxed)
}

pub fn set_persisted_contiguous_height(height: u64) {
    let mut current = PERSISTED_CONTIGUOUS_HEIGHT.load(Ordering::Relaxed);
    while height > current {
        match PERSISTED_CONTIGUOUS_HEIGHT.compare_exchange_weak(
            current,
            height,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(v) => current = v,
        }
    }
}

pub fn persisted_max_height_on_disk() -> u64 {
    PERSISTED_MAX_HEIGHT_ON_DISK.load(Ordering::Relaxed)
}

pub fn set_persisted_max_height_on_disk(height: u64) {
    let mut current = PERSISTED_MAX_HEIGHT_ON_DISK.load(Ordering::Relaxed);
    while height > current {
        match PERSISTED_MAX_HEIGHT_ON_DISK.compare_exchange_weak(
            current,
            height,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(v) => current = v,
        }
    }
}

pub fn quarantine_count() -> u64 {
    QUARANTINE_COUNT.load(Ordering::Relaxed)
}

pub fn reset_quarantine_count() {
    QUARANTINE_COUNT.store(0, Ordering::Relaxed);
}

pub fn add_quarantine_count(delta: u64) {
    if delta > 0 {
        QUARANTINE_COUNT.fetch_add(delta, Ordering::Relaxed);
    }
}

pub fn init_persist_writer() {
    let _ = PERSIST_WRITER.get_or_init(|| {
        if !persist_async_enabled() {
            return PersistWriter {
                sender: None,
                async_mode: false,
            };
        }

        let (tx, rx) = sync_channel::<PersistJob>(persist_queue_capacity());
        thread::spawn(move || {
            let mut last_checkpoint = Instant::now();
            loop {
                let job = match rx.recv() {
                    Ok(j) => j,
                    Err(_) => break,
                };
                if let Err(e) = write_block_json_sync(job.height, &job.block) {
                    eprintln!(
                        "[warn] persist writer failed for block {}: {}",
                        job.height, e
                    );
                }
                set_persisted_height(job.height);
                PERSIST_QUEUE_LEN.fetch_sub(1, Ordering::Relaxed);

                if last_checkpoint.elapsed() >= Duration::from_secs(5) {
                    eprintln!(
                        "[i] persist checkpoint: persisted_height={} queue_len={}",
                        persisted_height(),
                        persist_queue_len()
                    );
                    last_checkpoint = Instant::now();
                }
            }
        });

        PersistWriter {
            sender: Some(tx),
            async_mode: true,
        }
    });
}

pub fn drain_persist_queue(timeout: Duration) -> bool {
    let start = Instant::now();
    while persist_queue_len() > 0 {
        if start.elapsed() >= timeout {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
    true
}

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

fn maybe_advance_contiguous(dir: &Path, written_height: u64) {
    let contiguous = persisted_contiguous_height();
    if written_height != contiguous.saturating_add(1) {
        return;
    }
    let mut probe = contiguous.saturating_add(1);
    loop {
        let path = dir.join(format!("block_{}.json", probe));
        if path.exists() {
            set_persisted_contiguous_height(probe);
            probe = probe.saturating_add(1);
            continue;
        }
        break;
    }
}

fn write_block_json_sync(height: u64, block: &Block) -> std::io::Result<()> {
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
    fs::write(&path, json)?;
    set_persisted_height(height);
    set_persisted_max_height_on_disk(height);
    maybe_advance_contiguous(&dir, height);
    Ok(())
}

pub fn write_block_json(height: u64, block: &Block) -> std::io::Result<()> {
    init_persist_writer();
    if let Some(writer) = PERSIST_WRITER.get() {
        if writer.async_mode {
            if let Some(ref tx) = writer.sender {
                PERSIST_QUEUE_LEN.fetch_add(1, Ordering::Relaxed);
                if let Err(err) = tx.send(PersistJob {
                    height,
                    block: block.clone(),
                }) {
                    PERSIST_QUEUE_LEN.fetch_sub(1, Ordering::Relaxed);
                    return write_block_json_sync(err.0.height, &err.0.block);
                }
                return Ok(());
            }
        }
    }
    write_block_json_sync(height, block)
}
