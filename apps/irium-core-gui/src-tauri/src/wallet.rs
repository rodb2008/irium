use serde::{Deserialize, Serialize};

use crate::rpc_client::RpcClient;

#[derive(Serialize)]
struct WalletCreateRequest {
    passphrase: String,
}

#[derive(Deserialize)]
struct WalletCreateResponse {
    address: String,
}

#[derive(Serialize)]
struct WalletUnlockRequest {
    passphrase: String,
}

#[derive(Deserialize)]
struct WalletUnlockResponse {
    addresses: Vec<String>,
    current_address: String,
}

#[derive(Deserialize)]
struct WalletAddressesResponse {
    addresses: Vec<String>,
}

#[derive(Deserialize)]
struct WalletReceiveResponse {
    address: String,
}

#[derive(Deserialize)]
struct WalletLockResponse {
    locked: bool,
}

#[derive(Serialize)]
struct WalletSendRequest {
    to_address: String,
    amount: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    from_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fee_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fee_per_byte: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    coin_select: Option<String>,
}

#[derive(Deserialize)]
struct WalletSendResponse {
    txid: String,
    accepted: bool,
    fee: u64,
    total_input: u64,
    change: u64,
}

#[derive(Deserialize)]
struct BalanceResponse {
    balance: u64,
}

#[derive(Deserialize)]
struct HistoryResponse {
    height: u64,
    txs: Vec<HistoryItem>,
}

#[derive(Deserialize, Clone)]
pub struct HistoryItem {
    pub txid: String,
    pub height: u64,
    pub received: u64,
    pub spent: u64,
    pub net: i64,
    pub is_coinbase: bool,
}

fn format_irm(amount: u64) -> String {
    let whole = amount / 100_000_000;
    let frac = amount % 100_000_000;
    if frac == 0 {
        format!("{}", whole)
    } else {
        format!("{}.{:08}", whole, frac)
    }
}

pub async fn create_wallet(client: &RpcClient, passphrase: &str) -> Result<String, String> {
    let req = WalletCreateRequest {
        passphrase: passphrase.to_string(),
    };
    let resp: WalletCreateResponse = client.post_json("/wallet/create", &req).await?;
    Ok(resp.address)
}

pub async fn unlock_wallet(client: &RpcClient, passphrase: &str) -> Result<(), String> {
    let req = WalletUnlockRequest {
        passphrase: passphrase.to_string(),
    };
    let _resp: WalletUnlockResponse = client.post_json("/wallet/unlock", &req).await?;
    Ok(())
}

pub async fn lock_wallet(client: &RpcClient) -> Result<(), String> {
    let _resp: WalletLockResponse = client.post_json("/wallet/lock", &serde_json::json!({})).await?;
    Ok(())
}

pub async fn new_address(client: &RpcClient) -> Result<String, String> {
    let resp: WalletReceiveResponse = client.post_json("/wallet/new_address", &serde_json::json!({})).await?;
    Ok(resp.address)
}

pub async fn receive_address(client: &RpcClient) -> Result<String, String> {
    let resp: WalletReceiveResponse = client.get_json("/wallet/receive").await?;
    Ok(resp.address)
}

pub async fn list_addresses(client: &RpcClient) -> Result<Vec<String>, String> {
    let resp: WalletAddressesResponse = client.get_json("/wallet/addresses").await?;
    Ok(resp.addresses)
}

pub async fn balance(client: &RpcClient) -> Result<(String, String), String> {
    let addresses = list_addresses(client).await?;
    let mut total = 0u64;
    for addr in addresses {
        let payload: BalanceResponse = client
            .get_json(&format!("/rpc/balance?address={}", addr))
            .await?;
        total = total.saturating_add(payload.balance);
    }
    Ok((format_irm(total), "0.0".to_string()))
}

pub async fn history(client: &RpcClient, limit: usize) -> Result<Vec<HistoryItem>, String> {
    let addresses = list_addresses(client).await?;
    let mut all: Vec<HistoryItem> = Vec::new();
    for addr in addresses {
        let payload: HistoryResponse = client
            .get_json(&format!("/rpc/history?address={}", addr))
            .await?;
        all.extend(payload.txs.into_iter());
    }
    all.sort_by(|a, b| b.height.cmp(&a.height));
    all.truncate(limit);
    Ok(all)
}

pub async fn send(
    client: &RpcClient,
    to_addr: &str,
    amount: &str,
    fee_mode: &str,
) -> Result<String, String> {
    let fee_mode = fee_mode.trim();
    let mode = match fee_mode {
        "" | "auto" => None,
        _ => Some(fee_mode.to_string()),
    };
    let req = WalletSendRequest {
        to_address: to_addr.to_string(),
        amount: amount.to_string(),
        from_address: None,
        fee_mode: mode,
        fee_per_byte: None,
        coin_select: None,
    };
    let resp: WalletSendResponse = client.post_json("/wallet/send", &req).await?;
    if !resp.accepted {
        return Err("Transaction rejected by node".to_string());
    }
    Ok(resp.txid)
}
