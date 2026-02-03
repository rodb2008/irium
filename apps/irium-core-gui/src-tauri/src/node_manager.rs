use crate::settings::{ensure_data_dir, Settings};
use reqwest::Url;
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

#[derive(Default)]
pub struct NodeManager {
    child: Option<Child>,
}

impl NodeManager {
    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    pub fn start(&mut self, settings: &Settings) -> Result<(), String> {
        if self.child.is_some() {
            return Ok(());
        }
        let data_dir = PathBuf::from(&settings.data_dir);
        ensure_data_dir(&data_dir)?;

        let url = Url::parse(&settings.rpc_url).map_err(|e| e.to_string())?;
        let host = url.host_str().unwrap_or("127.0.0.1");
        let port = url.port().unwrap_or(38300);

        let log_path = PathBuf::from(&settings.log_file);
        if let Some(parent) = log_path.parent() {
            ensure_data_dir(parent)?;
        }
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| format!("open log file: {e}"))?;
        let log_file_err = log_file.try_clone().map_err(|e| e.to_string())?;

        let mut cmd = Command::new(&settings.node_bin);
        cmd.current_dir(&data_dir)
            .env("HOME", &settings.data_dir)
            .env("IRIUM_NODE_HOST", host)
            .env("IRIUM_NODE_PORT", port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_file_err));

        if let Some(token) = &settings.rpc_token {
            cmd.env("IRIUM_RPC_TOKEN", token);
        }
        if let Some(cfg) = &settings.node_config {
            cmd.env("IRIUM_NODE_CONFIG", cfg);
        }

        let child = cmd.spawn().map_err(|e| format!("start node: {e}"))?;
        self.child = Some(child);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }
}
