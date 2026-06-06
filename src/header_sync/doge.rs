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

// ====================================================================
// Issue #68 Option B (v1.9.83): AuxPoW data fetch via blockchair HTTP.
//
// PR-6's P2P `getdata MSG_BLOCK` path didn't work against real DOGE
// peers (peers ack our getdata then timeout instead of sending the
// block — verified-correct via a Python probe). We pivot to HTTP for
// AuxPoW data only; the header stream stays on P2P matching BTC/LTC.
// ====================================================================

/// Politeness sleep between consecutive blockchair HTTP requests in
/// the AuxPoW fetch loop. Blockchair's free tier rate-limits aggressive
/// burst requests; 1s spacing keeps us comfortably below the threshold.
pub const DOGE_AUXPOW_POLITE_SLEEP_MS: u64 = 1000;

/// Pure JSON parser for blockchair's `dogecoin/raw/block/<height>`
/// response. Returns `Ok(None)` if the block has no AuxPoW bit,
/// `Ok(Some(bytes))` with iriumd-format AuxPoW bytes (compatible with
/// `auxpow::deserialize`) when AuxPoW data is present. Pure function,
/// no I/O — testable with a hard-coded JSON literal.
fn parse_blockchair_auxpow_json(
    json: &Value,
    height: u64,
) -> Result<Option<Vec<u8>>, String> {
    let height_str = height.to_string();
    let entry = json
        .get("data")
        .and_then(|d| d.get(&height_str))
        .ok_or_else(|| format!("blockchair response missing data.{height_str}"))?;
    let decoded = entry
        .get("decoded_raw_block")
        .ok_or_else(|| "blockchair entry missing decoded_raw_block".to_string())?;
    let version = decoded
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "decoded_raw_block.version missing or not u64".to_string())?
        as u32;

    if version & 0x100 == 0 {
        return Ok(None);
    }

    let ap_json = decoded
        .get("auxpow")
        .ok_or_else(|| "AuxPoW bit set but auxpow field missing".to_string())?;

    // auxpow.tx is a nested dict with "hex" field (full coinbase tx).
    let tx_hex = ap_json
        .get("tx")
        .and_then(|t| t.get("hex"))
        .and_then(|h| h.as_str())
        .ok_or_else(|| "auxpow.tx.hex missing or not str".to_string())?;
    let coinbase_txn =
        hex::decode(tx_hex).map_err(|e| format!("auxpow.tx.hex decode: {e}"))?;

    // auxpow.parentblock is the parent header hex (must be 80 bytes).
    let parent_hex = ap_json
        .get("parentblock")
        .and_then(|p| p.as_str())
        .ok_or_else(|| "auxpow.parentblock missing or not str".to_string())?;
    let parent_bytes = hex::decode(parent_hex)
        .map_err(|e| format!("auxpow.parentblock decode: {e}"))?;
    if parent_bytes.len() != 80 {
        return Err(format!(
            "auxpow.parentblock wrong length {} (expected 80)",
            parent_bytes.len()
        ));
    }
    let mut parent_header = [0u8; 80];
    parent_header.copy_from_slice(&parent_bytes);
    let parent_hash = crate::pow::sha256d(&parent_header);

    let coinbase_index = ap_json
        .get("index")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "auxpow.index missing or not u64".to_string())? as u32;
    let chain_index = ap_json
        .get("chainindex")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "auxpow.chainindex missing or not u64".to_string())? as u32;

    let coinbase_branch = parse_branch_array(
        ap_json
            .get("merklebranch")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "auxpow.merklebranch missing or not array".to_string())?,
        "merklebranch",
    )?;
    let blockchain_branch = parse_branch_array(
        ap_json
            .get("chainmerklebranch")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "auxpow.chainmerklebranch missing or not array".to_string())?,
        "chainmerklebranch",
    )?;

    let ap = crate::auxpow::AuxPoW {
        coinbase_txn,
        parent_hash,
        coinbase_branch,
        coinbase_branch_index: coinbase_index,
        blockchain_branch,
        blockchain_branch_index: chain_index,
        parent_header,
    };

    Ok(Some(crate::auxpow::serialize(&ap)))
}

fn parse_branch_array(arr: &[Value], label: &str) -> Result<Vec<[u8; 32]>, String> {
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        let hex_str = v
            .as_str()
            .ok_or_else(|| format!("{label} entry not str"))?;
        let mut bytes = hex::decode(hex_str)
            .map_err(|e| format!("{label} hex decode: {e}"))?;
        if bytes.len() != 32 {
            return Err(format!(
                "{label} entry wrong length {} (expected 32)",
                bytes.len()
            ));
        }
        // Blockchair returns hashes in display order (big-endian
        // uint256); reverse to natural-byte-order to match how
        // compute_merkle_root inside auxpow::validate expects them.
        bytes.reverse();
        let mut h = [0u8; 32];
        h.copy_from_slice(&bytes);
        out.push(h);
    }
    Ok(out)
}

/// Fetch the AuxPoW data for one DOGE block by height via blockchair's
/// HTTP API. Returns `Ok(None)` for blocks without the AuxPoW bit
/// (pre-371,337 or no-bit pools post-activation) and `Ok(Some(bytes))`
/// otherwise — `bytes` is the iriumd-format AuxPoW serialization,
/// directly compatible with `auxpow::deserialize` and PR-5's
/// `apply_doge_header_batch_with_auxpow` validator.
pub async fn fetch_doge_block_auxpow(
    client: &Client,
    height: u64,
) -> Result<Option<Vec<u8>>, String> {
    let url = format!("https://api.blockchair.com/dogecoin/raw/block/{height}");
    let mut last_err = String::new();
    for attempt in 0..PER_HEADER_RETRIES {
        let result: Result<Option<Vec<u8>>, String> = async {
            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("blockchair request: {e}"))?;
            let status = resp.status();
            if !status.is_success() {
                return Err(format!("blockchair HTTP {status}"));
            }
            let json: Value = resp
                .json()
                .await
                .map_err(|e| format!("blockchair json parse: {e}"))?;
            parse_blockchair_auxpow_json(&json, height)
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
        "blockchair auxpow h={height} exhausted {PER_HEADER_RETRIES} attempts; last error: {last_err}"
    ))
}

#[cfg(test)]
mod option_b_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_blockchair_returns_none_when_auxpow_bit_clear() {
        let response = json!({
            "data": {
                "100": {
                    "decoded_raw_block": {
                        "version": 0x20000000u64,
                    }
                }
            }
        });
        let r = parse_blockchair_auxpow_json(&response, 100).expect("parse");
        assert!(r.is_none(), "no AuxPoW bit -> None");
    }

    #[test]
    fn parse_blockchair_errors_on_missing_height_entry() {
        let response = json!({ "data": {} });
        let err = parse_blockchair_auxpow_json(&response, 100).unwrap_err();
        assert!(err.contains("missing data.100"), "got: {err}");
    }

    #[test]
    fn parse_blockchair_errors_when_auxpow_bit_set_but_no_auxpow_field() {
        let response = json!({
            "data": {
                "100": {
                    "decoded_raw_block": {
                        "version": 0x00010102u64,
                    }
                }
            }
        });
        let err = parse_blockchair_auxpow_json(&response, 100).unwrap_err();
        assert!(err.contains("auxpow field missing"), "got: {err}");
    }

    #[test]
    fn parse_blockchair_assembles_minimal_auxpow_round_trip() {
        // Minimal AuxPoW: empty branches, 60-byte coinbase, 80-byte
        // parent_header. The assembled iriumd-format bytes must
        // round-trip through auxpow::deserialize without error.
        let tx_hex = "010000000100000000000000000000000000000000000000000000000000000000000000000000000000ffffffff0100000000000000000000000000";
        let parent_hex = "00".repeat(80);
        let response = json!({
            "data": {
                "200": {
                    "decoded_raw_block": {
                        "version": 0x00010102u64,
                        "auxpow": {
                            "tx": { "hex": tx_hex },
                            "index": 0u64,
                            "chainindex": 0u64,
                            "merklebranch": [],
                            "chainmerklebranch": [],
                            "parentblock": parent_hex,
                        }
                    }
                }
            }
        });
        let bytes = parse_blockchair_auxpow_json(&response, 200)
            .expect("parse")
            .expect("Some bytes for AuxPoW-bit header");
        let mut off = 0usize;
        let parsed = crate::auxpow::deserialize(&bytes, &mut off)
            .expect("iriumd auxpow deserialize round-trip");
        assert_eq!(off, bytes.len(), "deserialize consumed all bytes");
        assert_eq!(parsed.coinbase_branch.len(), 0);
        assert_eq!(parsed.blockchain_branch.len(), 0);
        assert_eq!(parsed.coinbase_branch_index, 0);
        assert_eq!(parsed.blockchain_branch_index, 0);
        assert_eq!(parsed.parent_header.len(), 80);
    }

    #[test]
    fn parse_blockchair_branch_array_reverses_display_to_natural() {
        // Single 32-byte branch in display order. The assembled bytes
        // should contain the REVERSED hash (natural / wire order).
        let display = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
        let mut natural = hex::decode(display).unwrap();
        natural.reverse();
        let tx_hex = "010000000100000000000000000000000000000000000000000000000000000000000000000000000000ffffffff0100000000000000000000000000";
        let parent_hex = "00".repeat(80);
        let response = json!({
            "data": {
                "200": {
                    "decoded_raw_block": {
                        "version": 0x00010102u64,
                        "auxpow": {
                            "tx": { "hex": tx_hex },
                            "index": 0u64,
                            "chainindex": 0u64,
                            "merklebranch": [display],
                            "chainmerklebranch": [],
                            "parentblock": parent_hex,
                        }
                    }
                }
            }
        });
        let bytes = parse_blockchair_auxpow_json(&response, 200)
            .expect("parse")
            .expect("Some bytes");
        let mut off = 0usize;
        let parsed = crate::auxpow::deserialize(&bytes, &mut off)
            .expect("deserialize");
        assert_eq!(parsed.coinbase_branch.len(), 1);
        assert_eq!(&parsed.coinbase_branch[0][..], &natural[..]);
    }
}
