use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose, Engine as _};
use irium_node_rs::constants::COINBASE_MATURITY;
use irium_node_rs::pow::sha256d;
use irium_node_rs::tx::{Transaction, TxInput, TxOutput};
use k256::ecdsa::signature::hazmat::PrehashSigner;
use k256::ecdsa::{Signature, SigningKey};
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::SecretKey;
use pbkdf2::pbkdf2_hmac;
use rand::rngs::OsRng;
use rand::RngCore;
use ripemd::Ripemd160;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::rpc_client::RpcClient;

const IRIUM_P2PKH_VERSION: u8 = 0x39;
const PBKDF2_ITERS: u32 = 100_000;
const DEFAULT_FEE_PER_BYTE: u64 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct WalletFile {
    version: u32,
    crypto: WalletCrypto,
}

#[derive(Debug, Serialize, Deserialize)]
struct WalletCrypto {
    salt: String,
    nonce: String,
    cipher: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WalletPlain {
    keys: Vec<WalletKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WalletKey {
    address: String,
    pkh: String,
    pubkey: String,
    privkey: String,
}

#[derive(Default)]
pub struct WalletState {
    unlocked: Option<WalletPlain>,
    last_touch: Option<Instant>,
    passphrase: Option<String>,
}

#[derive(Deserialize)]
struct BalanceResponse {
    balance: u64,
    utxo_count: usize,
}

#[derive(Deserialize)]
struct UtxosResponse {
    height: u64,
    utxos: Vec<UtxoItem>,
}

#[derive(Deserialize, Clone)]
struct UtxoItem {
    txid: String,
    index: u32,
    value: u64,
    height: u64,
    is_coinbase: bool,
    script_pubkey: String,
}

#[derive(Deserialize)]
struct HistoryResponse {
    height: u64,
    txs: Vec<HistoryItem>,
}

#[derive(Deserialize, Clone)]
struct HistoryItem {
    txid: String,
    height: u64,
    received: u64,
    spent: u64,
    net: i64,
    is_coinbase: bool,
}

#[derive(Serialize)]
struct SubmitTxRequest {
    tx_hex: String,
}

#[derive(Deserialize)]
struct FeeEstimateResponse {
    min_fee_per_byte: f64,
}

pub struct WalletService {
    pub state: WalletState,
}

fn derive_key(passphrase: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), salt, PBKDF2_ITERS, &mut key);
    key
}

fn encrypt_wallet(passphrase: &str, plain: &WalletPlain) -> Result<WalletFile, String> {
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let key = derive_key(passphrase, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let payload = serde_json::to_vec(plain).map_err(|e| e.to_string())?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), payload.as_ref())
        .map_err(|e| e.to_string())?;
    Ok(WalletFile {
        version: 1,
        crypto: WalletCrypto {
            salt: general_purpose::STANDARD.encode(salt),
            nonce: general_purpose::STANDARD.encode(nonce),
            cipher: general_purpose::STANDARD.encode(ciphertext),
        },
    })
}

fn decrypt_wallet(passphrase: &str, file: &WalletFile) -> Result<WalletPlain, String> {
    let salt = general_purpose::STANDARD
        .decode(&file.crypto.salt)
        .map_err(|e| e.to_string())?;
    let nonce = general_purpose::STANDARD
        .decode(&file.crypto.nonce)
        .map_err(|e| e.to_string())?;
    let cipher_bytes = general_purpose::STANDARD
        .decode(&file.crypto.cipher)
        .map_err(|e| e.to_string())?;
    let key = derive_key(passphrase, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let payload = cipher
        .decrypt(Nonce::from_slice(&nonce), cipher_bytes.as_ref())
        .map_err(|_| "Invalid passphrase".to_string())?;
    serde_json::from_slice(&payload).map_err(|e| e.to_string())
}

fn wallet_path(data_dir: &Path) -> PathBuf {
    data_dir.join("wallet.json")
}

fn save_wallet(path: &Path, file: &WalletFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(file).map_err(|e| e.to_string())?;
    std::fs::write(path, raw).map_err(|e| e.to_string())
}

fn load_wallet(path: &Path) -> Result<WalletFile, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

fn hash160(data: &[u8]) -> [u8; 20] {
    let sha = Sha256::digest(data);
    let rip = Ripemd160::digest(&sha);
    let mut out = [0u8; 20];
    out.copy_from_slice(&rip);
    out
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

fn base58_p2pkh_to_hash(addr: &str) -> Option<Vec<u8>> {
    let data = bs58::decode(addr).into_vec().ok()?;
    if data.len() < 25 {
        return None;
    }
    let (body, checksum) = data.split_at(data.len() - 4);
    let first = Sha256::digest(body);
    let second = Sha256::digest(&first);
    if &second[0..4] != checksum {
        return None;
    }
    if body.len() < 21 {
        return None;
    }
    if body[0] != IRIUM_P2PKH_VERSION {
        return None;
    }
    let payload = &body[1..];
    if payload.len() != 20 {
        return None;
    }
    Some(payload.to_vec())
}

fn generate_key() -> WalletKey {
    let secret = SecretKey::random(&mut OsRng);
    let public = secret.public_key();
    let pubkey = public.to_encoded_point(true);
    let pkh = hash160(pubkey.as_bytes());
    let address = base58_p2pkh_from_hash(&pkh);
    WalletKey {
        address,
        pkh: hex::encode(pkh),
        pubkey: hex::encode(pubkey.as_bytes()),
        privkey: hex::encode(secret.to_bytes()),
    }
}

fn parse_irm(s: &str) -> Result<u64, String> {
    if s.trim().is_empty() {
        return Err("empty amount".to_string());
    }
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() > 2 {
        return Err("invalid amount".to_string());
    }
    let whole: u64 = parts[0].parse().map_err(|_| "invalid amount".to_string())?;
    let frac = if parts.len() == 2 {
        let frac_str = parts[1];
        if frac_str.len() > 8 {
            return Err("too many decimals".to_string());
        }
        let mut frac_val: u64 = frac_str.parse().map_err(|_| "invalid amount".to_string())?;
        for _ in frac_str.len()..8 {
            frac_val *= 10;
        }
        frac_val
    } else {
        0
    };
    Ok(whole
        .saturating_mul(100_000_000)
        .saturating_add(frac))
}

fn format_irm(amount: u64) -> String {
    let whole = amount / 100_000_000;
    let frac = amount % 100_000_000;
    if frac == 0 {
        format!("{}", whole)
    } else {
        format!("{}.{}", whole, format!("{:08}", frac))
    }
}

fn p2pkh_script(pkh: &[u8; 20]) -> Vec<u8> {
    let mut script = Vec::with_capacity(25);
    script.push(0x76);
    script.push(0xa9);
    script.push(0x14);
    script.extend_from_slice(pkh);
    script.push(0x88);
    script.push(0xac);
    script
}

fn signature_digest(tx: &Transaction, input_index: usize, script_pubkey: &[u8]) -> [u8; 32] {
    let mut tx_copy = tx.clone();
    for (idx, input) in tx_copy.inputs.iter_mut().enumerate() {
        if idx == input_index {
            input.script_sig = script_pubkey.to_vec();
        } else {
            input.script_sig.clear();
        }
    }
    let mut data = tx_copy.serialize();
    data.extend_from_slice(&1u32.to_le_bytes());
    sha256d(&data)
}

fn hex_to_32(s: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(s).map_err(|_| "invalid hex".to_string())?;
    if bytes.len() != 32 {
        return Err("invalid txid length".to_string());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

impl WalletService {
    pub fn new() -> Self {
        Self {
            state: WalletState::default(),
        }
    }

    fn touch(&mut self) {
        self.state.last_touch = Some(Instant::now());
    }

    fn maybe_autolock(&mut self, auto_lock_minutes: u64) {
        if auto_lock_minutes == 0 {
            return;
        }
        if let Some(last) = self.state.last_touch {
            if last.elapsed() > Duration::from_secs(auto_lock_minutes * 60) {
                self.state.unlocked = None;
            }
        }
    }

    pub fn create_wallet(&mut self, data_dir: &Path, passphrase: &str) -> Result<String, String> {
        let path = wallet_path(data_dir);
        if path.exists() {
            return Err("Wallet already exists".to_string());
        }
        let key = generate_key();
        let plain = WalletPlain { keys: vec![key.clone()] };
        let file = encrypt_wallet(passphrase, &plain)?;
        save_wallet(&path, &file)?;
        self.state.unlocked = Some(plain);
        self.state.passphrase = Some(passphrase.to_string());
        self.touch();
        Ok(key.address)
    }

    pub fn unlock_wallet(&mut self, data_dir: &Path, passphrase: &str) -> Result<(), String> {
        let path = wallet_path(data_dir);
        let file = load_wallet(&path)?;
        let plain = decrypt_wallet(passphrase, &file)?;
        self.state.unlocked = Some(plain);
        self.state.passphrase = Some(passphrase.to_string());
        self.touch();
        Ok(())
    }

    pub fn lock_wallet(&mut self) {
        self.state.unlocked = None;
        self.state.passphrase = None;
        self.state.last_touch = None;
    }

    pub fn new_address(&mut self, data_dir: &Path) -> Result<String, String> {
        let mut plain = self
            .state
            .unlocked
            .clone()
            .ok_or_else(|| "Wallet locked".to_string())?;
        let passphrase = self
            .state
            .passphrase
            .clone()
            .ok_or_else(|| "Wallet locked".to_string())?;
        let key = generate_key();
        plain.keys.push(key.clone());
        let file = encrypt_wallet(&passphrase, &plain)?;
        save_wallet(&wallet_path(data_dir), &file)?;
        self.state.unlocked = Some(plain);
        self.state.passphrase = Some(passphrase);
        self.touch();
        Ok(key.address)
    }

    pub fn current_address(&mut self, auto_lock_minutes: u64) -> Result<String, String> {
        self.maybe_autolock(auto_lock_minutes);
        let plain = self
            .state
            .unlocked
            .as_ref()
            .ok_or_else(|| "Wallet locked".to_string())?;
        let key = plain.keys.last().ok_or_else(|| "No keys".to_string())?;
        self.touch();
        Ok(key.address.clone())
    }

    pub async fn balance(&mut self, client: &RpcClient, auto_lock_minutes: u64) -> Result<(String, String), String> {
        self.maybe_autolock(auto_lock_minutes);
        let plain = self
            .state
            .unlocked
            .as_ref()
            .ok_or_else(|| "Wallet locked".to_string())?;
        let mut total = 0u64;
        for key in &plain.keys {
            let payload: BalanceResponse = client
                .get_json(&format!("/rpc/balance?address={}", key.address))
                .await?;
            total = total.saturating_add(payload.balance);
        }
        self.touch();
        Ok((format_irm(total), "0.0".to_string()))
    }

    pub async fn history(&mut self, client: &RpcClient, auto_lock_minutes: u64, limit: usize) -> Result<Vec<HistoryItem>, String> {
        self.maybe_autolock(auto_lock_minutes);
        let plain = self
            .state
            .unlocked
            .as_ref()
            .ok_or_else(|| "Wallet locked".to_string())?;
        let mut all: Vec<HistoryItem> = Vec::new();
        for key in &plain.keys {
            let payload: HistoryResponse = client
                .get_json(&format!("/rpc/history?address={}", key.address))
                .await?;
            all.extend(payload.txs.into_iter());
        }
        all.sort_by(|a, b| b.height.cmp(&a.height));
        all.truncate(limit);
        self.touch();
        Ok(all)
    }

    pub async fn send(
        &mut self,
        client: &RpcClient,
        auto_lock_minutes: u64,
        to_addr: &str,
        amount_str: &str,
        fee_mode: &str,
    ) -> Result<String, String> {
        self.maybe_autolock(auto_lock_minutes);
        let plain = self
            .state
            .unlocked
            .as_ref()
            .ok_or_else(|| "Wallet locked".to_string())?
            .clone();
        let amount = parse_irm(amount_str)?;
        if base58_p2pkh_to_hash(to_addr).is_none() {
            return Err("Invalid destination address".to_string());
        }

        let mut utxos: Vec<(UtxoItem, WalletKey)> = Vec::new();
        let mut chain_height = 0u64;
        for key in &plain.keys {
            let payload: UtxosResponse = client
                .get_json(&format!("/rpc/utxos?address={}", key.address))
                .await?;
            chain_height = chain_height.max(payload.height);
            for u in payload.utxos {
                utxos.push((u, key.clone()));
            }
        }

        let mut fee_per_byte = DEFAULT_FEE_PER_BYTE;
        if fee_mode != "low" && fee_mode != "normal" && fee_mode != "high" {
            if let Ok(est) = client.get_json::<FeeEstimateResponse>("/rpc/fee_estimate").await {
                let est_fee = est.min_fee_per_byte.ceil() as u64;
                if est_fee > fee_per_byte {
                    fee_per_byte = est_fee;
                }
            }
        } else {
            if let Ok(est) = client.get_json::<FeeEstimateResponse>("/rpc/fee_estimate").await {
                let base = est.min_fee_per_byte.ceil() as u64;
                fee_per_byte = match fee_mode {
                    "low" => base.saturating_div(2).max(1),
                    "high" => base.saturating_mul(2),
                    _ => base,
                };
            }
        }

        utxos.sort_by_key(|(u, _)| u.value);
        let mut selected: Vec<(UtxoItem, WalletKey)> = Vec::new();
        let mut total = 0u64;
        let mut fee = 0u64;
        for (u, k) in utxos.iter() {
            let confirmations = chain_height.saturating_sub(u.height);
            if u.is_coinbase && confirmations < COINBASE_MATURITY {
                continue;
            }
            selected.push((u.clone(), k.clone()));
            total = total.saturating_add(u.value);
            let outputs = if total > amount { 2 } else { 1 };
            fee = estimate_tx_size(selected.len(), outputs).saturating_mul(fee_per_byte);
            if total >= amount.saturating_add(fee) {
                break;
            }
        }

        if total < amount.saturating_add(fee) {
            return Err("Insufficient funds".to_string());
        }

        let to_pkh = base58_p2pkh_to_hash(to_addr).ok_or_else(|| "Invalid address".to_string())?;
        let mut to_arr = [0u8; 20];
        to_arr.copy_from_slice(&to_pkh);
        let to_script = p2pkh_script(&to_arr);

        let mut inputs: Vec<TxInput> = Vec::new();
        let mut key_map: Vec<WalletKey> = Vec::new();
        for (u, k) in &selected {
            let txid = hex_to_32(&u.txid)?;
            inputs.push(TxInput {
                prev_txid: txid,
                prev_index: u.index,
                script_sig: Vec::new(),
                sequence: 0xffff_ffff,
            });
            key_map.push(k.clone());
        }

        let mut outputs = vec![TxOutput {
            value: amount,
            script_pubkey: to_script,
        }];

        let mut change = total.saturating_sub(amount).saturating_sub(fee);
        if change > 0 {
            let from_pkh = base58_p2pkh_to_hash(&selected[0].1.address).ok_or_else(|| "Invalid source address".to_string())?;
            let mut from_arr = [0u8; 20];
            from_arr.copy_from_slice(&from_pkh);
            let change_script = p2pkh_script(&from_arr);
            outputs.push(TxOutput {
                value: change,
                script_pubkey: change_script,
            });
        }

        let mut tx = Transaction {
            version: 1,
            inputs,
            outputs,
            locktime: 0,
        };

        for (idx, (u, k)) in selected.iter().enumerate() {
            let script_pubkey = hex::decode(&u.script_pubkey).map_err(|_| "Invalid script".to_string())?;
            let digest = signature_digest(&tx, idx, &script_pubkey);
            let priv_bytes = hex::decode(&k.privkey).map_err(|_| "Invalid privkey".to_string())?;
            let signing_key = SigningKey::from_bytes(priv_bytes.as_slice().into())
                .map_err(|_| "Invalid signing key".to_string())?;
            let sig: Signature = signing_key.sign_prehash(&digest).map_err(|e| e.to_string())?;
            let sig = sig.normalize_s().unwrap_or(sig);
            let mut sig_bytes = sig.to_der().as_bytes().to_vec();
            sig_bytes.push(0x01);
            let pub_bytes = hex::decode(&k.pubkey).map_err(|_| "Invalid pubkey".to_string())?;
            let mut script = Vec::new();
            script.push(sig_bytes.len() as u8);
            script.extend_from_slice(&sig_bytes);
            script.push(pub_bytes.len() as u8);
            script.extend_from_slice(&pub_bytes);
            tx.inputs[idx].script_sig = script;
        }

        let raw = tx.serialize();
        let req = SubmitTxRequest {
            tx_hex: hex::encode(raw),
        };
        let _resp: serde_json::Value = client.post_json("/rpc/submit_tx", &req).await?;
        self.touch();
        Ok(hex::encode(tx.txid()))
    }
}

fn estimate_tx_size(inputs: usize, outputs: usize) -> u64 {
    10 + inputs as u64 * 148 + outputs as u64 * 34
}
