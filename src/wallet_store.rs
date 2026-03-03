use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use bs58;
use pbkdf2::pbkdf2_hmac;
use rand_core::{OsRng, RngCore};
use ripemd::Ripemd160;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::SecretKey;

const IRIUM_P2PKH_VERSION: u8 = 0x39;
const WIF_VERSION: u8 = 0x80;
const PBKDF2_ITERS: u32 = 100_000;
const WALLET_VERSION: u32 = 1;
const DEFAULT_AUTO_LOCK_MIN: u64 = 10;

#[derive(Debug, Serialize, Deserialize)]
pub struct WalletFile {
    version: u32,
    crypto: WalletCrypto,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WalletCrypto {
    salt: String,
    nonce: String,
    cipher: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletPlain {
    pub keys: Vec<WalletKey>,
    #[serde(default)]
    pub seed_hex: Option<String>,
    #[serde(default)]
    pub next_index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletKey {
    pub address: String,
    pub pkh: String,
    pub pubkey: String,
    pub privkey: String,
}

#[derive(Default)]
struct WalletState {
    unlocked: Option<WalletPlain>,
    passphrase: Option<String>,
    last_touch: Option<Instant>,
}

pub struct WalletManager {
    path: PathBuf,
    state: WalletState,
}

impl WalletManager {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            state: WalletState::default(),
        }
    }

    pub fn default_path() -> PathBuf {
        if let Ok(path) = env::var("IRIUM_NODE_WALLET_FILE") {
            return PathBuf::from(path);
        }
        if let Ok(path) = env::var("IRIUM_WALLET_FILE") {
            return PathBuf::from(path);
        }
        let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
        PathBuf::from(home).join(".irium/wallet.core.json")
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    pub fn is_unlocked(&self) -> bool {
        self.state.unlocked.is_some()
    }

    pub fn create(&mut self, passphrase: &str) -> Result<WalletKey, String> {
        self.create_with_seed(passphrase, None)
    }

    pub fn create_with_seed(
        &mut self,
        passphrase: &str,
        seed_hex: Option<&str>,
    ) -> Result<WalletKey, String> {
        if passphrase.trim().is_empty() {
            return Err("passphrase required".to_string());
        }
        if self.path.exists() {
            return Err("wallet already exists".to_string());
        }

        let mut plain = WalletPlain {
            keys: Vec::new(),
            seed_hex: None,
            next_index: 0,
        };

        let key = if let Some(seed) = seed_hex {
            let clean = normalize_seed_hex(seed)?;
            let secret = derive_secret_from_seed_hex(&clean, 0)?;
            plain.seed_hex = Some(clean);
            plain.next_index = 1;
            key_from_secret(&secret, true)
        } else {
            let clean = generate_seed_hex();
            let secret = derive_secret_from_seed_hex(&clean, 0)?;
            plain.seed_hex = Some(clean);
            plain.next_index = 1;
            key_from_secret(&secret, true)
        };

        plain.keys.push(key.clone());
        let file = encrypt_wallet(passphrase, &plain)?;
        save_wallet(&self.path, &file)?;
        self.state.unlocked = Some(plain);
        self.state.passphrase = Some(passphrase.to_string());
        self.touch();
        Ok(key)
    }

    pub fn unlock(&mut self, passphrase: &str) -> Result<(), String> {
        if passphrase.trim().is_empty() {
            return Err("passphrase required".to_string());
        }
        let file = load_wallet(&self.path)?;
        let mut plain = decrypt_wallet(passphrase, &file)?;
        if plain.next_index == 0 && plain.seed_hex.is_some() {
            plain.next_index = plain.keys.len() as u32;
        }
        self.state.unlocked = Some(plain);
        self.state.passphrase = Some(passphrase.to_string());
        self.touch();
        Ok(())
    }

    pub fn lock(&mut self) {
        self.state = WalletState::default();
    }

    pub fn addresses(&mut self) -> Result<Vec<String>, String> {
        self.ensure_unlocked()?;
        let addrs = self
            .state
            .unlocked
            .as_ref()
            .map(|w| w.keys.iter().map(|k| k.address.clone()).collect())
            .unwrap_or_default();
        self.touch();
        Ok(addrs)
    }

    pub fn current_address(&mut self) -> Result<String, String> {
        self.ensure_unlocked()?;
        let addr = self
            .state
            .unlocked
            .as_ref()
            .and_then(|w| w.keys.last())
            .map(|k| k.address.clone())
            .ok_or_else(|| "wallet has no keys".to_string())?;
        self.touch();
        Ok(addr)
    }

    pub fn new_address(&mut self) -> Result<WalletKey, String> {
        self.ensure_unlocked()?;
        let passphrase = self
            .state
            .passphrase
            .clone()
            .ok_or_else(|| "wallet locked".to_string())?;

        let key = if let Some(ref mut plain) = self.state.unlocked {
            let key = if let Some(seed_hex) = plain.seed_hex.clone() {
                let index = plain.next_index;
                let secret = derive_secret_from_seed_hex(&seed_hex, index)?;
                let k = key_from_secret(&secret, true);
                plain.next_index = plain.next_index.saturating_add(1);
                k
            } else {
                generate_key()
            };

            if plain.keys.iter().any(|k| k.address == key.address) {
                return Err("derived address already exists".to_string());
            }
            plain.keys.push(key.clone());
            key
        } else {
            return Err("wallet locked".to_string());
        };

        self.persist_unlocked(&passphrase)?;
        self.touch();
        Ok(key)
    }

    pub fn keys(&mut self) -> Result<Vec<WalletKey>, String> {
        self.ensure_unlocked()?;
        let keys = self
            .state
            .unlocked
            .as_ref()
            .map(|w| w.keys.clone())
            .unwrap_or_default();
        self.touch();
        Ok(keys)
    }

    pub fn export_wif(&mut self, address: &str) -> Result<String, String> {
        self.ensure_unlocked()?;
        let key = self
            .state
            .unlocked
            .as_ref()
            .and_then(|w| w.keys.iter().find(|k| k.address == address.trim()))
            .ok_or_else(|| "address not found in wallet".to_string())?;
        let priv_bytes = hex::decode(&key.privkey).map_err(|_| "invalid wallet key".to_string())?;
        if priv_bytes.len() != 32 {
            return Err("invalid wallet key length".to_string());
        }
        let mut sec = [0u8; 32];
        sec.copy_from_slice(&priv_bytes);
        self.touch();
        Ok(secret_to_wif(&sec, true))
    }

    pub fn import_wif(&mut self, wif: &str) -> Result<WalletKey, String> {
        self.ensure_unlocked()?;
        let passphrase = self
            .state
            .passphrase
            .clone()
            .ok_or_else(|| "wallet locked".to_string())?;

        let (secret, compressed) = wif_to_secret_and_compression(wif.trim())?;
        let secret_key = SecretKey::from_slice(&secret).map_err(|_| "invalid WIF secret".to_string())?;
        let key = key_from_secret(&secret_key, compressed);

        if let Some(ref mut plain) = self.state.unlocked {
            if plain.keys.iter().any(|k| k.address == key.address) {
                return Err("address already exists in wallet".to_string());
            }
            plain.keys.push(key.clone());
            // Imported-key wallet should not continue deterministic derivation blindly.
            plain.seed_hex = None;
            plain.next_index = plain.keys.len() as u32;
        }

        self.persist_unlocked(&passphrase)?;
        self.touch();
        Ok(key)
    }

    pub fn export_seed(&mut self) -> Result<String, String> {
        self.ensure_unlocked()?;
        let seed = self
            .state
            .unlocked
            .as_ref()
            .and_then(|w| w.seed_hex.clone())
            .ok_or_else(|| "no deterministic seed stored in wallet".to_string())?;
        self.touch();
        Ok(seed)
    }

    pub fn import_seed(&mut self, seed_hex: &str, force: bool) -> Result<WalletKey, String> {
        self.ensure_unlocked()?;
        let passphrase = self
            .state
            .passphrase
            .clone()
            .ok_or_else(|| "wallet locked".to_string())?;

        let clean = normalize_seed_hex(seed_hex)?;
        let secret = derive_secret_from_seed_hex(&clean, 0)?;
        let key = key_from_secret(&secret, true);

        if let Some(ref mut plain) = self.state.unlocked {
            if !force && !plain.keys.is_empty() {
                return Err("wallet already has keys; pass force=true to replace".to_string());
            }
            plain.seed_hex = Some(clean);
            plain.next_index = 1;
            plain.keys.clear();
            plain.keys.push(key.clone());
        }

        self.persist_unlocked(&passphrase)?;
        self.touch();
        Ok(key)
    }

    fn persist_unlocked(&self, passphrase: &str) -> Result<(), String> {
        let plain = self
            .state
            .unlocked
            .as_ref()
            .ok_or_else(|| "wallet locked".to_string())?;
        let file = encrypt_wallet(passphrase, plain)?;
        save_wallet(&self.path, &file)
    }

    fn ensure_unlocked(&mut self) -> Result<(), String> {
        self.maybe_auto_lock();
        if self.state.unlocked.is_none() {
            return Err("wallet locked".to_string());
        }
        Ok(())
    }

    fn touch(&mut self) {
        self.state.last_touch = Some(Instant::now());
    }

    fn maybe_auto_lock(&mut self) {
        let mins = auto_lock_minutes();
        if mins == 0 {
            return;
        }
        if let Some(last) = self.state.last_touch {
            if last.elapsed() >= Duration::from_secs(mins * 60) {
                self.lock();
            }
        }
    }
}

fn auto_lock_minutes() -> u64 {
    env::var("IRIUM_WALLET_AUTO_LOCK_MIN")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_AUTO_LOCK_MIN)
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
    #[allow(deprecated)]
    let nonce_ga = Nonce::from_slice(&nonce);
    let ciphertext = cipher
        .encrypt(&nonce_ga, payload.as_ref())
        .map_err(|e| e.to_string())?;
    Ok(WalletFile {
        version: WALLET_VERSION,
        crypto: WalletCrypto {
            salt: hex::encode(salt),
            nonce: hex::encode(nonce),
            cipher: hex::encode(ciphertext),
        },
    })
}

fn decrypt_wallet(passphrase: &str, file: &WalletFile) -> Result<WalletPlain, String> {
    if file.version != WALLET_VERSION {
        return Err("unsupported wallet version".to_string());
    }
    let salt = hex::decode(&file.crypto.salt).map_err(|e| e.to_string())?;
    let nonce = hex::decode(&file.crypto.nonce).map_err(|e| e.to_string())?;
    let cipher_bytes = hex::decode(&file.crypto.cipher).map_err(|e| e.to_string())?;
    let key = derive_key(passphrase, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    #[allow(deprecated)]
    let payload = cipher
        .decrypt(Nonce::from_slice(&nonce), cipher_bytes.as_ref())
        .map_err(|_| "invalid passphrase".to_string())?;
    serde_json::from_slice(&payload).map_err(|e| e.to_string())
}

fn save_wallet(path: &Path, file: &WalletFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(file).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, perms).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn load_wallet(path: &Path) -> Result<WalletFile, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
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

fn base58check_decode(input: &str) -> Option<Vec<u8>> {
    let data = bs58::decode(input).into_vec().ok()?;
    if data.len() < 5 {
        return None;
    }
    let (payload, check) = data.split_at(data.len() - 4);
    let first = Sha256::digest(payload);
    let second = Sha256::digest(&first);
    if &second[0..4] != check {
        return None;
    }
    Some(payload.to_vec())
}

fn secret_to_wif(secret: &[u8; 32], compressed: bool) -> String {
    let mut payload = Vec::with_capacity(34);
    payload.push(WIF_VERSION);
    payload.extend_from_slice(secret);
    if compressed {
        payload.push(0x01);
    }
    let first = Sha256::digest(&payload);
    let second = Sha256::digest(&first);
    let mut full = payload;
    full.extend_from_slice(&second[0..4]);
    bs58::encode(full).into_string()
}

fn wif_to_secret_and_compression(wif: &str) -> Result<([u8; 32], bool), String> {
    let data = base58check_decode(wif).ok_or_else(|| "invalid WIF".to_string())?;
    if data.len() != 33 && data.len() != 34 {
        return Err("invalid WIF length".to_string());
    }
    if data[0] != WIF_VERSION {
        return Err("unsupported WIF version".to_string());
    }
    let compressed = if data.len() == 34 {
        if data[33] != 0x01 {
            return Err("invalid WIF compression flag".to_string());
        }
        true
    } else {
        false
    };
    let mut sec = [0u8; 32];
    sec.copy_from_slice(&data[1..33]);
    Ok((sec, compressed))
}

fn normalize_seed_hex(seed_hex: &str) -> Result<String, String> {
    let s = seed_hex.trim().to_lowercase();
    if s.len() != 64 {
        return Err("seed must be 64-char hex".to_string());
    }
    let _ = hex::decode(&s).map_err(|_| "seed must be valid hex".to_string())?;
    Ok(s)
}

fn generate_seed_hex() -> String {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    hex::encode(seed)
}

fn derive_secret_from_seed_hex(seed_hex: &str, index: u32) -> Result<SecretKey, String> {
    let seed = hex::decode(seed_hex).map_err(|_| "seed must be valid hex".to_string())?;
    if seed.len() != 32 {
        return Err("seed must be 64-char hex".to_string());
    }
    for attempt in 0u32..=1024 {
        let mut h = Sha256::new();
        h.update(&seed);
        h.update(index.to_be_bytes());
        h.update(attempt.to_be_bytes());
        let digest = h.finalize();
        if let Ok(sec) = SecretKey::from_slice(&digest) {
            return Ok(sec);
        }
    }
    Err("failed to derive valid key from seed".to_string())
}

fn key_from_secret(secret: &SecretKey, compressed: bool) -> WalletKey {
    let public = secret.public_key();
    let pubkey = public.to_encoded_point(compressed);
    let pkh = hash160(pubkey.as_bytes());
    let address = base58_p2pkh_from_hash(&pkh);
    WalletKey {
        address,
        pkh: hex::encode(pkh),
        pubkey: hex::encode(pubkey.as_bytes()),
        privkey: hex::encode(secret.to_bytes()),
    }
}

fn generate_key() -> WalletKey {
    let secret = SecretKey::random(&mut OsRng);
    key_from_secret(&secret, true)
}
