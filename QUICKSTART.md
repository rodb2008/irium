# Irium Rust Quickstart (Mainnet)

This guide runs a Rust Irium node/miner on mainnet (no testnet, no DNS). Assumes Rust toolchain is installed (`source ~/.cargo/env`).

## 1) Verify bootstrap artifacts
```
cd /home/irium/irium
# seeds + sig + trust root
ls bootstrap/seedlist.txt bootstrap/seedlist.txt.sig bootstrap/trust/allowed_signers
# anchors file is in bootstrap/anchors.json
```
The node verifies `seedlist.txt` against `allowed_signers` via `ssh-keygen -Y verify`.

## 2) Build and test
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
RUST_LOG=info cargo run --release --bin iriumd
```
- Seeds: signed `bootstrap/seedlist.txt` + cached `bootstrap/seedlist.runtime` (unless `p2p_seeds` overrides).
- HTTP API: defaults to 127.0.0.1:38300 (`IRIUM_NODE_HOST`, `IRIUM_NODE_PORT`).
- P2P: bind from config; uses sybil-resistant handshake and header-first sync.

## 5) Mining
Set a payout address (or PKH) so rewards are spendable:
```
cd /home/irium/irium
export IRIUM_MINER_ADDRESS=Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa   # or set IRIUM_MINER_PKH (40-hex)
source ~/.cargo/env
./target/release/irium-miner
```
- Relies on the local node/mempool.
- Optional: set `IRIUM_RELAY_ADDRESS` to advertise a relay payout address in coinbase outputs.

## 6) SPV check
```
source ~/.cargo/env
cargo run --release --bin irium-spv -- verify <height> <txid> <index> <proof_hex_csv>
```

## 7) systemd (optional)
See `scripts/iriumd.service.example`:
- Copy to `/etc/systemd/system/iriumd.service` (adjust user/paths).
- `systemctl daemon-reload && systemctl enable --now iriumd`.
Logs go to `journalctl -u iriumd`.

## Bootstrap/peer cache paths
- Signed seeds: `bootstrap/seedlist.txt` (+ .sig + trust/allowed_signers)
- Runtime peers: `bootstrap/seedlist.runtime`
- Anchors: `bootstrap/anchors.json` (anchor enforcement planned)
- Runtime state: `state/`
