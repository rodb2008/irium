use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_SEEDLIST_BASELINE: &str = "bootstrap/seedlist.txt";
const DEFAULT_SEEDLIST_RUNTIME: &str = "bootstrap/seedlist.runtime";
const DEFAULT_PEER_DB: &str = "state/peers.json";

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn repo_root() -> PathBuf {
    std::env::var("IRIUM_REPO_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn normalize_multiaddr(addr: &str) -> Result<String, String> {
    let candidate = addr.trim();
    if candidate.is_empty() {
        return Err("empty multiaddr".to_string());
    }
    if !candidate.starts_with('/') {
        return Err("multiaddr must start with '/'".to_string());
    }
    Ok(candidate.to_string())
}

/// Record of an observed peer, mirroring `PeerRecord` in Python.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerRecord {
    pub multiaddr: String,
    pub agent: Option<String>,
    pub last_seen: f64,
    pub first_seen: f64,
    pub relay_address: Option<String>,
}

impl PeerRecord {
    pub fn new(multiaddr: String, agent: Option<String>) -> PeerRecord {
        let t = now_secs();
        PeerRecord {
            multiaddr,
            agent,
            last_seen: t,
            first_seen: t,
            relay_address: None,
        }
    }

    pub fn touch(&mut self) {
        self.last_seen = now_secs();
    }
}

/// Manage baseline + runtime seedlists, mirroring `SeedlistManager`.
#[derive(Debug)]
pub struct SeedlistManager {
    baseline: PathBuf,
    runtime: PathBuf,
    limit: usize,
}

impl SeedlistManager {
    pub fn new(limit: usize) -> SeedlistManager {
        let root = repo_root();
        let baseline = root.join(DEFAULT_SEEDLIST_BASELINE);
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
            runtime,
            limit,
        }
    }

    fn load_runtime_entries(&self) -> Vec<String> {
        let mut entries = Vec::new();
        let text = match fs::read_to_string(&self.runtime) {
            Ok(t) => t,
            Err(_) => return entries,
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            entries.push(line.to_string());
        }
        entries
    }

    pub fn write_runtime_entries<I>(&self, entries: I)
    where
        I: IntoIterator<Item = String>,
    {
        let mut normalised = Vec::new();
        for addr in entries {
            match normalize_multiaddr(&addr) {
                Ok(a) => normalised.push(a),
                Err(e) => {
                    eprintln!("Skipping invalid runtime seed {}: {}", addr, e);
                }
            }
        }
        let mut unique = Vec::new();
        for entry in normalised {
            if !unique.contains(&entry) {
                unique.push(entry);
            }
            if unique.len() >= self.limit {
                break;
            }
        }
        let timestamp = chrono::Utc::now().to_rfc3339();
        let mut body = format!("# Runtime seedlist refreshed {}\n", timestamp);
        for entry in &unique {
            body.push_str(entry);
            body.push('\n');
        }
        if let Err(e) = fs::write(&self.runtime, body) {
            eprintln!(
                "Failed to write runtime seedlist {}: {}",
                self.runtime.display(),
                e
            );
        }
    }

    pub fn merged_seedlist(&self) -> Vec<String> {
        let mut baseline_entries = Vec::new();
        if let Ok(text) = fs::read_to_string(&self.baseline) {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                baseline_entries.push(line.to_string());
            }
        }

        let mut combined = Vec::new();
        for e in baseline_entries
            .into_iter()
            .chain(self.load_runtime_entries())
        {
            if !combined.contains(&e) {
                combined.push(e);
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
        let root = repo_root();
        let db_path = root.join(DEFAULT_PEER_DB);
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
                self.records.insert(
                    addr.clone(),
                    PeerRecord {
                        multiaddr: addr.clone(),
                        agent,
                        first_seen,
                        last_seen,
                        relay_address,
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
            map.insert(addr.clone(), serde_json::Value::Object(obj));
        }
        let value = serde_json::Value::Object(map);
        if let Ok(text) = serde_json::to_string_pretty(&value) {
            if let Err(e) = fs::write(&self.db_path, text) {
                eprintln!("Failed to write peer db {}: {}", self.db_path.display(), e);
            }
        }
    }

    /// Register a successful connection and optionally update runtime seedlist.
    pub fn register_connection(
        &mut self,
        multiaddr: String,
        agent: Option<String>,
        relay_address: Option<String>,
    ) {
        let entry = self
            .records
            .entry(multiaddr.clone())
            .or_insert_with(|| PeerRecord::new(multiaddr.clone(), agent.clone()));
        entry.agent = agent;
        entry.relay_address = relay_address.or(entry.relay_address.clone());
        entry.touch();

        self.flush();

        // Simple heuristic: keep all observed peers in runtime seedlist,
        // letting SeedlistManager enforce size limits and deduplication.
        let entries: Vec<String> = self.records.keys().cloned().collect();
        self.seed_manager.write_runtime_entries(entries);
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
}
