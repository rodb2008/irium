# Irium Mining Guide (Rust Miner)

This guide covers solo mining against your own node and optional Stratum pool mining. The Rust miner pulls block templates from the node (Bitcoin-style) and auto-submits blocks back to the node.

## 1) Build the binaries
```bash
cd /home/irium/irium
source ~/.cargo/env
cargo build --release
```

## 2) Optional systemd setup
If you want the node/miner to survive reboots:
```bash
cd /home/irium/irium
./install.sh
```
Edit `/etc/irium/iriumd.env` and `/etc/irium/miner.env` before enabling the miner.

## 2) Run the node
```bash
RUST_LOG=info ./target/release/iriumd
```

## 3) Create a wallet address
```bash
./target/release/irium-wallet new-address
```
Save the printed private key. It controls the funds for that address.

## 4) Solo mining (recommended)
```bash
export IRIUM_MINER_ADDRESS=<YOUR_IRIUM_ADDRESS>
export IRIUM_NODE_RPC=http://127.0.0.1:38300
# If the node requires auth, export IRIUM_RPC_TOKEN too.
./target/release/irium-miner
```

Template tuning (optional):
- `IRIUM_GBT_MAX_TXS`: cap the number of mempool txs included in templates.
- `IRIUM_GBT_MIN_FEE`: minimum fee/byte to include in templates.
- `IRIUM_GBT_LONGPOLL=1`: enable long-poll template refresh.
- `IRIUM_GBT_LONGPOLL_SECS`: long-poll timeout (default 25s, max 120s).
- `IRIUM_MINER_STRICT_RPC=1`: stop mining if RPC/template fetch fails.

## 5) Stratum pool mining (optional)
Set a Stratum URL to enable pool mode (disables solo template mining):
```bash
export IRIUM_STRATUM_URL=stratum+tcp://pool.example.com:3333
export IRIUM_STRATUM_USER=<POOL_USER>
export IRIUM_STRATUM_PASS=<POOL_PASS>
./target/release/irium-miner
```
Notes:
- Stratum is TCP-only in the current miner.
- Pool mining uses the pool-provided coinbase/merkle and submits shares via `mining.submit`.

## 6) Check balance
```bash
./target/release/irium-wallet balance <YOUR_IRIUM_ADDRESS>
```

## Troubleshooting
- RPC unauthorized: ensure `IRIUM_RPC_TOKEN` matches the node.
- HTTPS mismatch: set `IRIUM_NODE_RPC=https://127.0.0.1:38300` if the node is running TLS.
- No templates: confirm node is running and reachable at `IRIUM_NODE_RPC`.
- Low hashrate: check CPU governor and ensure the miner is not throttled.
