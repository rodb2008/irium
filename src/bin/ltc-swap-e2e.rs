//! End-to-end IRM/LTC swap test against iriumd-devnet + litecoind regtest.
//!
//! Drives the direct `/rpc/createltcswap` -> `/rpc/claimltcswap` flow with
//! a single wallet acting as both maker and taker (locks IRM, builds the
//! LTC payment, claims with the LTC merkle proof). Verifies the BTC SPV
//! style payment proof consensus path end to end.
//!
//! Environment:
//!   IRIUMD_RPC_URL    default http://127.0.0.1:38400  (iriumd-devnet)
//!   IRIUMD_RPC_TOKEN  required                        (devnet wallet auth)
//!   LTC_RPC_URL       default http://127.0.0.1:19443  (litecoind regtest)
//!   LTC_RPC_USER      default iriumtest
//!   LTC_RPC_PASSWORD  default iriumtest
//!   LTC_RPC_WALLET    default "devnet" (litecoin-core multi-wallet scope)
//!   LTC_E2E_IRM       default "1.00000000"            (swap amount)
//!   LTC_E2E_SATS      default 100000                  (LTC payment sats)
//!   LTC_E2E_CONF      default 1                       (LTC confirmations required)
//!   LTC_E2E_PASSPHRASE  default ""                    (auto-unlock devnet wallet if set)
//!
//! Exit codes:
//!   0  full flow passed
//!   1  any step failed

use std::env;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use reqwest::blocking::Client;
use serde_json::{json, Value};

fn log(step: &str, status: &str, msg: &str) {
    println!("STEP_{step} status={status} {msg}");
}

// =====================================================================
// iriumd RPC helper (Bearer auth, REST/JSON)
// =====================================================================

struct Iriumd {
    client: Client,
    url: String,
    token: String,
}

impl Iriumd {
    fn from_env() -> Result<Self, String> {
        let url = env::var("IRIUMD_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:38400".to_string());
        let token = env::var("IRIUMD_RPC_TOKEN")
            .map_err(|_| "IRIUMD_RPC_TOKEN required".to_string())?;
        if token.trim().is_empty() {
            return Err("IRIUMD_RPC_TOKEN is empty".into());
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| format!("iriumd http client: {e}"))?;
        Ok(Self { client, url, token })
    }

    fn get(&self, path: &str) -> Result<Value, String> {
        let resp = self
            .client
            .get(format!("{}{path}", self.url))
            .bearer_auth(&self.token)
            .send()
            .map_err(|e| format!("GET {path}: {e}"))?;
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        if !status.is_success() {
            return Err(format!("GET {path} HTTP {status}: {body}"));
        }
        serde_json::from_str(&body)
            .map_err(|e| format!("GET {path} json parse: {e}; body: {body}"))
    }

    fn post(&self, path: &str, body: Value) -> Result<Value, String> {
        let resp = self
            .client
            .post(format!("{}{path}", self.url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .map_err(|e| format!("POST {path}: {e}"))?;
        let status = resp.status();
        let txt = resp.text().unwrap_or_default();
        if !status.is_success() {
            return Err(format!("POST {path} HTTP {status}: {txt}"));
        }
        serde_json::from_str(&txt)
            .map_err(|e| format!("POST {path} json parse: {e}; body: {txt}"))
    }
}

// =====================================================================
// litecoind RPC helper (Basic auth, JSON-RPC 1.0)
// =====================================================================

struct Litecoind {
    client: Client,
    url: String,
    wallet_url: String,
    user: String,
    password: String,
}

impl Litecoind {
    fn from_env() -> Result<Self, String> {
        let url = env::var("LTC_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:19443".to_string());
        let wallet = env::var("LTC_RPC_WALLET").unwrap_or_else(|_| "devnet".to_string());
        let user = env::var("LTC_RPC_USER").unwrap_or_else(|_| "iriumtest".to_string());
        let password = env::var("LTC_RPC_PASSWORD").unwrap_or_else(|_| "iriumtest".to_string());
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| format!("litecoind http: {e}"))?;
        let wallet_url = format!("{}/wallet/{}", url.trim_end_matches('/'), wallet);
        Ok(Self {
            client,
            url,
            wallet_url,
            user,
            password,
        })
    }

    fn rpc_at(&self, endpoint: &str, method: &str, params: Value) -> Result<Value, String> {
        let req = json!({
            "jsonrpc": "1.0",
            "id": "ltc-swap-e2e",
            "method": method,
            "params": params,
        });
        let resp = self
            .client
            .post(endpoint)
            .basic_auth(&self.user, Some(&self.password))
            .json(&req)
            .send()
            .map_err(|e| format!("ltc {method}: {e}"))?;
        let txt = resp.text().unwrap_or_default();
        let v: Value = serde_json::from_str(&txt)
            .map_err(|e| format!("ltc {method} json: {e}; raw: {txt}"))?;
        if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
            return Err(format!("ltc {method} error: {err}"));
        }
        Ok(v["result"].clone())
    }

    /// Chain-level RPCs (getblockcount, getblockhash, getblockheader, ...)
    fn rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        self.rpc_at(&self.url, method, params)
    }

    /// Wallet-scoped RPCs (getnewaddress, sendtoaddress, signrawtransactionwithwallet, ...)
    fn wallet_rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        self.rpc_at(&self.wallet_url, method, params)
    }
}

// =====================================================================
// Step 0 — probe iriumd-devnet LTC SPV relay
// =====================================================================

fn step_0_probe(iriumd: &Iriumd) -> Result<u64, String> {
    log("0", "start", "probing /rpc/ltcrelaytip");
    let tip = iriumd.get("/rpc/ltcrelaytip")?;
    let active = tip["active"].as_bool().unwrap_or(false);
    let height = tip["tip_height"].as_u64().unwrap_or(0);
    let hash = tip["tip_hash"].as_str().unwrap_or("?");
    if !active {
        return Err(format!("LTC SPV relay not active (tip_height={height})"));
    }
    if height < 1 {
        return Err(format!("LTC SPV tip too low for swap test: {height}"));
    }
    log("0", "ok", &format!("ltcrelaytip active=true height={height} hash={hash}"));
    Ok(height)
}

// =====================================================================
// Step 1 — get a fresh LTC address from litecoind (legacy P2PKH)
// =====================================================================

fn step_1_ltc_address(litecoind: &Litecoind) -> Result<String, String> {
    log("1", "start", "litecoind getnewaddress legacy P2PKH for swap payee");
    let v = litecoind.wallet_rpc(
        "getnewaddress",
        json!(["swap-payee", "legacy"]),
    )?;
    let addr = v.as_str().ok_or("getnewaddress did not return a string")?.to_string();
    log("1", "ok", &format!("ltc payee address={addr}"));
    Ok(addr)
}

// =====================================================================
// Step 2 — discover devnet wallet address + current iriumd height
// =====================================================================

fn step_2_wallet_info(iriumd: &Iriumd) -> Result<(String, u64), String> {
    log("2", "start", "/wallet/info and /rpc/getblocktemplate for height");
    let info = iriumd.get("/wallet/info")?;
    let unlocked = info["is_unlocked"].as_bool().unwrap_or(false);
    if !unlocked {
        return Err("devnet wallet is locked; POST /wallet/unlock with passphrase first".into());
    }
    let path = info["path"].as_str().unwrap_or("?");
    let addrs = iriumd.get("/wallet/addresses")?;
    let addr = addrs["addresses"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .ok_or("no wallet address found")?
        .to_string();

    let tmpl = iriumd.get("/rpc/getblocktemplate")?;
    let next_h = tmpl["height"].as_u64().unwrap_or(0);
    let irm_height = next_h.saturating_sub(1);

    log("2", "ok", &format!("wallet_unlocked=true path={path} addr={addr} irm_height={irm_height}"));
    Ok((addr, irm_height))
}

// =====================================================================
// Step 3 — POST /rpc/createltcswap (lock IRM, declare LTC expectation)
// =====================================================================

#[derive(Debug)]
struct SwapHandle {
    funding_txid: String,
    swap_vout: u32,
    funding_binding_hex: String,
    expected_ltc_amount_sats: u64,
    expected_ltc_payment_address: String,
    op_return_payload_hex: String,
    timeout_height: u64,
}

fn step_3_create_swap(
    iriumd: &Iriumd,
    irm_wallet_addr: &str,
    ltc_payee_addr: &str,
    irm_height_now: u64,
) -> Result<SwapHandle, String> {
    let irm_amount = env::var("LTC_E2E_IRM").unwrap_or_else(|_| "1.00000000".to_string());
    let ltc_sats: u64 = env::var("LTC_E2E_SATS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);
    let confirmations: u8 = env::var("LTC_E2E_CONF")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let timeout_height = irm_height_now + 100;

    log("3", "start", &format!(
        "POST /rpc/createltcswap irm={irm_amount} sats={ltc_sats} conf={confirmations} timeout={timeout_height}"
    ));

    let body = json!({
        "irm_amount": irm_amount,
        "ltc_amount_sats": ltc_sats,
        "ltc_recipient_address": ltc_payee_addr,
        "recipient_address": irm_wallet_addr,
        "refund_address": irm_wallet_addr,
        "confirmations_required": confirmations,
        "timeout_height": timeout_height,
        "fee_per_byte": 1,
        "broadcast": true,
    });

    let resp = iriumd.post("/rpc/createltcswap", body)?;
    let accepted = resp["accepted"].as_bool().unwrap_or(false);
    if !accepted {
        return Err(format!("createltcswap not accepted: {resp}"));
    }
    let handle = SwapHandle {
        funding_txid: resp["txid"].as_str().ok_or("missing txid")?.to_string(),
        swap_vout: resp["swap_vout"].as_u64().ok_or("missing swap_vout")? as u32,
        funding_binding_hex: resp["funding_binding_hex"].as_str().unwrap_or("").to_string(),
        expected_ltc_amount_sats: resp["expected_ltc_amount_sats"].as_u64().unwrap_or(ltc_sats),
        expected_ltc_payment_address: resp["expected_ltc_payment_address"]
            .as_str()
            .unwrap_or(ltc_payee_addr)
            .to_string(),
        op_return_payload_hex: resp["ltc_op_return_payload_hex"].as_str().unwrap_or("").to_string(),
        timeout_height,
    };
    log("3", "ok", &format!(
        "swap funded txid={} vout={} op_return={} binding={}",
        handle.funding_txid,
        handle.swap_vout,
        handle.op_return_payload_hex,
        handle.funding_binding_hex,
    ));
    Ok(handle)
}

// =====================================================================
// Steps 4-12 — TODO: mine iriumd, build LTC tx, mine LTC regtest, build
//   merkle proof, push LTC headers, claim, verify balance.
// =====================================================================

fn maybe_unlock(iriumd: &Iriumd) -> Result<(), String> {
    let pass = env::var("LTC_E2E_PASSPHRASE").unwrap_or_default();
    if pass.is_empty() {
        return Ok(());
    }
    let info = iriumd.get("/wallet/info").unwrap_or(Value::Null);
    if info["is_unlocked"].as_bool() == Some(true) {
        return Ok(());
    }
    log("unlock", "start", "/wallet/unlock with LTC_E2E_PASSPHRASE");
    iriumd
        .post("/wallet/unlock", json!({ "passphrase": pass }))
        .map_err(|e| format!("auto-unlock failed: {e}"))?;
    log("unlock", "ok", "wallet unlocked");
    Ok(())
}

fn run() -> Result<(), String> {
    let iriumd = Iriumd::from_env()?;
    let litecoind = Litecoind::from_env()?;

    maybe_unlock(&iriumd)?;

    let _ltc_tip = step_0_probe(&iriumd)?;
    let ltc_payee = step_1_ltc_address(&litecoind)?;
    let (irm_addr, irm_height) = step_2_wallet_info(&iriumd)?;
    let _handle = step_3_create_swap(&iriumd, &irm_addr, &ltc_payee, irm_height)?;

    // Pause briefly so the swap-funding tx makes it into mempool
    // before any downstream mining step would consume it.
    thread::sleep(Duration::from_millis(200));

    Err("steps 4-12 not yet implemented in this commit".into())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => {
            log("done", "ok", "all implemented steps passed");
            ExitCode::SUCCESS
        }
        Err(e) => {
            log("done", "fail", &e);
            ExitCode::FAILURE
        }
    }
}
