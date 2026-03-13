use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::{json, Value};

#[derive(Clone)]
pub struct BtcClient {
    pub rpc_url: Option<String>,
    pub rpc_user: Option<String>,
    pub rpc_pass: Option<String>,
    pub min_confirmations: u32,
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
    ) -> Self {
        Self {
            rpc_url: Some(rpc_url),
            rpc_user,
            rpc_pass,
            min_confirmations,
        }
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
