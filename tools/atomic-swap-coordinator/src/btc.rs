use anyhow::{anyhow, Result};
use reqwest::{Client, Url};
use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr};

#[derive(Clone, Debug)]
pub struct BtcClient {
    pub rpc_url: Option<String>,
    pub rpc_user: Option<String>,
    pub rpc_pass: Option<String>,
    pub min_confirmations: u32,
}

fn allow_remote_rpc() -> bool {
    std::env::var("COORDINATOR_ALLOW_REMOTE_RPC")
        .ok()
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn is_allowed_rpc_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4 == Ipv4Addr::new(0, 0, 0, 0)
        }
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local() || v6.is_unspecified(),
    }
}

fn sanitize_rpc_url(raw: &str) -> Result<String> {
    let url = Url::parse(raw).map_err(|_| anyhow!("btc_rpc_invalid_url"))?;
    match url.scheme() {
        "http" | "https" => {}
        _ => return Err(anyhow!("btc_rpc_invalid_scheme")),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(anyhow!("btc_rpc_embedded_credentials_forbidden"));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(anyhow!("btc_rpc_invalid_url_components"));
    }
    if !allow_remote_rpc() {
        let host = url
            .host_str()
            .ok_or_else(|| anyhow!("btc_rpc_missing_host"))?;
        if host != "localhost" {
            let ip = host
                .parse::<IpAddr>()
                .map_err(|_| anyhow!("btc_rpc_host_must_be_local_or_private"))?;
            if !is_allowed_rpc_ip(ip) {
                return Err(anyhow!("btc_rpc_host_must_be_local_or_private"));
            }
        }
    }
    Ok(url.to_string())
}

impl BtcClient {
    pub fn disabled(min_confirmations: u32) -> Self {
        Self {
            rpc_url: None,
            rpc_user: None,
            rpc_pass: None,
            min_confirmations,
        }
    }

    pub fn enabled(
        rpc_url: String,
        rpc_user: Option<String>,
        rpc_pass: Option<String>,
        min_confirmations: u32,
    ) -> Result<Self> {
        Ok(Self {
            rpc_url: Some(sanitize_rpc_url(&rpc_url)?),
            rpc_user,
            rpc_pass,
            min_confirmations,
        })
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let url = self
            .rpc_url
            .clone()
            .ok_or_else(|| anyhow!("btc_rpc_disabled"))?;
        let body = json!({"jsonrpc":"1.0","id":"coord","method":method,"params":params});
        let cli = Client::builder().build()?;
        let mut req = cli.post(url).json(&body);
        if let (Some(u), Some(p)) = (self.rpc_user.clone(), self.rpc_pass.clone()) {
            req = req.basic_auth(u, Some(p));
        }
        let r = req.send().await?;
        if !r.status().is_success() {
            return Err(anyhow!("btc_rpc_http_{}", r.status()));
        }
        let v: Value = r.json().await?;
        if !v["error"].is_null() {
            return Err(anyhow!("btc_rpc_error:{}", v["error"]));
        }
        Ok(v["result"].clone())
    }

    pub async fn get_new_address(&self, label: &str) -> Result<String> {
        if self.rpc_url.is_none() {
            return Ok(format!("btc-testnet-address-pending-{label}"));
        }
        let v = self.call("getnewaddress", json!([label, "bech32"])).await?;
        v.as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("btc_rpc_invalid_getnewaddress"))
    }

    pub async fn tx_confirmations(&self, txid: &str) -> Result<u32> {
        if self.rpc_url.is_none() {
            return Ok(0);
        }
        let v = self.call("getrawtransaction", json!([txid, true])).await?;
        Ok(v["confirmations"].as_u64().unwrap_or(0) as u32)
    }

    pub async fn validate_funding_tx(
        &self,
        txid: &str,
        expected_address: &str,
        min_amount_sats: u64,
    ) -> Result<bool> {
        if self.rpc_url.is_none() {
            return Ok(!txid.is_empty());
        }
        let v = self.call("getrawtransaction", json!([txid, true])).await?;
        let outs = v["vout"]
            .as_array()
            .ok_or_else(|| anyhow!("btc_rpc_vout_missing"))?;
        for o in outs {
            let sats = (o["value"].as_f64().unwrap_or(0.0) * 100_000_000.0).round() as u64;
            if sats < min_amount_sats {
                continue;
            }
            let addrs = o["scriptPubKey"]["address"]
                .as_str()
                .map(|s| vec![s.to_string()])
                .or_else(|| {
                    o["scriptPubKey"]["addresses"].as_array().map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                });
            if let Some(addrs) = addrs {
                if addrs.iter().any(|a| a == expected_address) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    pub async fn autodetect_funding_txid(
        &self,
        address: &str,
        min_amount_sats: u64,
    ) -> Result<Option<String>> {
        if self.rpc_url.is_none() {
            return Ok(None);
        }
        let v = self
            .call("listunspent", json!([0, 9999999, [address]]))
            .await?;
        let arr = v
            .as_array()
            .ok_or_else(|| anyhow!("btc_rpc_listunspent_invalid"))?;
        for u in arr {
            let amount_sats = (u["amount"].as_f64().unwrap_or(0.0) * 100_000_000.0).round() as u64;
            if amount_sats >= min_amount_sats {
                if let Some(txid) = u["txid"].as_str() {
                    return Ok(Some(txid.to_string()));
                }
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn btc_rpc_rejects_public_hostname_by_default() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("COORDINATOR_ALLOW_REMOTE_RPC");
        let err = BtcClient::enabled("https://example.com".to_string(), None, None, 1).unwrap_err();
        assert!(err.to_string().contains("local_or_private") || err.to_string().contains("host_must_be"));
    }

    #[test]
    fn btc_rpc_allows_loopback_url() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("COORDINATOR_ALLOW_REMOTE_RPC");
        let cli = BtcClient::enabled("http://127.0.0.1:8332".to_string(), None, None, 1).unwrap();
        assert_eq!(cli.rpc_url.as_deref(), Some("http://127.0.0.1:8332/"));
    }

    #[test]
    fn btc_rpc_can_allow_remote_when_explicitly_enabled() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("COORDINATOR_ALLOW_REMOTE_RPC", "1");
        let cli = BtcClient::enabled("https://example.com".to_string(), None, None, 1).unwrap();
        assert_eq!(cli.rpc_url.as_deref(), Some("https://example.com/"));
        std::env::remove_var("COORDINATOR_ALLOW_REMOTE_RPC");
    }
}
