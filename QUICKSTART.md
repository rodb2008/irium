# Irium Rust Quickstart (Mainnet)


## Plain-English Quickstart
If you are new to this, follow these steps in order. Keep the node running while you mine.

1) Install Rust (skip if already installed):
   - Visit https://rustup.rs and install, then open a new terminal.
2) Download and build:
```
git clone https://github.com/iriumlabs/irium.git
cd irium
source ~/.cargo/env
cargo build --release
```
3) Start the node (leave this running):
```
./target/release/iriumd
```
4) Create a wallet address (copy it):
```
./target/release/irium-wallet init
./target/release/irium-wallet new-address
```
5) Optional but recommended: set a simple RPC token to avoid rate limits:
```
export IRIUM_RPC_TOKEN=$(openssl rand -hex 24)
```
6) Start mining (use your address):
```
export IRIUM_MINER_ADDRESS=<YOUR_ADDRESS>
./target/release/irium-miner --threads 2
```
7) Check your balance:
```
./target/release/irium-wallet balance <YOUR_ADDRESS>
```
Notes:
- If the miner starts at height 1, the node is not reachable. Start `iriumd` or set `IRIUM_NODE_RPC=http://<node>:38300`.
- If you use `IRIUM_RPC_TOKEN`, the node and miner must use the same token value.

If you want the detailed/advanced steps, continue below.

This guide runs a Rust Irium node/miner on mainnet (no testnet, no DNS). Assumes Rust toolchain is installed (`source ~/.cargo/env`).

## 0) Download and build
```
git clone https://github.com/iriumlabs/irium.git
cd irium
source ~/.cargo/env
cargo build --release
```

## 1) Verify bootstrap artifacts
```
cd /home/irium/irium
# seeds + sig + trust root
ls bootstrap/seedlist.txt bootstrap/seedlist.txt.sig bootstrap/trust/allowed_signers
# anchors file is in bootstrap/anchors.json
```
The node verifies `seedlist.txt` against `allowed_signers` via `ssh-keygen -Y verify`.

## 2) Test (optional)
```
source ~/.cargo/env
cargo test --quiet
```

## 3) Configure (optional)
Create `configs/node.json` if you want to set P2P bind/seed or relay address:
```json
{
  "p2p_bind": "0.0.0.0:51001",
  "p2p_seeds": [],
  "relay_address": null
}
```
Set env var to use it:
```
export IRIUM_NODE_CONFIG=/home/irium/irium/configs/node.json
```

## 4) Run the node
```
source ~/.cargo/env
RUST_LOG=info ./target/release/iriumd
```
- Seeds: signed `bootstrap/seedlist.txt` + cached `bootstrap/seedlist.runtime` (unless `p2p_seeds` overrides).
- HTTP API: defaults to 127.0.0.1:38300 (`IRIUM_NODE_HOST`, `IRIUM_NODE_PORT`).
- P2P: bind from config; uses sybil-resistant handshake and header-first sync.

## 5) Create a wallet address
```
./target/release/irium-wallet init
./target/release/irium-wallet new-address
./target/release/irium-wallet list-addresses
./target/release/irium-wallet address-to-pkh <base58_address>
```
This creates `~/.irium/wallet.json` and prints the first address. Back up the wallet file.

## 6) Start mining
Set a payout address (or PKH) so rewards are spendable:
```
cd /home/irium/irium
export IRIUM_MINER_ADDRESS=<YOUR_IRIUM_ADDRESS>   # or set IRIUM_MINER_PKH (40-hex)
source ~/.cargo/env
./target/release/irium-miner
```
- Relies on the local node/mempool and auto-submits mined blocks to `IRIUM_NODE_RPC` (default http://127.0.0.1:38300).
- RPC auth tokens are user-defined. Example: `export IRIUM_RPC_TOKEN=$(openssl rand -hex 24)` and set the same value in `/etc/irium/iriumd.env` and `/etc/irium/miner.env`.
- Control CPU usage with `--threads N` or `IRIUM_MINER_THREADS=N` (default 1).
- If you run only the miner, point it at a reachable node RPC with `IRIUM_NODE_RPC=http://<node>:38300` (and `IRIUM_RPC_TOKEN` if required).
- Uses `/rpc/getblocktemplate`; tune with `IRIUM_GBT_MAX_TXS`, `IRIUM_GBT_MIN_FEE`, `IRIUM_GBT_LONGPOLL`, `IRIUM_GBT_LONGPOLL_SECS`.
- Optional: set `IRIUM_RELAY_ADDRESS` to advertise a relay payout address in coinbase outputs.
- If the node is using HTTPS + `IRIUM_RPC_TOKEN`, set `IRIUM_NODE_RPC=https://127.0.0.1:38300` and export the same `IRIUM_RPC_TOKEN` for the miner.
- If `IRIUM_RPC_TOKEN` is present but empty in `/etc/irium/*.env`, miners will send an empty token and get 401; delete the line or set a real token.
- If you see `HTTP 429 Too Many Requests`, set `IRIUM_RPC_TOKEN` in both `iriumd` and the miner, or raise `IRIUM_RATE_LIMIT_PER_MIN` in the node env.
- For HTTPS with a local self-signed cert, set `IRIUM_RPC_CA=/etc/irium/tls/irium-ca.crt` instead of `IRIUM_RPC_INSECURE=1`.
- Set `IRIUM_MINER_STRICT_RPC=1` to stop mining if RPC/template fetch fails.
- Pool mining (Stratum v1, TCP): set `IRIUM_STRATUM_URL`, `IRIUM_STRATUM_USER`, `IRIUM_STRATUM_PASS`.

## 7) Check balance
```
./target/release/irium-wallet balance <YOUR_IRIUM_ADDRESS>
./target/release/irium-wallet list-unspent <YOUR_IRIUM_ADDRESS>
./target/release/irium-wallet history <YOUR_IRIUM_ADDRESS>
./target/release/irium-wallet estimate-fee
./target/release/irium-wallet send <from_addr> <to_addr> <amount_irm>
./target/release/irium-wallet send <from_addr> <to_addr> <amount_irm> --coin-select largest
```

## 8) SPV check
```
source ~/.cargo/env
cargo run --release --bin irium-spv -- verify <height> <txid> <index> <proof_hex_csv>
```

## 9) systemd (recommended)
Install systemd units so the node/miner survive reboots:
```
cd /home/irium/irium
./install.sh
```
- Edit `/etc/irium/iriumd.env` and `/etc/irium/miner.env` for your paths and wallet.
- Optional API services: `/etc/irium/explorer.env` and `/etc/irium/wallet-api.env`.
- Enable the miner after setting `IRIUM_MINER_ADDRESS`:
```
sudo systemctl enable --now irium-miner.service
# optional APIs
sudo systemctl enable --now irium-explorer.service
sudo systemctl enable --now irium-wallet-api.service
```
Logs go to `journalctl -u iriumd`, `journalctl -u irium-miner`, `journalctl -u irium-explorer`, and `journalctl -u irium-wallet-api`.

## Bootstrap/peer cache paths
- Signed seeds: `bootstrap/seedlist.txt` (+ .sig + trust/allowed_signers)
- Runtime peers: `bootstrap/seedlist.runtime`
- Anchors: `bootstrap/anchors.json` (anchor enforcement planned)
- Peer cache: `state/peers.json` (used when seeds are unavailable)
- Runtime state: `state/`
