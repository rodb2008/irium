//! DOGE public-API + regtest fetch logic for the in-process header
//! sync task. Mirrors `src/bin/doge-header-sync.rs` but async.

use std::env;
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};

use super::common::{
    Source, PER_HEADER_RETRIES, POLITE_SLEEP_MS, RETRY_SLEEP_MS,
};

const DEFAULT_DOGE_RPC_URL: &str = "http://127.0.0.1:19543";
const DEFAULT_DOGE_RPC_USER: &str = "iriumtest";
const DEFAULT_DOGE_RPC_PASSWORD: &str = "iriumtest";

const MAINNET_APIS: &[&str] = &["https://dogecoinspace.org/api"];

pub async fn fetch_doge_net_tip(client: &Client, source: Source) -> Result<u64, String> {
    match source {
        Source::Regtest => regtest_get_block_count(client).await,
        Source::Mainnet => mainnet_get_tip_height_with_fallbacks(client).await,
    }
}

pub async fn fetch_doge_headers(
    client: &Client,
    source: Source,
    start: u64,
    end: u64,
) -> Result<String, String> {
    match source {
        Source::Regtest => regtest_fetch_headers(client, start, end).await,
        Source::Mainnet => {
            // v1.9.65: try the existing mempool.space-shape path first
            // (zero behavior change when dogecoinspace.org is up); fall
            // back to Blockcypher which assembles the 80-byte header
            // from parsed JSON fields when the primary is down.
            match mainnet_fetch_headers(client, start, end).await {
                Ok(s) => Ok(s),
                Err(primary_err) => {
                    match blockcypher_fetch_headers(client, start, end).await {
                        Ok(s) => Ok(s),
                        Err(secondary_err) => Err(format!(
                            "all DOGE header-fetch APIs failed;                              dogecoinspace.org: {primary_err};                              blockcypher.com: {secondary_err}"
                        )),
                    }
                }
            }
        }
    }
}

/// v1.9.65: tip-probe with 5-API fallback chain. Each API has its own
/// response shape so we cannot reuse `MAINNET_APIS` (which assumes the
/// mempool.space `/blocks/tip/height` plain-text contract). Returns the
/// first non-error result, or a combined error string listing every
/// attempt if all fail.
async fn mainnet_get_tip_height_with_fallbacks(client: &Client) -> Result<u64, String> {
    let mut errors: Vec<String> = Vec::new();

    // 1. dogecoinspace.org (existing) — plain-text response.
    match mainnet_get_tip_height(client).await {
        Ok(h) => return Ok(h),
        Err(e) => errors.push(format!("dogecoinspace.org: {e}")),
    }

    // 2. Blockchair — JSON: {"data": {"blocks": <next_block_number>}}.
    // blocks = tip_height + 1, so subtract 1.
    match probe_blockchair_tip(client).await {
        Ok(h) => return Ok(h),
        Err(e) => errors.push(format!("blockchair.com: {e}")),
    }

    // 3. Blockcypher — JSON: {"height": N, ...}.
    match probe_blockcypher_tip(client).await {
        Ok(h) => return Ok(h),
        Err(e) => errors.push(format!("blockcypher.com: {e}")),
    }

    // 4. chainz.cryptoid.info — plain-text integer.
    match probe_cryptoid_tip(client).await {
        Ok(h) => return Ok(h),
        Err(e) => errors.push(format!("cryptoid.info: {e}")),
    }

    // 5. dogechain.info — JSON: {"block": {"height": N, ...}}.
    match probe_dogechain_tip(client).await {
        Ok(h) => return Ok(h),
        Err(e) => errors.push(format!("dogechain.info: {e}")),
    }

    Err(format!(
        "all DOGE tip-probe APIs failed; attempts: [{}]",
        errors.join("; ")
    ))
}

async fn probe_blockchair_tip(client: &Client) -> Result<u64, String> {
    #[derive(Deserialize)]
    struct R {
        data: D,
    }
    #[derive(Deserialize)]
    struct D {
        blocks: u64,
    }
    let resp = client
        .get("https://api.blockchair.com/dogecoin/stats")
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let r: R = resp
        .json()
        .await
        .map_err(|e| format!("json parse: {e}"))?;
    // blockchair `blocks` is the height of the NEXT block to mine, so
    // the current tip is one less.
    Ok(r.data.blocks.saturating_sub(1))
}

async fn probe_blockcypher_tip(client: &Client) -> Result<u64, String> {
    #[derive(Deserialize)]
    struct R {
        height: u64,
    }
    let resp = client
        .get("https://api.blockcypher.com/v1/doge/main")
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let r: R = resp
        .json()
        .await
        .map_err(|e| format!("json parse: {e}"))?;
    Ok(r.height)
}

async fn probe_cryptoid_tip(client: &Client) -> Result<u64, String> {
    let resp = client
        .get("https://chainz.cryptoid.info/doge/api.dsp?q=getblockcount")
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let body = resp
        .text()
        .await
        .map_err(|e| format!("body read: {e}"))?;
    body.trim()
        .parse::<u64>()
        .map_err(|e| format!("parse u64: {e} (body={body:?})"))
}

async fn probe_dogechain_tip(client: &Client) -> Result<u64, String> {
    #[derive(Deserialize)]
    struct R {
        block: B,
    }
    #[derive(Deserialize)]
    struct B {
        height: u64,
    }
    let resp = client
        .get("https://dogechain.info/api/v1/block/latest")
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let r: R = resp
        .json()
        .await
        .map_err(|e| format!("json parse: {e}"))?;
    Ok(r.block.height)
}

/// v1.9.65: Blockcypher fallback header fetch. Blockcypher returns
/// fully-parsed block JSON; we assemble the canonical 80-byte header on
/// the client side. Hashes (`prev_block`, `mrkl_root`) come back in
/// display order, so reverse the bytes to natural-byte-order before
/// assembling. `bits` is an integer in their JSON; encoded LE on wire.
async fn blockcypher_fetch_headers(
    client: &Client,
    start: u64,
    end: u64,
) -> Result<String, String> {
    let mut hex_out = String::with_capacity(((end - start + 1) * 160) as usize);
    for height in start..=end {
        let header = blockcypher_fetch_one_header(client, height).await?;
        hex_out.push_str(&header);
        tokio::time::sleep(Duration::from_millis(POLITE_SLEEP_MS)).await;
    }
    Ok(hex_out)
}

#[derive(Deserialize)]
struct BcBlock {
    ver: u32,
    prev_block: String,
    mrkl_root: String,
    time: String,
    bits: u32,
    nonce: u32,
}

async fn blockcypher_fetch_one_header(client: &Client, height: u64) -> Result<String, String> {
    let url = format!("https://api.blockcypher.com/v1/doge/main/blocks/{height}");
    let mut last_err = String::new();
    for attempt in 0..PER_HEADER_RETRIES {
        let result: Result<String, String> = async {
            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}", resp.status()));
            }
            let b: BcBlock = resp
                .json()
                .await
                .map_err(|e| format!("json parse: {e}"))?;
            // Parse the RFC3339 timestamp into a unix timestamp. Blockcypher
            // sends e.g. "2026-06-04T05:31:12Z" — we want a u32 seconds value.
            let time_unix = chrono::DateTime::parse_from_rfc3339(b.time.trim())
                .map_err(|e| format!("time parse: {e}"))?
                .timestamp() as u32;
            // Reverse display-order 32-byte hashes to natural byte order.
            let prev_bytes = hex::decode(b.prev_block.trim())
                .map_err(|e| format!("prev_block hex: {e}"))?;
            if prev_bytes.len() != 32 {
                return Err(format!("prev_block wrong length {}", prev_bytes.len()));
            }
            let mut prev_natural = prev_bytes;
            prev_natural.reverse();
            let mrkl_bytes = hex::decode(b.mrkl_root.trim())
                .map_err(|e| format!("mrkl_root hex: {e}"))?;
            if mrkl_bytes.len() != 32 {
                return Err(format!("mrkl_root wrong length {}", mrkl_bytes.len()));
            }
            let mut mrkl_natural = mrkl_bytes;
            mrkl_natural.reverse();
            // Assemble the 80-byte wire-format header.
            let mut header = Vec::with_capacity(80);
            header.extend_from_slice(&b.ver.to_le_bytes());
            header.extend_from_slice(&prev_natural);
            header.extend_from_slice(&mrkl_natural);
            header.extend_from_slice(&time_unix.to_le_bytes());
            header.extend_from_slice(&b.bits.to_le_bytes());
            header.extend_from_slice(&b.nonce.to_le_bytes());
            if header.len() != 80 {
                return Err(format!("header length {} (expected 80)", header.len()));
            }
            Ok(hex::encode(header))
        }
        .await;
        match result {
            Ok(v) => return Ok(v),
            Err(e) => {
                last_err = e;
                if attempt + 1 < PER_HEADER_RETRIES {
                    tokio::time::sleep(Duration::from_millis(RETRY_SLEEP_MS)).await;
                }
            }
        }
    }
    Err(format!(
        "blockcypher h={height} exhausted {PER_HEADER_RETRIES} attempts; last error: {last_err}"
    ))
}

// -------- Regtest source (dogecoind JSON-RPC) --------

#[derive(Deserialize)]
struct DogecoindRpcResp<T> {
    result: Option<T>,
    error: Option<Value>,
}

async fn dogecoind_rpc<T: for<'de> serde::Deserialize<'de>>(
    client: &Client,
    method: &str,
    params: Value,
) -> Result<T, String> {
    let url = env::var("DOGE_RPC_URL").unwrap_or_else(|_| DEFAULT_DOGE_RPC_URL.to_string());
    let user =
        env::var("DOGE_RPC_USER").unwrap_or_else(|_| DEFAULT_DOGE_RPC_USER.to_string());
    let password =
        env::var("DOGE_RPC_PASSWORD").unwrap_or_else(|_| DEFAULT_DOGE_RPC_PASSWORD.to_string());

    let body = json!({
        "jsonrpc": "1.0",
        "id": "iriumd-doge-header-sync",
        "method": method,
        "params": params,
    });
    let resp = client
        .post(&url)
        .basic_auth(user, Some(password))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("dogecoind {method} request failed: {e}"))?;
    let status = resp.status();
    let raw = resp
        .text()
        .await
        .map_err(|e| format!("dogecoind {method} body read failed: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "dogecoind {method} returned HTTP {status}; body: {raw}"
        ));
    }
    let parsed: DogecoindRpcResp<T> = serde_json::from_str(&raw)
        .map_err(|e| format!("dogecoind {method} decode failed: {e}; raw: {raw}"))?;
    if let Some(err) = parsed.error {
        if !err.is_null() {
            return Err(format!("dogecoind {method} returned error: {err}"));
        }
    }
    parsed
        .result
        .ok_or_else(|| format!("dogecoind {method} returned no result"))
}

async fn regtest_get_block_count(client: &Client) -> Result<u64, String> {
    dogecoind_rpc::<u64>(client, "getblockcount", json!([])).await
}

async fn regtest_fetch_headers(
    client: &Client,
    start: u64,
    end: u64,
) -> Result<String, String> {
    let mut hex_out = String::with_capacity(((end - start + 1) * 160) as usize);
    for height in start..=end {
        let hash =
            dogecoind_rpc::<String>(client, "getblockhash", json!([height])).await?;
        let header_hex =
            dogecoind_rpc::<String>(client, "getblockheader", json!([hash, false])).await?;
        if header_hex.len() != 160 {
            return Err(format!(
                "dogecoind getblockheader at h={height} returned hex length {} (expected 160)",
                header_hex.len()
            ));
        }
        hex_out.push_str(&header_hex);
    }
    Ok(hex_out)
}

// -------- Mainnet source (public DOGE API) --------

async fn mainnet_get_tip_height(client: &Client) -> Result<u64, String> {
    let mut last_err = String::new();
    for api in MAINNET_APIS {
        let url = format!("{api}/blocks/tip/height");
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.text().await {
                Ok(t) => match t.trim().parse::<u64>() {
                    Ok(h) => return Ok(h),
                    Err(e) => last_err = format!("{api} returned non-numeric tip: {e}"),
                },
                Err(e) => last_err = format!("{api} body read failed: {e}"),
            },
            Ok(resp) => last_err = format!("{api} returned HTTP {}", resp.status()),
            Err(e) => last_err = format!("{api} request failed: {e}"),
        }
    }
    Err(format!(
        "all public DOGE APIs failed for tip-height probe; last error: {last_err}"
    ))
}

async fn mainnet_fetch_headers(
    client: &Client,
    start: u64,
    end: u64,
) -> Result<String, String> {
    let api = mainnet_pick_api(client).await?;
    let mut hex_out = String::with_capacity(((end - start + 1) * 160) as usize);
    for height in start..=end {
        let hash = mainnet_fetch_height_to_hash(client, api, height).await?;
        let header_hex = mainnet_fetch_block_header(client, api, &hash).await?;
        if header_hex.len() != 160 {
            return Err(format!(
                "header for height {height} has hex length {} (expected 160)",
                header_hex.len()
            ));
        }
        hex_out.push_str(&header_hex);
        tokio::time::sleep(Duration::from_millis(POLITE_SLEEP_MS)).await;
    }
    Ok(hex_out)
}

async fn mainnet_pick_api(client: &Client) -> Result<&'static str, String> {
    let mut last_err = String::new();
    for api in MAINNET_APIS {
        let url = format!("{api}/blocks/tip/height");
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(api),
            Ok(resp) => last_err = format!("{api} returned HTTP {}", resp.status()),
            Err(e) => last_err = format!("{api} request failed: {e}"),
        }
    }
    Err(format!(
        "no public DOGE API responding for fetch loop; last error: {last_err}"
    ))
}

async fn mainnet_fetch_height_to_hash(
    client: &Client,
    api: &str,
    height: u64,
) -> Result<String, String> {
    let url = format!("{api}/block-height/{height}");
    let mut last_err = String::new();
    for attempt in 0..PER_HEADER_RETRIES {
        let result: Result<String, String> = async {
            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("block-height {height} request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!(
                    "block-height {height} returned HTTP {}",
                    resp.status()
                ));
            }
            let h = resp
                .text()
                .await
                .map_err(|e| format!("block-height {height} body read failed: {e}"))?;
            let trimmed = h.trim();
            if trimmed.len() != 64 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!(
                    "block-height {height} returned malformed hash: {trimmed:?}"
                ));
            }
            Ok(trimmed.to_string())
        }
        .await;
        match result {
            Ok(v) => return Ok(v),
            Err(e) => {
                last_err = e;
                if attempt + 1 < PER_HEADER_RETRIES {
                    tokio::time::sleep(Duration::from_millis(RETRY_SLEEP_MS)).await;
                }
            }
        }
    }
    Err(format!(
        "exhausted {PER_HEADER_RETRIES} attempts; last error: {last_err}"
    ))
}

async fn mainnet_fetch_block_header(
    client: &Client,
    api: &str,
    hash: &str,
) -> Result<String, String> {
    let url = format!("{api}/block/{hash}/header");
    let mut last_err = String::new();
    for attempt in 0..PER_HEADER_RETRIES {
        let result: Result<String, String> = async {
            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("block/{hash}/header request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!(
                    "block/{hash}/header returned HTTP {}",
                    resp.status()
                ));
            }
            let h = resp
                .text()
                .await
                .map_err(|e| format!("block/{hash}/header body read failed: {e}"))?;
            Ok(h.trim().to_string())
        }
        .await;
        match result {
            Ok(v) => return Ok(v),
            Err(e) => {
                last_err = e;
                if attempt + 1 < PER_HEADER_RETRIES {
                    tokio::time::sleep(Duration::from_millis(RETRY_SLEEP_MS)).await;
                }
            }
        }
    }
    Err(format!(
        "exhausted {PER_HEADER_RETRIES} attempts; last error: {last_err}"
    ))
}
