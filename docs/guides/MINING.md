# Irium Mining Guide (Full P2P Miner)

This guide shows how to mine using the full P2P miner with your own wallet. No wallets or peers are hardcoded; everything is env/CLI driven (Bitcoin-style).

## 1) Create a miner wallet (per-miner)
```bash
# Creates ~/.irium-miners/<miner-id>/irium-wallet.json
scripts/setup-miner.sh my-miner-1
```

## 2) Single miner run (recommended)
```bash
# Select your wallet file
export IRIUM_WALLET_FILE="$HOME/.irium-miners/my-miner-1/irium-wallet.json"

# Start a node (pick a free port if 38291 is busy)
nohup python3 -u scripts/irium-node.py 38291 > /tmp/node.log 2>&1 &

# Start one full P2P miner (positional P2P port, no --port flag)
python3 scripts/irium-miner.py 38292
```

Stop:
```bash
pkill -f 'scripts/irium-miner.py'
pkill -f 'scripts/irium-node.py'
```

## 3) Multicore mining (full P2P miners)
```bash
# Use your wallet (same as above)
export IRIUM_WALLET_FILE="$HOME/.irium-miners/my-miner-1/irium-wallet.json"

# Ensure a node is running (example)
nohup python3 -u scripts/irium-node.py 38291 > /tmp/node.log 2>&1 &

# Launch N miners; logs in /tmp/miner-<port>.log
./scripts/irium-miner-multicore.sh 4
```

## 4) Environment and CLI (no hardcoding)
- Wallet file: `IRIUM_WALLET_FILE=/path/to/irium-wallet.json`
- P2P port (full miner): positional arg (e.g., `scripts/irium-miner.py 38292`)
- Node port: positional arg (e.g., `scripts/irium-node.py 38291`)
- Bootstrap peers (optional): `BOOTSTRAP_NODES="host1:port,host2:port"`
- Data dirs (optional overrides):
  - `IRIUM_BLOCKS_DIR` (default `~/.irium/blocks`)
  - `IRIUM_MEMPOOL_DIR` (default `~/.irium/mempool`)
  - `IRIUM_WALLET_FILE` (default `~/.irium/irium-wallet.json` for wallet API)

## 5) Troubleshooting
- Port in use: pick different ports (e.g., node `39291`, miner `39292+`).
- Wallet not found: ensure `IRIUM_WALLET_FILE` is set and file perms are `600`.
- Logs:
  - Node: `/tmp/node.log`
  - Miners: `/tmp/miner-<P2PPORT>.log`
