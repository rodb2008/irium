const invoke = (cmd, args = {}) => {
  if (window.__TAURI__?.tauri?.invoke) {
    return window.__TAURI__.tauri.invoke(cmd, args);
  }
  return Promise.reject(new Error('Tauri not available'));
};

const state = {
  status: null,
  settings: null,
  walletUnlocked: false,
};

function setActivePage(id) {
  document.querySelectorAll('.page').forEach((el) => el.classList.remove('active'));
  document.getElementById(id)?.classList.add('active');
  document.querySelectorAll('.nav button').forEach((btn) => {
    btn.classList.toggle('active', btn.dataset.page === id);
  });
}

async function refreshStatus() {
  try {
    const data = await invoke('get_status');
    state.status = data;
    document.getElementById('stat-height').textContent = data.height ?? '—';
    document.getElementById('stat-peers').textContent = data.peer_count ?? data.peers ?? '—';
    document.getElementById('stat-tip').textContent = data.tip_hash ?? '—';
    document.getElementById('stat-node').textContent = 'Running';
    document.getElementById('stat-node').classList.add('ok');
    document.getElementById('status-bar-text').textContent = `Node running · height ${data.height ?? '?'} · peers ${data.peer_count ?? '?'}`;
  } catch (err) {
    document.getElementById('stat-node').textContent = 'Stopped';
    document.getElementById('stat-node').classList.remove('ok');
    document.getElementById('status-bar-text').textContent = 'Node stopped or unreachable';
  }
}

async function loadSettings() {
  const settings = await invoke('get_settings');
  state.settings = settings;
  document.getElementById('settings-mode').value = settings.mode;
  document.getElementById('settings-rpc').value = settings.rpc_url;
  document.getElementById('settings-token').value = settings.rpc_token ?? '';
  document.getElementById('settings-datadir').value = settings.data_dir;
}

async function saveSettings() {
  const updated = {
    ...state.settings,
    mode: document.getElementById('settings-mode').value,
    rpc_url: document.getElementById('settings-rpc').value,
    rpc_token: document.getElementById('settings-token').value,
    data_dir: document.getElementById('settings-datadir').value,
  };
  await invoke('save_settings', { settings: updated });
  state.settings = updated;
}

async function startNode() {
  await saveSettings();
  await invoke('start_node');
  refreshStatus();
}

async function stopNode() {
  await invoke('stop_node');
  refreshStatus();
}

async function walletCreate() {
  const pass = document.getElementById('wallet-pass').value.trim();
  if (!pass) return alert('Passphrase required');
  const resp = await invoke('wallet_create', { passphrase: pass });
  document.getElementById('wallet-address').textContent = resp.address;
  await walletRefresh();
}

async function walletUnlock() {
  const pass = document.getElementById('wallet-pass').value.trim();
  if (!pass) return alert('Passphrase required');
  await invoke('wallet_unlock', { passphrase: pass });
  state.walletUnlocked = true;
  await walletRefresh();
}

async function walletLock() {
  await invoke('wallet_lock');
  state.walletUnlocked = false;
}

async function walletNewAddress() {
  const resp = await invoke('wallet_new_address');
  document.getElementById('wallet-address').textContent = resp.address;
  await walletRefresh();
}

async function walletRefresh() {
  try {
    const address = await invoke('wallet_receive');
    document.getElementById('wallet-address').textContent = address.address;
    document.getElementById('wallet-qr').innerHTML = address.qr_svg;
  } catch {}
  try {
    const balance = await invoke('wallet_balance');
    document.getElementById('wallet-balance').textContent = `${balance.confirmed} IRM`;
    document.getElementById('wallet-unconfirmed').textContent = `${balance.unconfirmed} IRM`;
  } catch {}
  try {
    const txs = await invoke('wallet_history', { limit: 10 });
    const list = document.getElementById('wallet-tx-list');
    list.innerHTML = txs.map((tx) => `
      <div class="list-item">
        <strong>${tx.txid.slice(0, 12)}…</strong> · ${tx.net} IRM · height ${tx.height}
      </div>
    `).join('') || '<div class="list-item">No transactions</div>';
  } catch {}
}

async function walletSend() {
  const to = document.getElementById('send-to').value.trim();
  const amount = document.getElementById('send-amount').value.trim();
  const fee = document.getElementById('send-fee').value.trim();
  const resp = await invoke('wallet_send', { to, amount, fee_mode: fee });
  document.getElementById('send-result').textContent = `Sent. Txid: ${resp.txid}`;
  await walletRefresh();
}

async function loadBlocks() {
  const data = await invoke('explorer_blocks', { limit: 20 });
  const list = document.getElementById('explorer-blocks');
  list.innerHTML = data.map((b) => `
    <div class="list-item" onclick="showBlock('${b.height}')">
      <strong>Block ${b.height}</strong> · ${b.hash.slice(0, 12)}… · ${b.tx_count} tx · ${b.time}
    </div>
  `).join('');
}

async function showBlock(height) {
  const data = await invoke('explorer_block', { height: Number(height) });
  document.getElementById('explorer-detail').textContent = JSON.stringify(data, null, 2);
}

async function showTx() {
  const txid = document.getElementById('explorer-txid').value.trim();
  const data = await invoke('explorer_tx', { txid });
  document.getElementById('explorer-detail').textContent = JSON.stringify(data, null, 2);
}

async function initLogs() {
  if (!window.__TAURI__?.event?.listen) return;
  const logOutput = document.getElementById('log-output');
  await window.__TAURI__.event.listen('log_line', (event) => {
    logOutput.textContent += event.payload + '\n';
    logOutput.scrollTop = logOutput.scrollHeight;
  });
  await invoke('start_log_tail');
}

window.addEventListener('DOMContentLoaded', async () => {
  document.querySelectorAll('.nav button').forEach((btn) => {
    btn.addEventListener('click', () => setActivePage(btn.dataset.page));
  });
  setActivePage('page-dashboard');
  document.getElementById('btn-start-node').addEventListener('click', startNode);
  document.getElementById('btn-stop-node').addEventListener('click', stopNode);
  document.getElementById('btn-save-settings').addEventListener('click', saveSettings);
  document.getElementById('btn-wallet-create').addEventListener('click', walletCreate);
  document.getElementById('btn-wallet-unlock').addEventListener('click', walletUnlock);
  document.getElementById('btn-wallet-lock').addEventListener('click', walletLock);
  document.getElementById('btn-wallet-new').addEventListener('click', walletNewAddress);
  document.getElementById('btn-wallet-send').addEventListener('click', walletSend);
document.getElementById('btn-open-settings')?.addEventListener('click', () => setActivePage('page-settings'));
  document.getElementById('btn-explorer-refresh').addEventListener('click', loadBlocks);
  document.getElementById('btn-explorer-tx').addEventListener('click', showTx);

  await loadSettings();
  await refreshStatus();
  await walletRefresh();
  await loadBlocks();
  await initLogs();

  setInterval(refreshStatus, 5000);
});

window.showBlock = showBlock;
