use std::env;

#[derive(Clone)]
pub struct Config {
    pub port: u16,
    pub iriumd_rpc: String,
    pub iriumd_token: String,
    pub explorer_url: String,
    pub stratum_asic: String,
    pub stratum_cpu: String,
    pub stratum_solo: String,
    pub stratum_443: String,
    pub db_path: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            port: env::var("POOL_API_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3339),
            iriumd_rpc: env::var("IRIUMD_RPC")
                .unwrap_or_else(|_| "http://127.0.0.1:38300".into()),
            iriumd_token: env::var("IRIUM_RPC_TOKEN").unwrap_or_default(),
            explorer_url: env::var("EXPLORER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:38310".into()),
            stratum_asic: env::var("STRATUM_ASIC_METRICS")
                .unwrap_or_else(|_| "http://127.0.0.1:3334/metrics".into()),
            stratum_cpu: env::var("STRATUM_CPU_METRICS")
                .unwrap_or_else(|_| "http://127.0.0.1:3346/metrics".into()),
            stratum_solo: env::var("STRATUM_SOLO_METRICS")
                .unwrap_or_else(|_| "http://127.0.0.1:3338/metrics".into()),
            stratum_443: env::var("STRATUM_443_METRICS")
                .unwrap_or_else(|_| "http://127.0.0.1:3444/metrics".into()),
            db_path: env::var("DB_PATH")
                .unwrap_or_else(|_| "/home/irium/irium-pool-api.db".into()),
        }
    }
}
