# Irium Blockchain (IRM)

[![Release](https://img.shields.io/badge/release-v1.0-blue)](https://github.com/iriumlabs/irium/releases/tag/v1.0)
[![Status](https://img.shields.io/badge/status-stable-brightgreen)](#)
[![Network](https://img.shields.io/badge/network-LIVE-brightgreen)](#)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)


A next-generation proof-of-work blockchain designed for true decentralization.

- Release: v1.0 – Production Release (stable mining + P2P sync)
- Release Notes: https://github.com/iriumlabs/irium/releases/tag/v1.0

## What is Irium?

Irium is a decentralized cryptocurrency built from the ground up with a focus on solving real problems in blockchain networks. Using proven SHA-256d proof-of-work (same as Bitcoin), Irium introduces 8 unique innovations that make it more resilient, accessible, and fair.

## Why Irium?

- True Decentralization: Zero-DNS bootstrap (no single point of failure)
- Ultra-Low Fees: 0.0001 IRM per transaction
- Fair Launch: No ICO, no premine (3.5% founder vesting with timelocks)
- Energy Efficient: SHA-256d can leverage existing BTC mining infra
- Mobile-First: SPV + NiPoPoW support
- Incentivized Network: Relay rewards for node operators

## Technical Specifications

| Parameter           | Value                    | Description                            |
|--------------------|--------------------------|----------------------------------------|
| Ticker             | IRM                      | Official symbol                        |
| Algorithm          | SHA-256d                 | Proof-of-Work (Bitcoin-compatible)     |
| Max Supply         | 100,000,000 IRM          | Hard cap                               |
| Genesis Vesting    | 3,500,000 IRM            | 3.5% timelocked (1y/2y/3y)             |
| Mineable Supply    | 96,500,000 IRM           | Available to public miners             |
| Block Time         | ~13 minutes (780 sec)    | Bitcoin-like                           |
| Initial Reward     | 50 IRM                   | First 210,000 blocks                   |
| Halving            | Every 210,000 blocks     | ~4 years                               |
| Retarget           | Every 2016 blocks        | ~14 days                               |
| Coinbase Maturity  | 100 blocks               | Rewards mature after 100 blocks        |
| Min Tx Fee         | 0.0001 IRM               | 10,000 satoshis (ultra-low)            |
| P2P Port           | 38291                    | Default network port                   |

## 8 Unique Innovations

1) Zero-DNS Bootstrap: signed IP multiaddr seedlist + checkpoint anchors  
2) Self-Healing Peer Discovery: uptime proofs and peer reputation  
3) Genesis Vesting with CLTV: 3 UTXOs timelocked (1y/2y/3y)  
4) Per-Transaction Relay Rewards: up to 10% of fees to relays  
5) Sybil-Resistant Handshake: small PoW challenge on connect  
6) Anchor-File Consensus: signed headers protect against eclipse  
7) Light Client First (SPV + NiPoPoW): mobile/IoT without full chain  
8) On-chain Metadata Commitments: hash pointers in coinbase

## Security & Consensus (v1.0)

- Coinbase Maturity Enforcement (100-block lock)  
- Timestamp Validation (within a 2-hour window)  
- Signature Verification (every transaction validated)  
- UTXO Height Tracking (maturity + correctness)  
- Fixes: nonce overflow, P2P memory leak; improved shutdown and errors

Note: v1.0 is a consensus hard fork—upgrade required.

## Quick Start (v1.0)

### Download & Install
```bash
wget https://iriumlabs.org/releases/v1.0/irium-bootstrap-v1.0.tar.gz
tar -xzf irium-bootstrap-v1.0.tar.gz
cd irium-bootstrap-v1.0
chmod +x install.sh
./install.sh
```

### Start Node
```bash
sudo systemctl start irium-node
sudo systemctl enable irium-node
sudo journalctl -u irium-node -f
```

### Create Wallet
```bash
python3 scripts/irium-wallet-proper.py create
python3 scripts/irium-wallet-proper.py new-address
```

### Start Mining

Single-core (full P2P):
```bash
export IRIUM_WALLET_FILE="$HOME/.irium/irium-wallet.json"
nohup python3 -u scripts/irium-node.py 39291 > /tmp/node-39291.log 2>&1 &

> **Port overrides:** The systemd-managed node reads `~/.irium/system-node-port` on startup. Write a new port into that file (e.g., `39291`) to move the background service off 38291 so manual `python3 scripts/irium-node.py --port 38291` runs can bind cleanly. Manual shells still default to 38291 unless you pass `--port`.

python3 scripts/irium-miner.py 39292
```

Multicore (full P2P):
```bash
export IRIUM_WALLET_FILE="$HOME/.irium/irium-wallet.json"
nohup python3 -u scripts/irium-node.py 39291 > /tmp/node-39291.log 2>&1 &
bash scripts/irium-miner-multicore.sh 4
./scripts/tail-mining-logs.sh 4 39292
```

### Status / Troubleshooting
```bash
sudo journalctl -u irium-node -n 20
ls ~/.irium/blocks/ | wc -l
```

## Miner port usage (important)

- The full miner expects the P2P port as a positional argument (no flag).
  - Default (uses 38292):
    ```bash
    python3 scripts/irium-miner.py
    ```
  - Specific port (example 39292):
    ```bash
    python3 scripts/irium-miner.py 39292
    ```
- Do NOT include parentheses or URLs around the command; that breaks it.
- If you prefer a flag, use the individual miner:
  ```bash
  python3 scripts/irium-miner-individual.py --wallet "$HOME/.irium/irium-wallet.json" --port 39292
  ```

### Multicore
- Launch N workers (base port 38292 increments by 1 per worker):
  ```bash
  bash scripts/irium-miner-multicore.sh 4
  ```
- Tail logs:
  ```bash
  ./scripts/tail-mining-logs.sh 4 39292
  ```

### Troubleshooting
- Ensure you’re on the main branch (not gh-pages):
  ```bash
  git branch --show-current  # should be: main
  ```
- Make sure a node is running (example 39291):
  ```bash
  nohup python3 -u scripts/irium-node.py 39291 > /tmp/node-39291.log 2>&1 &
  ```
- Check miner log shows “Starting mining loop” / “Nonce:” / “Hashrate:”:
  ```bash
  tail -n 120 /tmp/miner-39292.log
  ```
- If specifying a port “doesn’t work”:
  - Remove any parentheses/links from the command
  - Try a different free port:
    ```bash
    python3 scripts/irium-miner.py 40292
    ```
  - Or use:
    ```bash
    python3 scripts/irium-miner-individual.py --wallet "$HOME/.irium/irium-wallet.json" --port 39292
    ```

## CLI Usage

- Launch node: `python -m irium node --port 38291`
- Launch miner: `python -m irium miner 38292`
- Explorer API: `python -m irium explorer`
- Wallet API: `python -m irium wallet-api`
- Verify genesis: `python -m irium verify-genesis`

All commands must run from the repo root so the CLI can locate the scripts and locked genesis files.

## Testing & QA

```bash
python -m venv .venv && . .venv/bin/activate
pip install -r requirements.txt pytest  # or just pip install pytest
PYTHONPATH=$PWD pytest
```

These tests confirm the locked-genesis data matches the packaged header and that a fresh `ChainState` boots on top of it. Add new tests for consensus or networking changes before shipping a release.

## Anchor Signing

Use `python scripts/sign_anchor.py --signer <label>` to canonicalize `bootstrap/anchors.json`, sign it with `ssh-keygen -Y sign`, and append the base64 signature. By default the helper reads `~/.ssh/git-signing` and uses the `irium-anchor` namespace.

Verify the resulting file before publishing:

```bash
python3 irium/tools/verify_bootstrap.py --anchors bootstrap/anchors.json
ssh-keygen -Y verify -f trusted_keys.txt -I <signer_label> -n irium-anchor -s bootstrap/anchors.json.sig < bootstrap/anchors.json
```

Distribute the signed file with releases so new nodes can validate checkpoints before syncing.

## Network Information

- Network: LIVE ✅
- Services: Operational ✅
- P2P Peers: Growing 🌱

Genesis (locked):
- Hash: `000000001f83c27ca5f3447e75a00ef1c66966af157fc12a823675b897f2fd6c`
- Merkle root: `cd78279c389b6f2f0a4edc567f3ba67b27daed60ab014342bb4a5b56c2ebb4db`
- Nonce/Bits/Time: `1364084797 / 0x1d00ffff / 1735689600`
- Vesting: 3.5M IRM timelocked to PxG1FmGiSnvfXJUcryLna2L5MB4iGG1KD7

## APIs

- Base: https://api.iriumlabs.org/
- Explorer env: set `IRIUM_EXPLORER_HOST` / `IRIUM_EXPLORER_PORT` (defaults to 127.0.0.1:8082) before `python -m irium explorer`.
- Wallet env: set `IRIUM_WALLET_HOST` / `IRIUM_WALLET_PORT` (defaults to 127.0.0.1:8080) before `python -m irium wallet-api`; terminate TLS via nginx or Caddy.

Explorer API
```bash
curl https://api.iriumlabs.org/api/stats
curl https://api.iriumlabs.org/api/block/1
curl "https://api.iriumlabs.org/api/blocks?limit=10"
```

Wallet API
```bash
# Docs
curl https://207.244.247.86:8080/
# Balance
curl https://207.244.247.86:8080/api/wallet/balance
# New address
curl -X POST https://207.244.247.86:8080/new-address
```

## Documentation

- QUICKSTART.md — 5-minute setup  
- MINING.md — single + multicore, full P2P  
- WHITEPAPER.md — technical  
- CONTRIBUTING.md — how to contribute

## Important Notes

Dependencies:
```bash
pip3 install --user pycryptodome qrcode pillow
```

Wallet & Mining:
- Miner loads your wallet at startup.
- If you create a new address, restart your miner.
- Check address:
```bash
sudo journalctl -u irium-miner.service | grep "Mining address" | tail -1
```

Blockchain Sync:
- Node loads `~/.irium/blocks/`, connects to seed peers, compares heights, fetches if peers are ahead.
- If everyone shows same height, the network is simply waiting for the next block.

## Community & Support

- GitHub: https://github.com/iriumlabs/irium  
- Discussions: https://github.com/iriumlabs/irium/discussions  
- Issues: https://github.com/iriumlabs/irium/issues  
- Email: info@iriumlabs.org

## License

MIT — Free and open source.

## Updated Mining Commands (v1.0)

Use a virtualenv (PEP 668), set PYTHONPATH, and pass miner port positionally.

```bash
# 0) Repo root
cd ~/irium

# 1) venv + deps (first time)
sudo apt install -y python3.12-venv
python3 -m venv .venv
. .venv/bin/activate
export PYTHONPATH="$PWD"
pip install pycryptodome qrcode pillow requests websockets aiohttp

# 2) Start node (38291)
nohup python3 -u scripts/irium-node.py 38291 > /tmp/node.log 2>&1 &

# 3) Single miner (positional port; no --port)
export IRIUM_WALLET_FILE="$HOME/.irium/irium-wallet.json"
python3 -u scripts/irium-miner.py 38292

# 4) Multicore (N workers; ports BASE..BASE+N-1)
export IRIUM_WALLET_FILE="$HOME/.irium/irium-wallet.json"
bash scripts/irium-miner-multicore.sh 4

# 5) Logs (carriage-returns to newlines)
tail -n 120 /tmp/miner-38292.log | tr '\r' '\n'

# Stop miners
pkill -f 'scripts/irium-miner\.py'
```

Notes:
- Full miner uses positional port (e.g., `python3 scripts/irium-miner.py 38292`).
- Use ASCII env var name `IRIUM_WALLET_FILE` (not lookalikes).
- If pip is blocked (PEP 668), the venv step above fixes it.
