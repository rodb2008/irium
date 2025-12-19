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

```bash
# optional: set a config JSON with p2p_bind, relay_address, etc.
export IRIUM_NODE_CONFIG=/home/irium/irium/configs/node.json
source ~/.cargo/env
RUST_LOG=info cargo run --release --bin iriumd
```
- By default, seeds are taken from signed `bootstrap/seedlist.txt` (verified with `bootstrap/trust/allowed_signers`) and merged with `bootstrap/seedlist.runtime`.
- HTTP API binds to `IRIUM_NODE_HOST:IRIUM_NODE_PORT` (defaults 127.0.0.1:38300).
- P2P bind/seed overrides can be supplied in `configs/node.json`.

## Mining
The miner binary (`irium-miner`) assembles blocks from the local mempool and searches nonces. Always set a payout address so rewards are spendable:
```bash
cd /home/irium/irium
export IRIUM_MINER_ADDRESS=Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa   # or set IRIUM_MINER_PKH (40-hex)
source ~/.cargo/env
./target/release/irium-miner
```
Optional: set `IRIUM_RELAY_ADDRESS` to advertise a relay payout address in coinbase outputs.

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

## Systemd Example
See `scripts/iriumd.service.example` for a unit that runs the node under journald with automatic restart.


### Banlist signing
Use SSH signatures for banlists: `ssh-keygen -Y sign -f <signing_key> -n file - < bootstrap/banned_peers.txt > bootstrap/banned_peers.txt.sig`.
Verify with `ssh-keygen -Y verify -f bootstrap/trust/allowed_ban_signers -I ban-signer -n file -s bootstrap/banned_peers.txt.sig < bootstrap/banned_peers.txt`.

## Status / TODO
- Fork‑aware sync, header tracking, relay‑address coinbase support are in place.
- Outstanding: anchor enforcement, richer peer scoring/libp2p parity, production monitoring hardening.
