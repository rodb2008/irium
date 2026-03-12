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

    pub async fn create_htlc(&self, secret_hash_hex: &str) -> Result<Option<String>> {
        if self.rpc_url.is_none() {
            return Ok(None);
        }
        let url = format!(
            "{}/rpc/createhtlc",
            self.rpc_url.clone().unwrap().trim_end_matches('/')
        );
        let body = json!({
            "amount": self.amount_irm,
            "recipient_address": self.recipient_address.clone().ok_or_else(|| anyhow!("missing_irium_recipient"))?,
            "refund_address": self.refund_address.clone().ok_or_else(|| anyhow!("missing_irium_refund"))?,
            "secret_hash_hex": secret_hash_hex,
            "timeout_height": self.timeout_blocks,
            "broadcast": true
        });
        let cli = Client::builder().build()?;
        let mut req = cli.post(url).json(&body);
        if let Some(t) = self.rpc_token.clone() {
            req = req.bearer_auth(t);
        }
        let r = req.send().await?;
        let status = r.status();
        if !status.is_success() {
            let txt = r.text().await.unwrap_or_default();
            return Err(anyhow!("irium_createhtlc_http_{}:{}", status, txt));
        }
        let v: Value = r.json().await?;
        Ok(v["txid"].as_str().map(|s| s.to_string()))
    }
}
