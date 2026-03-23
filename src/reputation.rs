use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::storage;

const REPUTATION_PRUNE_AFTER_SECS: f64 = 30.0 * 24.0 * 60.0 * 60.0;
const REPUTATION_LOW_ACTIVITY_THRESHOLD: u32 = 3;

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn default_reputation_path() -> PathBuf {
    let path = storage::state_dir().join("peer_reputation.json");
    if !path.exists() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
        let legacy = PathBuf::from(home).join(".irium/peer_reputation.json");
        if legacy.exists() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::copy(&legacy, &path);
        }
    }
    path
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerReputation {
    pub peer_id: String,
    pub score: i32,
    pub successful_connections: u32,
    pub failed_connections: u32,
    pub blocks_received: u32,
    pub invalid_blocks: u32,
    pub uptime_proofs: u32,
    pub last_seen: f64,
}

impl PeerReputation {
    pub fn new(peer_id: String) -> PeerReputation {
        PeerReputation {
            peer_id,
            score: 100,
            successful_connections: 0,
            failed_connections: 0,
            blocks_received: 0,
            invalid_blocks: 0,
            uptime_proofs: 0,
            last_seen: now_secs(),
        }
    }

    pub fn update_score(&mut self) {
        let mut score = 100_i32;
        score += (self.successful_connections as i32) * 2;
        score -= (self.failed_connections as i32) * 5;
        score += (self.blocks_received as i32) * 10;
        score -= (self.invalid_blocks as i32) * 50;
        score += (self.uptime_proofs as i32) * 5;
        if score < 0 {
            score = 0;
        }
        if score > 1000 {
            score = 1000;
        }
        self.score = score;
    }

    pub fn is_trusted(&self) -> bool {
        self.score > 80
    }

    pub fn is_banned(&self) -> bool {
        self.score < 20
    }

    pub fn total_activity(&self) -> u32 {
        self.successful_connections
            .saturating_add(self.failed_connections)
            .saturating_add(self.blocks_received)
            .saturating_add(self.invalid_blocks)
            .saturating_add(self.uptime_proofs)
    }

    pub fn should_prune(&self, now: f64) -> bool {
        if self.last_seen <= 0.0 || now <= self.last_seen {
            return false;
        }
        let age = now - self.last_seen;
        age >= REPUTATION_PRUNE_AFTER_SECS
            && (self.successful_connections == 0
                || self.total_activity() <= REPUTATION_LOW_ACTIVITY_THRESHOLD)
    }
}

#[derive(Debug)]
pub struct ReputationManager {
    path: PathBuf,
    reputations: HashMap<String, PeerReputation>,
}

impl ReputationManager {
    pub fn banned_count(&self) -> usize {
        self.reputations.values().filter(|r| r.is_banned()).count()
    }

    pub fn new() -> ReputationManager {
        let path = default_reputation_path();
        let mut mgr = ReputationManager {
            path,
            reputations: HashMap::new(),
        };
        mgr.load();
        mgr
    }

    fn load(&mut self) {
        let text = match fs::read_to_string(&self.path) {
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
        let now = now_secs();
        let mut pruned_any = false;
        for (peer_id, value) in map {
            if let Some(obj) = value.as_object() {
                let mut rep = PeerReputation::new(peer_id.clone());
                rep.score = obj.get("score").and_then(|v| v.as_i64()).unwrap_or(100) as i32;
                rep.successful_connections = obj
                    .get("successful_connections")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                rep.failed_connections = obj
                    .get("failed_connections")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                rep.blocks_received = obj
                    .get("blocks_received")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                rep.invalid_blocks = obj
                    .get("invalid_blocks")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                rep.uptime_proofs = obj
                    .get("uptime_proofs")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                rep.last_seen = obj
                    .get("last_seen")
                    .and_then(|v| v.as_f64())
                    .unwrap_or_else(now_secs);
                rep.update_score();
                if rep.should_prune(now) {
                    pruned_any = true;
                    continue;
                }
                self.reputations.insert(peer_id.clone(), rep);
            }
        }
        if pruned_any {
            self.save();
        }
    }

    fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut map = serde_json::Map::new();
        for (peer_id, rep) in &self.reputations {
            map.insert(
                peer_id.clone(),
                serde_json::json!({
                    "score": rep.score,
                    "successful_connections": rep.successful_connections,
                    "failed_connections": rep.failed_connections,
                    "blocks_received": rep.blocks_received,
                    "invalid_blocks": rep.invalid_blocks,
                    "uptime_proofs": rep.uptime_proofs,
                    "last_seen": rep.last_seen,
                }),
            );
        }
        let value = serde_json::Value::Object(map);
        let Ok(text) = serde_json::to_string_pretty(&value) else {
            return;
        };
        let tmp = self.path.with_extension(format!("json.tmp.{}", process::id()));
        if let Ok(mut file) = File::create(&tmp) {
            if file.write_all(text.as_bytes()).is_ok() && file.sync_all().is_ok() {
                let _ = fs::rename(&tmp, &self.path);
                return;
            }
        }
        let _ = fs::remove_file(&tmp);
    }

    pub fn get_reputation(&mut self, peer_id: &str) -> &mut PeerReputation {
        self.reputations
            .entry(peer_id.to_string())
            .or_insert_with(|| PeerReputation::new(peer_id.to_string()))
    }

    pub fn record_success(&mut self, peer_id: &str) {
        let rep = self.get_reputation(peer_id);
        rep.successful_connections = rep.successful_connections.saturating_add(1);
        rep.last_seen = now_secs();
        rep.update_score();
        self.save();
    }

    pub fn record_failure(&mut self, peer_id: &str) {
        let rep = self.get_reputation(peer_id);
        rep.failed_connections = rep.failed_connections.saturating_add(1);
        rep.last_seen = now_secs();
        rep.update_score();
        self.save();
    }

    pub fn record_block(&mut self, peer_id: &str, valid: bool) {
        let rep = self.get_reputation(peer_id);
        if valid {
            rep.blocks_received = rep.blocks_received.saturating_add(1);
        } else {
            rep.invalid_blocks = rep.invalid_blocks.saturating_add(1);
        }
        rep.last_seen = now_secs();
        rep.update_score();
        self.save();
    }

    pub fn record_uptime_proof(&mut self, peer_id: &str) {
        let rep = self.get_reputation(peer_id);
        rep.uptime_proofs = rep.uptime_proofs.saturating_add(1);
        rep.last_seen = now_secs();
        rep.update_score();
        self.save();
    }

    pub fn is_banned(&mut self, peer_id: &str) -> bool {
        self.get_reputation(peer_id).is_banned()
    }

    pub fn score_of(&mut self, peer_id: &str) -> i32 {
        let rep = self.get_reputation(peer_id);
        rep.score
    }

    pub fn record_decode_error(&mut self, peer_id: &str) {
        let rep = self.get_reputation(peer_id);
        rep.invalid_blocks = rep.invalid_blocks.saturating_add(1);
        rep.last_seen = now_secs();
        rep.update_score();
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prunes_old_low_activity_entries() {
        let now = now_secs();
        let mut rep = PeerReputation::new("1.2.3.4:38291".to_string());
        rep.last_seen = now - (REPUTATION_PRUNE_AFTER_SECS + 60.0);
        rep.failed_connections = 1;
        rep.update_score();
        assert!(rep.should_prune(now));
    }

    #[test]
    fn keeps_old_successful_entries() {
        let now = now_secs();
        let mut rep = PeerReputation::new("1.2.3.4:38291".to_string());
        rep.last_seen = now - (REPUTATION_PRUNE_AFTER_SECS + 60.0);
        rep.successful_connections = 2;
        rep.failed_connections = 5;
        rep.blocks_received = 2;
        rep.update_score();
        assert!(!rep.should_prune(now));
    }
}
