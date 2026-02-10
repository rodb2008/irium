use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::storage;

const DEFAULT_SEEDLIST_BASELINE: &str = "bootstrap/seedlist.txt";
const DEFAULT_SEEDLIST_EXTRA: &str = "bootstrap/seedlist.extra";
const DEFAULT_SEEDLIST_RUNTIME: &str = "bootstrap/seedlist.runtime";
const DEFAULT_PEER_DB: &str = "state/peers.json";

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}


fn now_day() -> i64 {
    (now_secs() / 86_400.0).floor() as i64
}

fn repo_root() -> PathBuf {
    std::env::var("IRIUM_REPO_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn normalize_seed(addr: &str) -> Option<String> {
    let candidate = addr.trim();
    if candidate.is_empty() {
        return None;
    }
    if candidate.starts_with("/ip4/") {
        let parts: Vec<&str> = candidate.split('/').collect();
        if parts.len() >= 3 {
            return Some(parts[2].to_string());
        }
        return None;
    }
    if let Ok(ip) = candidate.parse::<IpAddr>() {
        return Some(ip.to_string());
    }
    // handle host:port form; strip port
    if let Ok(sock) = candidate.parse::<SocketAddr>() {
        return Some(sock.ip().to_string());
    }
    None
}

/// Record of an observed peer, mirroring `PeerRecord` in Python.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerRecord {
    pub multiaddr: String,
    pub agent: Option<String>,
    pub last_seen: f64,
    pub first_seen: f64,
    #[serde(default)]
    pub seen_days: Vec<i64>,
    pub relay_address: Option<String>,
    pub last_height: Option<u64>,
    pub node_id: Option<String>,
}

impl PeerRecord {
    pub fn new(multiaddr: String, agent: Option<String>) -> PeerRecord {
        let t = now_secs();
        let day = (t / 86_400.0).floor() as i64;
        PeerRecord {
            multiaddr,
            agent,
            last_seen: t,
            first_seen: t,
            seen_days: vec![day],
            relay_address: None,
            last_height: None,
            node_id: None,
        }
    }

    pub fn touch(&mut self) {
        self.last_seen = now_secs();
        let day = now_day();
        if !self.seen_days.contains(&day) {
            self.seen_days.push(day);
            self.seen_days.sort_unstable();
            if self.seen_days.len() > 30 {
                let start = self.seen_days.len() - 30;
                self.seen_days = self.seen_days[start..].to_vec();
            }
        }
    }
}

/// Manage baseline + runtime seedlists, mirroring `SeedlistManager`.
#[derive(Debug)]
pub struct SeedlistManager {
    baseline: PathBuf,
    extra: PathBuf,
    runtime: PathBuf,
    limit: usize,
}

impl SeedlistManager {
    pub fn new(limit: usize) -> SeedlistManager {
        let root = repo_root();
        let baseline = root.join(DEFAULT_SEEDLIST_BASELINE);
        let extra = root.join(DEFAULT_SEEDLIST_EXTRA);
        let runtime = root.join(DEFAULT_SEEDLIST_RUNTIME);
        if let Some(parent) = runtime.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if !runtime.exists() {
            let _ = fs::write(
                &runtime,
                "# Auto-generated runtime seedlist. Do not edit manually.\n",
            );
        }
        SeedlistManager {
            baseline,
            extra,
            runtime,
            limit,
        }
    }

    fn allow_unsigned_seedlist() -> bool {
        std::env::var("IRIUM_SEEDLIST_ALLOW_UNSIGNED")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    }

    fn runtime_seed_min_days() -> i64 {
        std::env::var("IRIUM_RUNTIME_SEED_DAYS")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(7)
            .clamp(1, 30)
    }

    fn runtime_seed_max_idle_hours() -> f64 {
        std::env::var("IRIUM_RUNTIME_SEED_MAX_IDLE_HOURS")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(24.0)
            .clamp(1.0, 168.0)
    }

    fn seedlist_sig_principal() -> String {
        std::env::var("IRIUM_SEEDLIST_SIG_PRINCIPAL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "bootstrap-signer".to_string())
    }

    fn seedlist_sig_namespace() -> String {
        std::env::var("IRIUM_SEEDLIST_SIG_NAMESPACE")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "file".to_string())
    }

    fn seedlist_allowed_signers() -> PathBuf {
        std::env::var("IRIUM_SEEDLIST_ALLOWED_SIGNERS")
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo_root().join("bootstrap/trust/allowed_signers"))
    }

    fn seedlist_sig_path(&self) -> PathBuf {
        PathBuf::from(format!("{}.sig", self.baseline.to_string_lossy()))
    }

    fn verify_seedlist_signature(&self) -> bool {
        let seed_data = match fs::read_to_string(&self.baseline) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let sig_path = self.seedlist_sig_path();
        if !sig_path.exists() {
            return false;
        }
        let allowed = Self::seedlist_allowed_signers();
        if !allowed.exists() {
            return false;
        }
        let principal = Self::seedlist_sig_principal();
        let namespace = Self::seedlist_sig_namespace();
        let mut child = match Command::new("ssh-keygen")
            .arg("-Y")
            .arg("verify")
            .arg("-f")
            .arg(&allowed)
            .arg("-I")
            .arg(&principal)
            .arg("-n")
            .arg(&namespace)
            .arg("-s")
            .arg(&sig_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => return false,
        };
        if let Some(stdin) = child.stdin.as_mut() {
            if stdin.write_all(seed_data.as_bytes()).is_err() {
                return false;
            }
        }
        match child.wait() {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    }
    fn load_seed_entries(&self, path: &PathBuf) -> Vec<String> {
        let mut entries = Vec::new();
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return entries,
        };
        for line in text.lines() {
            if let Some(ip) = normalize_seed(line) {
                if !entries.contains(&ip) {
                    entries.push(ip);
                }
            }
        }
        entries
    }

    fn load_runtime_entries(&self) -> Vec<String> {
        self.load_seed_entries(&self.runtime)
    }

    fn load_extra_entries(&self) -> Vec<String> {
        self.load_seed_entries(&self.extra)
    }

    pub fn write_runtime_entries<I>(&self, entries: I)
    where
        I: IntoIterator<Item = String>,
    {
        let mut unique = Vec::new();
        for addr in entries {
            if let Some(ip) = normalize_seed(&addr) {
                if !unique.contains(&ip) {
                    unique.push(ip);
                }
                if unique.len() >= self.limit {
                    break;
                }
            }
        }
        let timestamp = chrono::Utc::now().to_rfc3339();
        let mut body = format!("# Runtime seedlist refreshed {}\n", timestamp);
        for entry in &unique {
            body.push_str(entry);
            body.push('\n');
        }
        let _ = fs::write(&self.runtime, body);
    }

    pub fn merged_seedlist(&self) -> Vec<String> {
        let mut combined = Vec::new();
        let baseline_entries = if self.verify_seedlist_signature() {
            self.load_seed_entries(&self.baseline)
        } else if Self::allow_unsigned_seedlist() {
            eprintln!("Seedlist signature invalid or missing; using unsigned baseline seeds due to IRIUM_SEEDLIST_ALLOW_UNSIGNED=1");
            self.load_seed_entries(&self.baseline)
        } else {
            eprintln!("Seedlist signature invalid or missing; skipping baseline seeds");
            Vec::new()
        };
        for ip in baseline_entries {
            if !combined.contains(&ip) {
                combined.push(ip);
            }
        }
        for ip in self.load_extra_entries() {
            if !combined.contains(&ip) {
                combined.push(ip);
            }
        }
        for ip in self.load_runtime_entries() {
            if !combined.contains(&ip) {
                combined.push(ip);
            }
            if combined.len() >= self.limit {
                break;
            }
        }
        combined
    }
}

/// Persistent peer directory, mirroring Python `PeerDirectory`.
#[derive(Debug)]
pub struct PeerDirectory {
    db_path: PathBuf,
    seed_manager: SeedlistManager,
    records: HashMap<String, PeerRecord>,
}

impl PeerDirectory {
    pub fn new() -> PeerDirectory {
        let db_path = storage::state_dir().join("peers.json");
        if !db_path.exists() {
            let root = repo_root();
            let legacy = root.join(DEFAULT_PEER_DB);
            if legacy.exists() {
                if let Some(parent) = db_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::copy(&legacy, &db_path);
            }
        }
        if let Some(parent) = db_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let seed_manager = SeedlistManager::new(512);
        let mut dir = PeerDirectory {
            db_path,
            seed_manager,
            records: HashMap::new(),
        };
        dir.load();
        dir
    }

    fn load(&mut self) {
        let text = match fs::read_to_string(&self.db_path) {
            Ok(t) => t,
            Err(_) => return,
        };
        let parsed: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => return,
        };
        let map = match parsed.as_object() {
            Some(m) => m,
            None => return,
        };
        for (addr, value) in map {
            if let Some(obj) = value.as_object() {
                let agent = obj
                    .get("agent")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let seen_days = obj
                    .get("seen_days")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_i64())
                            .collect::<Vec<i64>>()
                    })
                    .unwrap_or_default();
                let first_seen = obj
                    .get("first_seen")
                    .and_then(|v| v.as_f64())
                    .unwrap_or_else(now_secs);
                let last_seen = obj
                    .get("last_seen")
                    .and_then(|v| v.as_f64())
                    .unwrap_or_else(now_secs);
                let relay_address = obj
                    .get("relay_address")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let last_height = obj.get("last_height").and_then(|v| v.as_u64());
                let node_id = obj
                    .get("node_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                self.records.insert(
                    addr.clone(),
                    PeerRecord {
                        multiaddr: addr.clone(),
                        agent,
                        first_seen,
                        last_seen,
                        seen_days,
                        relay_address,
                        last_height,
                        node_id,
                    },
                );
            }
        }
    }

    fn flush(&self) {
        let mut map = serde_json::Map::new();
        for (addr, rec) in &self.records {
            let mut obj = serde_json::Map::new();
            if let Some(agent) = &rec.agent {
                obj.insert(
                    "agent".to_string(),
                    serde_json::Value::String(agent.clone()),
                );
            }
            if let Some(relay) = &rec.relay_address {
                obj.insert(
                    "relay_address".to_string(),
                    serde_json::Value::String(relay.clone()),
                );
            }
            if !rec.seen_days.is_empty() {
                let arr = rec
                    .seen_days
                    .iter()
                    .map(|v| serde_json::Value::Number(serde_json::Number::from(*v)))
                    .collect();
                obj.insert("seen_days".to_string(), serde_json::Value::Array(arr));
            }
            obj.insert(
                "first_seen".to_string(),
                serde_json::Value::Number(
                    serde_json::Number::from_f64(rec.first_seen)
                        .unwrap_or_else(|| serde_json::Number::from(0)),
                ),
            );
            obj.insert(
                "last_seen".to_string(),
                serde_json::Value::Number(
                    serde_json::Number::from_f64(rec.last_seen)
                        .unwrap_or_else(|| serde_json::Number::from(0)),
                ),
            );
            if let Some(h) = rec.last_height {
                obj.insert(
                    "last_height".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(h)),
                );
            }
            if let Some(id) = &rec.node_id {
                obj.insert("node_id".to_string(), serde_json::Value::String(id.clone()));
            }
            map.insert(addr.clone(), serde_json::Value::Object(obj));
        }
        let value = serde_json::Value::Object(map);
        if let Ok(text) = serde_json::to_string_pretty(&value) {
            if let Err(e) = fs::write(&self.db_path, text) {
                eprintln!("Failed to write peer db {}: {}", self.db_path.display(), e);
            }
        }
    }

    /// Register a successful connection and update runtime seedlist via policy.
    pub fn register_connection(
        &mut self,
        multiaddr: String,
        agent: Option<String>,
        relay_address: Option<String>,
        node_id: Option<String>,
    ) {
        let entry = self
            .records
            .entry(multiaddr.clone())
            .or_insert_with(|| PeerRecord::new(multiaddr.clone(), agent.clone()));
        entry.agent = agent;
        entry.relay_address = relay_address.or(entry.relay_address.clone());
        entry.node_id = node_id.or(entry.node_id.clone());
        entry.touch();

        self.flush();
        self.refresh_seedlist_with_policy();
    }

    /// Register a peer from a hint list without marking it active.
    pub fn register_peer_hint(&mut self, multiaddr: String) {
        if self.records.contains_key(&multiaddr) {
            return;
        }
        let t = now_secs();
        self.records.insert(
            multiaddr.clone(),
            PeerRecord {
                multiaddr,
                agent: None,
                last_seen: 0.0,
                first_seen: t,
                seen_days: Vec::new(),
                relay_address: None,
                last_height: None,
                node_id: None,
            },
        );
        self.flush();
    }

    /// Mark a peer as seen without changing its metadata.
    pub fn mark_seen(&mut self, multiaddr: &str) {
        if let Some(rec) = self.records.get_mut(multiaddr) {
            let before = rec.seen_days.len();
            rec.touch();
            if rec.seen_days.len() != before {
                self.flush();
            }
        }
    }

    pub fn peers(&self) -> Vec<PeerRecord> {
        self.records.values().cloned().collect()
    }

    pub fn relay_address_for_peer(&self, socket: &SocketAddr) -> Option<String> {
        let multiaddr = format!("/ip4/{}/tcp/{}", socket.ip(), socket.port());
        self.records
            .get(&multiaddr)
            .and_then(|r| r.relay_address.clone())
    }

    /// Apply seedlist policy: promote peers active for 7 consecutive days,
    /// drop peers idle > 24h. Baseline seeds remain in the static seedlist file.
    pub fn refresh_seedlist_with_policy(&self) {
        let now = now_secs();
        let today = now_day();
        let min_days = SeedlistManager::runtime_seed_min_days();
        let max_idle = SeedlistManager::runtime_seed_max_idle_hours();
        let start_day = today.saturating_sub(min_days.saturating_sub(1));
        let mut seeds = Vec::new();

        for rec in self.records.values() {
            let idle_hours = (now - rec.last_seen) / 3600.0;
            let mut active_days = 0;
            for day in start_day..=today {
                if rec.seen_days.contains(&day) {
                    active_days += 1;
                }
            }
            if active_days >= min_days && idle_hours <= max_idle {
                if let Some(ip) = normalize_seed(&rec.multiaddr) {
                    seeds.push(ip);
                }
            }
        }

        seeds.sort();
        seeds.dedup();
        self.seed_manager.write_runtime_entries(seeds);
    }

    pub fn record_height(&mut self, multiaddr: &str, height: u64) {
        if let Some(rec) = self.records.get_mut(multiaddr) {
            rec.last_height = Some(height);
            rec.touch();
            self.flush();
        }
    }
}
