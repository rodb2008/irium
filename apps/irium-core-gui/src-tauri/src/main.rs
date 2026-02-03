mod explorer;
mod log_tail;
mod node_manager;
mod rpc_client;
mod settings;
mod wallet;

use crate::explorer::{fetch_block, fetch_tx, latest_blocks, BlockSummary};
use crate::log_tail::spawn_log_tail;
use crate::node_manager::NodeManager;
use crate::rpc_client::RpcClient;
use crate::settings::{load_settings, save_settings as save_settings_file, Settings};
use crate::wallet::WalletService;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};


fn format_irm_value(amount: i64) -> String {
    let sign = if amount < 0 { "-" } else { "" };
    let value = amount.abs() as u64;
    let whole = value / 100_000_000;
    let frac = value % 100_000_000;
    if frac == 0 {
        format!("{}{}", sign, whole)
    } else {
        format!("{}{}.{}", sign, whole, format!("{:08}", frac))
    }
}

struct AppState {
    settings: Mutex<Settings>,
    node: Mutex<NodeManager>,
    wallet: Mutex<WalletService>,
    log_tail_started: Mutex<bool>,
}

fn rpc_client_from(settings: &Settings) -> Result<RpcClient, String> {
    RpcClient::new(
        &settings.rpc_url,
        settings.rpc_token.clone(),
        settings.rpc_ca.clone(),
        settings.rpc_allow_insecure,
    )
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(state: State<AppState>, settings: Settings) -> Result<(), String> {
    save_settings_file(&settings)?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
fn start_node(state: State<AppState>) -> Result<(), String> {
    let settings = state.settings.lock().unwrap().clone();
    if settings.mode == "attach" {
        return Ok(());
    }
    let mut node = state.node.lock().unwrap();
    node.start(&settings)
}

#[tauri::command]
fn stop_node(state: State<AppState>) -> Result<(), String> {
    let mut node = state.node.lock().unwrap();
    node.stop()
}

#[tauri::command]
async fn get_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let settings = state.settings.lock().unwrap().clone();
    let client = rpc_client_from(&settings)?;
    client.get_json("/status").await
}

#[tauri::command]
fn start_log_tail(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let mut started = state.log_tail_started.lock().unwrap();
    if *started {
        return Ok(());
    }
    let settings = state.settings.lock().unwrap().clone();
    let path = PathBuf::from(settings.log_file);
    spawn_log_tail(app, path);
    *started = true;
    Ok(())
}

#[derive(Serialize)]
struct WalletReceive {
    address: String,
    qr_svg: String,
}

#[tauri::command]
fn wallet_create(state: State<AppState>, passphrase: String) -> Result<WalletReceive, String> {
    let settings = state.settings.lock().unwrap().clone();
    let mut wallet = state.wallet.lock().unwrap();
    let addr = wallet.create_wallet(PathBuf::from(settings.data_dir).as_path(), &passphrase)?;
    let qr_svg = irium_node_rs::qr::render_svg(&addr, 6, 2)?;
    Ok(WalletReceive { address: addr, qr_svg })
}

#[tauri::command]
fn wallet_unlock(state: State<AppState>, passphrase: String) -> Result<(), String> {
    let settings = state.settings.lock().unwrap().clone();
    let mut wallet = state.wallet.lock().unwrap();
    wallet.unlock_wallet(PathBuf::from(settings.data_dir).as_path(), &passphrase)
}

#[tauri::command]
fn wallet_lock(state: State<AppState>) {
    let mut wallet = state.wallet.lock().unwrap();
    wallet.lock_wallet();
}

#[tauri::command]
fn wallet_new_address(state: State<AppState>) -> Result<WalletReceive, String> {
    let settings = state.settings.lock().unwrap().clone();
    let mut wallet = state.wallet.lock().unwrap();
    let addr = wallet.new_address(PathBuf::from(settings.data_dir).as_path())?;
    let qr_svg = irium_node_rs::qr::render_svg(&addr, 6, 2)?;
    Ok(WalletReceive { address: addr, qr_svg })
}

#[tauri::command]
fn wallet_receive(state: State<AppState>) -> Result<WalletReceive, String> {
    let settings = state.settings.lock().unwrap().clone();
    let mut wallet = state.wallet.lock().unwrap();
    let addr = wallet.current_address(settings.auto_lock_minutes)?;
    let qr_svg = irium_node_rs::qr::render_svg(&addr, 6, 2)?;
    Ok(WalletReceive { address: addr, qr_svg })
}

#[derive(Serialize)]
struct WalletBalance {
    confirmed: String,
    unconfirmed: String,
}

#[tauri::command]
async fn wallet_balance(state: State<'_, AppState>) -> Result<WalletBalance, String> {
    let settings = state.settings.lock().unwrap().clone();
    let client = rpc_client_from(&settings)?;
    let mut wallet = state.wallet.lock().unwrap();
    let (confirmed, unconfirmed) = wallet.balance(&client, settings.auto_lock_minutes).await?;
    Ok(WalletBalance { confirmed, unconfirmed })
}

#[derive(Serialize)]
struct WalletTx {
    txid: String,
    height: u64,
    net: String,
}

#[tauri::command]
async fn wallet_history(state: State<'_, AppState>, limit: usize) -> Result<Vec<WalletTx>, String> {
    let settings = state.settings.lock().unwrap().clone();
    let client = rpc_client_from(&settings)?;
    let mut wallet = state.wallet.lock().unwrap();
    let items = wallet.history(&client, settings.auto_lock_minutes, limit).await?;
    Ok(items
        .into_iter()
        .map(|i| WalletTx {
            txid: i.txid,
            height: i.height,
            net: format_irm_value(i.net),
        })
        .collect())
}

#[derive(Serialize)]
struct WalletSendResponse {
    txid: String,
}

#[tauri::command]
async fn wallet_send(
    state: State<'_, AppState>,
    to: String,
    amount: String,
    fee_mode: String,
) -> Result<WalletSendResponse, String> {
    let settings = state.settings.lock().unwrap().clone();
    let client = rpc_client_from(&settings)?;
    let mut wallet = state.wallet.lock().unwrap();
    let txid = wallet
        .send(&client, settings.auto_lock_minutes, &to, &amount, &fee_mode)
        .await?;
    Ok(WalletSendResponse { txid })
}

#[tauri::command]
async fn explorer_blocks(state: State<'_, AppState>, limit: usize) -> Result<Vec<BlockSummary>, String> {
    let settings = state.settings.lock().unwrap().clone();
    let client = rpc_client_from(&settings)?;
    latest_blocks(&client, limit).await
}

#[tauri::command]
async fn explorer_block(state: State<'_, AppState>, height: u64) -> Result<serde_json::Value, String> {
    let settings = state.settings.lock().unwrap().clone();
    let client = rpc_client_from(&settings)?;
    fetch_block(&client, height).await
}

#[tauri::command]
async fn explorer_tx(state: State<'_, AppState>, txid: String) -> Result<serde_json::Value, String> {
    let settings = state.settings.lock().unwrap().clone();
    let client = rpc_client_from(&settings)?;
    fetch_tx(&client, &txid).await
}

fn main() {
    let settings = load_settings();
    tauri::Builder::default()
        .manage(AppState {
            settings: Mutex::new(settings),
            node: Mutex::new(NodeManager::default()),
            wallet: Mutex::new(WalletService::new()),
            log_tail_started: Mutex::new(false),
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            start_node,
            stop_node,
            get_status,
            start_log_tail,
            wallet_create,
            wallet_unlock,
            wallet_lock,
            wallet_new_address,
            wallet_receive,
            wallet_balance,
            wallet_history,
            wallet_send,
            explorer_blocks,
            explorer_block,
            explorer_tx,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
