# Irium Blockchain (Rust Mainnet)

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Language: Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org)
[![Network: Mainnet Only](https://img.shields.io/badge/network-mainnet--only-red.svg)](#run-the-rust-node)

Irium is a production‑only proof‑of‑work blockchain for the IRM asset. The network launches with no testnet, no DNS seeds, and a locked genesis that enforces founder vesting and a 100 M IRM cap. This repository now contains the Rust implementation of the full node, miner, and SPV tooling.

- Consensus: SHA‑256d, 600s target, 2016‑block retarget, 50 IRM starting subsidy, halving every 210,000 blocks, 100‑block coinbase maturity, 100 M max supply (3.5 M CLTV‑locked at genesis).
- Bootstrap: signed `bootstrap/seedlist.txt` + `anchors.json`; runtime peers cached in `bootstrap/seedlist.runtime`.
- Design goal: mainnet‑first, DNS‑free bootstrap, light‑client friendly, optional relay rewards.

## Layout
- `src/` – Rust sources (node, P2P, miner, SPV, wallet primitives).
- `bootstrap/` – signed seedlist, anchors, trust roots.
- `configs/` – genesis/consensus config.
- `scripts/` – ops helpers (systemd example, setup scripts).
- `state/` – runtime data (peers.json, etc.).


## Get Started (Download + Run Services)
```bash
git clone https://github.com/iriumlabs/irium.git
cd irium
source ~/.cargo/env
cargo build --release
```
Run each service in its own terminal:
```bash
# Terminal 1: node
RUST_LOG=info ./target/release/iriumd
```
```bash
# Terminal 2: create an address (save the privkey) and start mining
./target/release/irium-wallet new-address
export IRIUM_MINER_ADDRESS=<YOUR_IRIUM_ADDRESS>
./target/release/irium-miner
```
```bash
# Terminal 3: check balance
./target/release/irium-wallet balance <YOUR_IRIUM_ADDRESS>
```
```bash
# Optional: SPV tool
./target/release/irium-spv --help
```
Notes:
- If the node requires `IRIUM_RPC_TOKEN`, export the same token for the miner and wallet.
- Keep the printed private key safe; it controls the funds for that address.

## Build & Test
```bash
cd /home/irium/irium
source ~/.cargo/env
cargo test --quiet
```

## Run the Rust Node

### Important runtime env vars
- `IRIUM_ANCHOR_MIN_SIGNERS`: minimum valid signatures required for `bootstrap/anchors.json` (default 1).
- `IRIUM_SYBIL_DIFFICULTY` / `IRIUM_SYBIL_DIFFICULTY_MAX`: base and cap for sybil handshake PoW (default 10/20).
- `IRIUM_BANNED_LIST` / `IRIUM_BANNED_TRUST`: optional signed banlist path and allowed signer file (default `bootstrap/banned_peers.txt` + `.sig`, `bootstrap/trust/allowed_ban_signers`).
- `IRIUM_TLS_CERT` / `IRIUM_TLS_KEY`: PEM paths to enable HTTPS for the HTTP API (if unset, HTTP is used).
- `IRIUM_RPC_CA`: optional PEM CA/cert to trust when calling HTTPS RPC endpoints.
- `IRIUM_RPC_INSECURE`: set to `1` to skip TLS validation for HTTPS RPC calls (dev-only).
- `IRIUM_RPC_TOKEN`: optional bearer token required for `POST /rpc/submit_block` and `POST /rpc/submit_tx`.
- `IRIUM_RPC_BODY_MAX`: max HTTP RPC body size in bytes (default 32MB).
- `IRIUM_NODE_PUBLIC_IP` / `IRIUM_PUBLIC_IP`: optional public IP override for seed nodes.
- `IRIUM_PUBLIC_IP_PROBE_TARGET`: optional `host:port` to probe for outbound IP discovery.

```bash
# optional: set a config JSON with p2p_bind, relay_address, etc.
export IRIUM_NODE_CONFIG=/home/irium/irium/configs/node.json
source ~/.cargo/env
RUST_LOG=info cargo run --release --bin iriumd
```
- By default, seeds are taken from signed `bootstrap/seedlist.txt` (verified with `bootstrap/trust/allowed_signers`) and merged with `bootstrap/seedlist.runtime`.
- HTTP API binds to `IRIUM_NODE_HOST:IRIUM_NODE_PORT` (defaults 127.0.0.1:38300).
- P2P bind/seed overrides can be supplied in `configs/node.json`.

## System Services
Run the node/miner as systemd services so they restart after reboots:
```bash
cd /home/irium/irium
./install.sh
# edit env files, then enable miner
sudo systemctl enable --now irium-miner.service
```
- Node env: `/etc/irium/iriumd.env`
- Miner env: `/etc/irium/miner.env`

## Mining
The miner binary (`irium-miner`) assembles blocks from the local mempool and searches nonces. Always set a payout address so rewards are spendable:
```bash
cd /home/irium/irium
export IRIUM_MINER_ADDRESS=<YOUR_IRIUM_ADDRESS>   # or set IRIUM_MINER_PKH (40-hex)
source ~/.cargo/env
./target/release/irium-miner
```
- Mined blocks are auto-submitted to `IRIUM_NODE_RPC` (default http://127.0.0.1:38300).
- If the node is running HTTPS + `IRIUM_RPC_TOKEN`, set `IRIUM_NODE_RPC=https://127.0.0.1:38300` and export the same `IRIUM_RPC_TOKEN` for the miner.
- If `IRIUM_RPC_TOKEN` is set to an empty value in env files, miners will still send an empty token and get 401; either remove the line or set a real token.
- The miner pulls templates from `/rpc/getblocktemplate` and honors `IRIUM_GBT_MAX_TXS`, `IRIUM_GBT_MIN_FEE`, `IRIUM_GBT_LONGPOLL`, and `IRIUM_GBT_LONGPOLL_SECS`.
- Set `IRIUM_MINER_STRICT_RPC=1` to stop mining if RPC/template fetch fails.
- Pool mining (Stratum v1, TCP): set `IRIUM_STRATUM_URL`, `IRIUM_STRATUM_USER`, `IRIUM_STRATUM_PASS`.
Optional: set `IRIUM_RELAY_ADDRESS` to advertise a relay payout address in coinbase outputs.

## Wallet
The wallet CLI can create a new address and query balances from a running node:
```bash
cd /home/irium/irium
source ~/.cargo/env
./target/release/irium-wallet new-address
```
```bash
export IRIUM_RPC_URL=http://127.0.0.1:38300
# if the node requires auth
# export IRIUM_RPC_TOKEN=...
./target/release/irium-wallet balance <base58_address>
```
Use `irium-wallet address-to-pkh <base58_address>` to convert an address to its 20-byte pubkey hash.

## SPV Tooling
`irium-spv` verifies merkle proofs against stored block JSON snapshots:
```bash
source ~/.cargo/env
cargo run --release --bin irium-spv -- verify <height> <txid> <index> <proof_hex_csv>
```

## Bootstrap Artifacts
- `bootstrap/seedlist.txt` + `seedlist.txt.sig` – signed initial peers.
- `bootstrap/seedlist.runtime` – node‑learned peers (auto‑updated at runtime).
- `bootstrap/anchors.json` – anchor checkpoints (validation planned).
- `state/peers.json` – peer cache used when seeds are unavailable.

## Systemd Example
See `scripts/iriumd.service.example` for a unit that runs the node under journald with automatic restart.


### Banlist signing
Use SSH signatures for banlists: `ssh-keygen -Y sign -f <signing_key> -n file - < bootstrap/banned_peers.txt > bootstrap/banned_peers.txt.sig`.
Verify with `ssh-keygen -Y verify -f bootstrap/trust/allowed_ban_signers -I ban-signer -n file -s bootstrap/banned_peers.txt.sig < bootstrap/banned_peers.txt`.

## Status / TODO
- Fork‑aware sync, header tracking, relay‑address coinbase support are in place.
- Outstanding: anchor enforcement, richer peer scoring/libp2p parity, production monitoring hardening.
