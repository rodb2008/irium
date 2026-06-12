
use anyhow::{Context, Result};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_host: String,
    pub bind_port: u16,
    /// Requests per second per IP before 429
    pub rate_limit_rps: u32,
    /// IPs exempt from rate limiting (localhost, loopback)
    pub trusted_ips: HashSet<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let database_url = std::env::var("DATABASE_URL")
            .context("DATABASE_URL must be set")?;
        let bind_host = std::env::var("API_HOST")
            .unwrap_or_else(|_| "127.0.0.1".to_string());
        let bind_port: u16 = std::env::var("API_PORT")
            .unwrap_or_else(|_| "3400".to_string())
            .parse()
            .context("API_PORT must be a valid port")?;
        let rate_limit_rps: u32 = std::env::var("API_RATE_LIMIT_RPS")
            .unwrap_or_else(|_| "60".to_string())
            .parse()
            .context("API_RATE_LIMIT_RPS must be a positive integer")?;
        let trusted_ips = {
            let mut s = HashSet::new();
            s.insert("127.0.0.1".to_string());
            s.insert("::1".to_string());
            // Additional trusted IPs from env, comma-separated
            if let Ok(extra) = std::env::var("API_TRUSTED_IPS") {
                for ip in extra.split(',') {
                    let ip = ip.trim();
                    if !ip.is_empty() { s.insert(ip.to_string()); }
                }
            }
            s
        };
        Ok(Self { database_url, bind_host, bind_port, rate_limit_rps, trusted_ips })
    }
}
