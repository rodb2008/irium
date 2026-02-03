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
        if passphrase.trim().is_empty() {
            return Err("passphrase required".to_string());
        }
        if self.path.exists() {
            return Err("wallet already exists".to_string());
        }
        let mut plain = WalletPlain { keys: Vec::new() };
        let key = generate_key();
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
        let plain = decrypt_wallet(passphrase, &file)?;
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
        let key = generate_key();
        if let Some(ref mut plain) = self.state.unlocked {
            plain.keys.push(key.clone());
            let file = encrypt_wallet(&passphrase, plain)?;
            save_wallet(&self.path, &file)?;
            self.touch();
            return Ok(key);
        }
        Err("wallet locked".to_string())
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
