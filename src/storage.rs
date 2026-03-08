use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    mpsc::{sync_channel, SyncSender, TrySendError},
    Mutex, OnceLock,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{
    collections::BTreeSet,
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
static PERSISTED_WINDOW_TIP: AtomicU64 = AtomicU64::new(0);
static MISSING_PERSISTED_IN_WINDOW: AtomicU64 = AtomicU64::new(0);
static MISSING_OR_MISMATCH_IN_WINDOW: AtomicU64 = AtomicU64::new(0);
static EXPECTED_HASH_COVERAGE_IN_WINDOW: AtomicU64 = AtomicU64::new(0);
static EXPECTED_HASH_WINDOW_SPAN: AtomicU64 = AtomicU64::new(0);
static GAP_HEALER_ACTIVE: AtomicBool = AtomicBool::new(false);
static GAP_HEALER_LAST_PROGRESS_TS: AtomicU64 = AtomicU64::new(0);
static GAP_HEALER_LAST_FILLED_HEIGHT: AtomicU64 = AtomicU64::new(0);
static GAP_HEALER_PENDING: OnceLock<Mutex<BTreeSet<u64>>> = OnceLock::new();
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

pub fn force_set_persisted_contiguous_height(height: u64) {
    PERSISTED_CONTIGUOUS_HEIGHT.store(height, Ordering::Relaxed);
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

pub fn persisted_window_tip() -> u64 {
    PERSISTED_WINDOW_TIP.load(Ordering::Relaxed)
}

pub fn set_persisted_window_tip(height: u64) {
    let mut current = PERSISTED_WINDOW_TIP.load(Ordering::Relaxed);
    while height > current {
        match PERSISTED_WINDOW_TIP.compare_exchange_weak(
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

pub fn missing_persisted_in_window() -> u64 {
    MISSING_PERSISTED_IN_WINDOW.load(Ordering::Relaxed)
}

pub fn set_missing_persisted_in_window(missing: u64) {
    MISSING_PERSISTED_IN_WINDOW.store(missing, Ordering::Relaxed);
}
pub fn missing_or_mismatch_in_window() -> u64 {
    MISSING_OR_MISMATCH_IN_WINDOW.load(Ordering::Relaxed)
}

pub fn set_missing_or_mismatch_in_window(count: u64) {
    MISSING_OR_MISMATCH_IN_WINDOW.store(count, Ordering::Relaxed);
}

pub fn expected_hash_coverage_in_window() -> u64 {
    EXPECTED_HASH_COVERAGE_IN_WINDOW.load(Ordering::Relaxed)
}

pub fn set_expected_hash_coverage_in_window(count: u64) {
    EXPECTED_HASH_COVERAGE_IN_WINDOW.store(count, Ordering::Relaxed);
}

pub fn expected_hash_window_span() -> u64 {
    EXPECTED_HASH_WINDOW_SPAN.load(Ordering::Relaxed)
}

pub fn set_expected_hash_window_span(span: u64) {
    EXPECTED_HASH_WINDOW_SPAN.store(span, Ordering::Relaxed);
}

fn gap_healer_pending_set() -> &'static Mutex<BTreeSet<u64>> {
    GAP_HEALER_PENDING.get_or_init(|| Mutex::new(BTreeSet::new()))
}

fn gap_healer_touch_progress(height: u64) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    GAP_HEALER_LAST_PROGRESS_TS.store(now, Ordering::Relaxed);
    GAP_HEALER_LAST_FILLED_HEIGHT.store(height, Ordering::Relaxed);
}

pub fn gap_healer_active() -> bool {
    GAP_HEALER_ACTIVE.load(Ordering::Relaxed)
}

pub fn set_gap_healer_active(active: bool) {
    GAP_HEALER_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn gap_healer_last_progress_ts() -> u64 {
    GAP_HEALER_LAST_PROGRESS_TS.load(Ordering::Relaxed)
}

pub fn gap_healer_last_filled_height() -> Option<u64> {
    let h = GAP_HEALER_LAST_FILLED_HEIGHT.load(Ordering::Relaxed);
    if h == 0 {
        None
    } else {
        Some(h)
    }
}

pub fn gap_healer_pending_count() -> u64 {
    gap_healer_pending_set()
        .lock()
        .map(|g| g.len() as u64)
        .unwrap_or(0)
}

pub fn set_gap_healer_target_heights(heights: &[u64]) {
    if let Ok(mut g) = gap_healer_pending_set().lock() {
        g.clear();
        for h in heights {
            g.insert(*h);
        }
        MISSING_OR_MISMATCH_IN_WINDOW.store(g.len() as u64, Ordering::Relaxed);
    }
}

pub fn set_gap_healer_missing_heights(heights: &[u64]) {
    set_gap_healer_target_heights(heights);
}

pub fn gap_healer_batch(limit: usize) -> Vec<u64> {
    if let Ok(g) = gap_healer_pending_set().lock() {
        return g.iter().copied().take(limit).collect();
    }
    Vec::new()
}

pub fn gap_healer_mark_filled(height: u64) -> bool {
    if let Ok(mut g) = gap_healer_pending_set().lock() {
        if g.remove(&height) {
            MISSING_OR_MISMATCH_IN_WINDOW.store(g.len() as u64, Ordering::Relaxed);
            gap_healer_touch_progress(height);
            return true;
        }
    }
    false
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
    #[serde(skip_serializing_if = "Option::is_none")]
    submit_source: Option<String>,
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

fn configured_dir(var: &str) -> Option<PathBuf> {
    let home = os_home_dir();

    let raw = env::var_os(var)?;
    let candidate = PathBuf::from(raw);
    let normalized = normalize_under(&home, &candidate)?;
    if normalized.starts_with(&home) {
        Some(normalized)
    } else {
        None
    }
}

fn runtime_root_dir() -> PathBuf {
    configured_dir("IRIUM_DATA_DIR").unwrap_or_else(|| os_home_dir().join(".irium"))
}

pub fn blocks_dir() -> PathBuf {
    configured_dir("IRIUM_BLOCKS_DIR").unwrap_or_else(|| runtime_root_dir().join("blocks"))
}

pub fn state_dir() -> PathBuf {
    configured_dir("IRIUM_STATE_DIR").unwrap_or_else(|| runtime_root_dir().join("state"))
}

pub fn ensure_runtime_dirs() -> std::io::Result<(PathBuf, PathBuf)> {
    let blocks = blocks_dir();
    fs::create_dir_all(&blocks)?;
    let state = state_dir();
    fs::create_dir_all(&state)?;
    Ok((blocks, state))
}

fn sanitize_filename_fragment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

fn validate_block_data_path(path: &Path, height: u64) -> std::io::Result<()> {
    let expected_name = format!("block_{}.json", height);
    let file_name = path.file_name().and_then(|s| s.to_str()).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid block filename")
    })?;
    if file_name != expected_name {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "unexpected block filename",
        ));
    }

    let blocks = blocks_dir();
    fs::create_dir_all(&blocks)?;
    let blocks_canon = fs::canonicalize(&blocks).unwrap_or(blocks);

    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing parent directory")
    })?;
    let parent_canon = fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());

    if parent_canon != blocks_canon {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "block path escapes blocks directory",
        ));
    }

    Ok(())
}

fn validate_block_quarantine_path(path: &Path, height: u64) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "missing quarantine parent",
        )
    })?;
    let main_path = block_json_path_for_height(height)?;
    let main_parent = main_path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing block parent")
    })?;
    let parent_canon = fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    let main_parent_canon =
        fs::canonicalize(main_parent).unwrap_or_else(|_| main_parent.to_path_buf());
    if parent_canon != main_parent_canon {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "quarantine path escapes blocks directory",
        ));
    }
    Ok(())
}

fn block_json_path_for_height(height: u64) -> std::io::Result<PathBuf> {
    if height > 100_000_000 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "height out of range",
        ));
    }
    let dir = blocks_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("block_{}.json", height));
    validate_block_data_path(&path, height)?;
    Ok(path)
}

fn maybe_quarantine_existing_block(height: u64, new_hash: &str) -> std::io::Result<()> {
    let path = block_json_path_for_height(height)?;

    if !path.exists() {
        return Ok(());
    }

    validate_block_data_path(&path, height)?;

    let existing = match fs::read_to_string(&path) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    let existing_hash = serde_json::from_str::<serde_json::Value>(&existing)
        .ok()
        .and_then(|v| {
            v.get("header")
                .and_then(|h| h.get("hash"))
                .or_else(|| {
                    v.get("block")
                        .and_then(|b| b.get("header"))
                        .and_then(|h| h.get("hash"))
                })
                .and_then(|h| h.as_str())
                .map(|s| s.to_string())
        });

    if existing_hash.as_deref() == Some(new_hash) {
        return Ok(());
    }

    let stem =
        sanitize_filename_fragment(path.file_stem().and_then(|s| s.to_str()).unwrap_or("block"));
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("json");
    let existing_hash_suffix =
        sanitize_filename_fragment(existing_hash.as_deref().unwrap_or("unknown"));

    let mut dest = path.with_file_name(format!("{}.fork.{}.{}", stem, existing_hash_suffix, ext));
    if dest.exists() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        dest = path.with_file_name(format!(
            "{}.fork.{}.{}.{}",
            stem, existing_hash_suffix, stamp, ext
        ));
    }

    // Best-effort quarantine; if rename fails, keep the existing file.
    let _ = (|| -> std::io::Result<()> {
        validate_block_data_path(&path, height)?;
        validate_block_quarantine_path(&dest, height)?;
        fs::rename(path, dest)?;
        Ok(())
    })();
    Ok(())
}

fn maybe_advance_contiguous(_dir: &Path, written_height: u64) {
    let contiguous = persisted_contiguous_height();
    if written_height == contiguous.saturating_add(1) {
        // Advance strictly one height at a time based on freshly persisted blocks.
        // Do not leap ahead by file existence alone; old fork files can create false continuity.
        set_persisted_contiguous_height(written_height);
    }
}

fn write_block_json_sync(height: u64, block: &Block) -> std::io::Result<()> {
    let path = block_json_path_for_height(height)?;
    let dir = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(blocks_dir);

    let header = &block.header;
    let hash = header.hash();

    let new_hash = hex::encode(hash);
    let _ = maybe_quarantine_existing_block(height, &new_hash);

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
        submit_source: None,
    };

    let json = serde_json::to_string_pretty(&jb)?;
    validate_block_data_path(&path, height)?;
    fs::write(&path, json)?;
    set_persisted_height(height);
    set_persisted_max_height_on_disk(height);
    maybe_advance_contiguous(&dir, height);
    Ok(())
}

pub fn write_block_json_with_source(
    height: u64,
    block: &Block,
    submit_source: Option<&str>,
) -> std::io::Result<()> {
    let path = block_json_path_for_height(height)?;
    let mut value = if path.exists() {
        let raw = fs::read_to_string(&path)?;
        serde_json::from_str::<serde_json::Value>(&raw)
            .unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if let Some(src) = submit_source {
        value["submit_source"] = serde_json::Value::String(src.to_string());
    }

    validate_block_data_path(&path, height)?;
    let json = serde_json::to_string_pretty(&value)?;
    fs::write(&path, json)?;
    let _ = block;
    Ok(())
}

pub fn read_block_submit_source(height: u64) -> Option<String> {
    let path = block_json_path_for_height(height).ok()?;
    let raw = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value
        .get("submit_source")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

pub fn write_block_json(height: u64, block: &Block) -> std::io::Result<()> {
    init_persist_writer();
    if let Some(writer) = PERSIST_WRITER.get() {
        if writer.async_mode {
            if let Some(ref tx) = writer.sender {
                // Never block async/P2P tasks on a full persist queue.
                let job = PersistJob {
                    height,
                    block: block.clone(),
                };
                match tx.try_send(job) {
                    Ok(()) => {
                        PERSIST_QUEUE_LEN.fetch_add(1, Ordering::Relaxed);
                        return Ok(());
                    }
                    Err(TrySendError::Full(job)) | Err(TrySendError::Disconnected(job)) => {
                        return write_block_json_sync(job.height, &job.block);
                    }
                }
            }
        }
    }
    write_block_json_sync(height, block)
}
