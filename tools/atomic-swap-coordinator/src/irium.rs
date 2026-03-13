use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::{json, Value};

#[derive(Clone)]
pub struct IriumClient {
    pub rpc_url: Option<String>,
    pub rpc_token: Option<String>,
    pub recipient_address: Option<String>,
    pub refund_address: Option<String>,
    pub amount_irm: String,
    pub timeout_blocks: u64,
}

#[derive(Clone, Debug)]
pub struct IriumHtlcRef {
    pub txid: String,
    pub vout: u32,
}

impl IriumClient {
    pub fn disabled() -> Self {
        Self {
            rpc_url: None,
            rpc_token: None,
            recipient_address: None,
            refund_address: None,
            amount_irm: "1.0".to_string(),
            timeout_blocks: 120,
        }
    }

    fn auth_req(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(t) = self.rpc_token.clone() {
            req.bearer_auth(t)
        } else {
            req
        }
    }

    async fn chain_height(&self) -> Result<u64> {
        let base = self
            .rpc_url
            .clone()
            .ok_or_else(|| anyhow!("irium_rpc_disabled"))?;
        let url = format!("{}/status", base.trim_end_matches('/'));
        let cli = Client::builder().build()?;
        let r = self.auth_req(cli.get(url)).send().await?;
        if !r.status().is_success() {
            return Ok(0);
        }
        let v: Value = r.json().await?;
        Ok(v["height"].as_u64().unwrap_or(0))
    }

    pub async fn tx_exists(&self, txid: &str) -> Result<bool> {
        if self.rpc_url.is_none() {
            return Ok(false);
        }
        let base = self.rpc_url.clone().unwrap_or_default();
        let url = format!("{}/rpc/tx?txid={}", base.trim_end_matches('/'), txid);
        let cli = Client::builder().build()?;
        let r = self.auth_req(cli.get(url)).send().await?;
        Ok(r.status().is_success())
    }

    pub async fn htlc_spent(&self, txid: &str, vout: u32) -> Result<bool> {
        if self.rpc_url.is_none() {
            return Ok(false);
        }
        let base = self.rpc_url.clone().unwrap_or_default();
        let url = format!(
            "{}/rpc/inspecthtlc?txid={}&vout={}",
            base.trim_end_matches('/'),
            txid,
            vout
        );
        let cli = Client::builder().build()?;
        let r = self.auth_req(cli.get(url)).send().await?;
        if !r.status().is_success() {
            return Ok(false);
        }
        let v: Value = r.json().await?;
        Ok(v["spent"].as_bool().unwrap_or(false))
    }

    pub async fn create_htlc(&self, secret_hash_hex: &str) -> Result<Option<IriumHtlcRef>> {
        if self.rpc_url.is_none() {
            return Ok(None);
        }

        let recipient = self
            .recipient_address
            .clone()
            .ok_or_else(|| anyhow!("missing_irium_recipient"))?;
        let refund = self
            .refund_address
            .clone()
            .ok_or_else(|| anyhow!("missing_irium_refund"))?;

        let base = self.rpc_url.clone().unwrap_or_default();
        let url = format!("{}/rpc/createhtlc", base.trim_end_matches('/'));
        let timeout_height = self.chain_height().await.unwrap_or(0) + self.timeout_blocks.max(6);
        let body = json!({
            "amount": self.amount_irm,
            "recipient_address": recipient,
            "refund_address": refund,
            "secret_hash_hex": secret_hash_hex,
            "timeout_height": timeout_height,
            "broadcast": true
        });
        let cli = Client::builder().build()?;
        let req = self.auth_req(cli.post(url).json(&body));
        let r = req.send().await?;
        let status = r.status();
        if !status.is_success() {
            let txt = r.text().await.unwrap_or_default();
            return Err(anyhow!("irium_createhtlc_http_{}:{}", status, txt));
        }
        let v: Value = r.json().await?;
        let txid = v["txid"]
            .as_str()
            .ok_or_else(|| anyhow!("irium_createhtlc_missing_txid"))?
            .to_string();
        let vout = v["htlc_vout"].as_u64().unwrap_or(0) as u32;
        Ok(Some(IriumHtlcRef { txid, vout }))
    }
}
