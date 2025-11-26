use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::tx::{decode_full_tx, Transaction};

#[derive(Debug, Clone)]
pub struct MempoolEntry {
    pub tx: Transaction,
    pub raw: Vec<u8>,
    pub fee: u64,
    pub size: usize,
    pub fee_per_byte: f64,
    pub added: u64,
    pub relays: Vec<String>,
    pub relay_addresses: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AddOutcome {
    pub txid: [u8; 32],
    pub evicted: Option<[u8; 32]>,
}

#[derive(Serialize, Deserialize)]
struct DiskEntry {
    hex: String,
    fee: Option<u64>,
    size: Option<usize>,
    fee_per_byte: Option<f64>,
    added: Option<u64>,
    txid: Option<String>,
    relays: Option<Vec<String>>,
    relay_addresses: Option<Vec<String>>,
}

pub struct MempoolManager {
    entries: HashMap<[u8; 32], MempoolEntry>,
    path: PathBuf,
    max_entries: usize,
    min_fee_per_byte: f64,
}

impl MempoolManager {
    pub fn new(path: PathBuf, max_entries: usize, min_fee_per_byte: f64) -> MempoolManager {
        let mut mgr = MempoolManager {
            entries: HashMap::new(),
            path,
            max_entries,
            min_fee_per_byte,
        };
        mgr.load_from_disk();
        mgr
    }

    fn load_from_disk(&mut self) {
        let data = match fs::read_to_string(&self.path) {
            Ok(d) => d,
            Err(_) => return,
        };
        let parsed: Vec<DiskEntry> = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Failed to parse mempool file {}: {e}", self.path.display());
                return;
            }
        };

        for entry in parsed {
            let raw = match hex::decode(&entry.hex) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("Invalid tx hex in mempool: {e}");
                    continue;
                }
            };
            let tx = match decode_full_tx(&raw) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Invalid mempool tx decode: {e}");
                    continue;
                }
            };
            let txid = tx.txid();
            let size = entry.size.unwrap_or(raw.len());
            let fee = entry.fee.unwrap_or(0);
            let fpb = entry.fee_per_byte.unwrap_or_else(|| {
                if size > 0 {
                    fee as f64 / size as f64
                } else {
                    0.0
                }
            });
            let added = entry.added.unwrap_or_else(now_secs);
            let relays = entry.relays.unwrap_or_default();
            let relay_addresses = entry.relay_addresses.unwrap_or_default();
            self.entries.insert(
                txid,
                MempoolEntry {
                    tx,
                    raw,
                    fee,
                    size,
                    fee_per_byte: fpb,
                    added,
                    relays,
                    relay_addresses,
                },
            );
        }
    }

    fn persist(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut disk_entries: Vec<DiskEntry> = Vec::with_capacity(self.entries.len());
        for (txid, entry) in self.entries.iter() {
            disk_entries.push(DiskEntry {
                hex: hex::encode(&entry.raw),
                fee: Some(entry.fee),
                size: Some(entry.size),
                fee_per_byte: Some(entry.fee_per_byte),
                added: Some(entry.added),
                txid: Some(hex::encode(txid)),
                relays: Some(entry.relays.clone()),
                relay_addresses: Some(entry.relay_addresses.clone()),
            });
        }
        let json = serde_json::to_string_pretty(&disk_entries).map_err(|e| e.to_string())?;
        fs::write(&self.path, json).map_err(|e| e.to_string())
    }

    pub fn add_transaction(
        &mut self,
        tx: Transaction,
        raw: Vec<u8>,
        fee: u64,
    ) -> Result<AddOutcome, String> {
        let txid = tx.txid();
        if self.entries.contains_key(&txid) {
            return Err("Transaction already in mempool".to_string());
        }
        let size = raw.len();
        let fee_per_byte = if size > 0 {
            fee as f64 / size as f64
        } else {
            0.0
        };
        if fee_per_byte < self.min_fee_per_byte {
            return Err("Fee per byte below minimum policy".to_string());
        }

        let mut evicted = None;
        if self.entries.len() >= self.max_entries {
            if let Some((lowest_txid, lowest)) = self
                .entries
                .iter()
                .min_by(|a, b| a.1.fee_per_byte.partial_cmp(&b.1.fee_per_byte).unwrap())
            {
                if fee_per_byte <= lowest.fee_per_byte {
                    return Err("Mempool full and fee too low".to_string());
                }
                evicted = Some(*lowest_txid);
            }
        }
        if let Some(e) = evicted {
            self.entries.remove(&e);
        }

        let entry = MempoolEntry {
            tx,
            raw,
            fee,
            size,
            fee_per_byte,
            added: now_secs(),
            relays: Vec::new(),
            relay_addresses: Vec::new(),
        };
        self.entries.insert(txid, entry);
        self.persist()?;

        Ok(AddOutcome { txid, evicted })
    }

    pub fn record_relay(&mut self, txid: &[u8; 32], peer: String) {
        if let Some(entry) = self.entries.get_mut(txid) {
            if !entry.relays.contains(&peer) {
                entry.relays.push(peer);
                let _ = self.persist();
            }
        }
    }

    pub fn record_relay_address(&mut self, txid: &[u8; 32], address: String) {
        if let Some(entry) = self.entries.get_mut(txid) {
            if !entry.relay_addresses.contains(&address) {
                entry.relay_addresses.push(address);
                let _ = self.persist();
            }
        }
    }

    pub fn remove(&mut self, txid: &[u8; 32]) -> bool {
        let removed = self.entries.remove(txid).is_some();
        if removed {
            let _ = self.persist();
        }
        removed
    }

    pub fn contains(&self, txid: &[u8; 32]) -> bool {
        self.entries.contains_key(txid)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn ordered_transactions(&self) -> Vec<Transaction> {
        let mut vec: Vec<&MempoolEntry> = self.entries.values().collect();
        vec.sort_by(|a, b| {
            b.fee_per_byte
                .partial_cmp(&a.fee_per_byte)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        vec.into_iter().map(|e| e.tx.clone()).collect()
    }

    pub fn ordered_entries(&self) -> Vec<MempoolEntry> {
        let mut vec: Vec<MempoolEntry> = self.entries.values().cloned().collect();
        vec.sort_by(|a, b| {
            b.fee_per_byte
                .partial_cmp(&a.fee_per_byte)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        vec
    }

    pub fn txids_hex(&self) -> Vec<String> {
        self.entries.keys().map(|h| hex::encode(h)).collect()
    }

    pub fn raw_tx(&self, txid: &[u8; 32]) -> Option<Vec<u8>> {
        self.entries.get(txid).map(|e| e.raw.clone())
    }

    pub fn relays_for(&self, txid: &[u8; 32]) -> Vec<String> {
        self.entries
            .get(txid)
            .map(|e| e.relays.clone())
            .unwrap_or_default()
    }

    pub fn relay_addresses_for(&self, txid: &[u8; 32]) -> Vec<String> {
        self.entries
            .get(txid)
            .map(|e| e.relay_addresses.clone())
            .unwrap_or_default()
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "mempool_test_{}_{}_{}",
            name,
            std::process::id(),
            now_secs()
        ));
        p
    }

    fn dummy_tx(value: u64) -> Transaction {
        let input = crate::tx::TxInput {
            prev_txid: [1u8; 32],
            prev_index: 0,
            script_sig: vec![0u8; 1],
            sequence: 0xffff_fffe,
        };
        let output = crate::tx::TxOutput {
            value,
            script_pubkey: vec![0u8],
        };
        Transaction {
            version: 1,
            inputs: vec![input],
            outputs: vec![output],
            locktime: 0,
        }
    }

    #[test]
    fn adds_and_evicts_by_fee() {
        let path = tmp_path("evict");
        let mut mgr = MempoolManager::new(path.clone(), 1, 0.0);

        let tx_low = dummy_tx(10);
        let raw_low = tx_low.serialize();
        let _ = mgr.add_transaction(tx_low.clone(), raw_low, 1).unwrap();
        assert_eq!(mgr.len(), 1);

        let tx_high = dummy_tx(20);
        let raw_high = tx_high.serialize();
        let res = mgr.add_transaction(tx_high.clone(), raw_high, 100);
        assert!(res.is_ok());
        assert_eq!(mgr.len(), 1);
        assert!(mgr.contains(&tx_high.txid()));
        assert!(!mgr.contains(&tx_low.txid()));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn records_relay_and_address() {
        let path = tmp_path("relay");
        let mut mgr = MempoolManager::new(path.clone(), 10, 0.0);
        let tx = dummy_tx(5);
        let raw = tx.serialize();
        let txid = tx.txid();
        mgr.add_transaction(tx.clone(), raw, 1).unwrap();
        mgr.record_relay(&txid, "peer1".to_string());
        mgr.record_relay_address(&txid, "aa".to_string());

        let entry = mgr.entries.get(&txid).unwrap();
        assert!(entry.relays.contains(&"peer1".to_string()));
        assert!(entry.relay_addresses.contains(&"aa".to_string()));

        let _ = std::fs::remove_file(path);
    }
}
