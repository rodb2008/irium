use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub mode: String,
    pub data_dir: String,
    pub rpc_url: String,
    pub rpc_token: Option<String>,
    pub rpc_ca: Option<String>,
    pub rpc_allow_insecure: bool,
    pub node_bin: String,
    pub node_config: Option<String>,
    pub log_file: String,
    pub auto_lock_minutes: u64,
}

fn project_dir() -> PathBuf {
    if let Some(dirs) = directories::ProjectDirs::from("org", "Irium", "IriumCore") {
        return dirs.data_dir().to_path_buf();
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    PathBuf::from(home).join(".iriumcore")
}

pub fn settings_path() -> PathBuf {
    let dir = project_dir();
    if let Err(_) = fs::create_dir_all(&dir) {}
    dir.join("settings.json")
}

fn default_token() -> String {
    let mut buf = [0u8; 24];
    OsRng.fill_bytes(&mut buf);
    hex::encode(buf)
}

pub fn default_settings() -> Settings {
    let data_dir = project_dir();
    let log_file = data_dir.join("iriumd.log");
    Settings {
        mode: "managed".to_string(),
        data_dir: data_dir.to_string_lossy().to_string(),
        rpc_url: "http://127.0.0.1:38300".to_string(),
        rpc_token: Some(default_token()),
        rpc_ca: None,
        rpc_allow_insecure: false,
        node_bin: "iriumd".to_string(),
        node_config: Some("configs/node.json".to_string()),
        log_file: log_file.to_string_lossy().to_string(),
        auto_lock_minutes: 5,
    }
}

pub fn load_settings() -> Settings {
    let path = settings_path();
    if let Ok(raw) = fs::read_to_string(&path) {
        if let Ok(mut settings) = serde_json::from_str::<Settings>(&raw) {
            if settings.rpc_token.is_none() {
                settings.rpc_token = Some(default_token());
            }
            return settings;
        }
    }
    let settings = default_settings();
    let _ = save_settings(&settings);
    settings
}

pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| e.to_string())
}

pub fn ensure_data_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| format!("create data dir: {e}"))
}
