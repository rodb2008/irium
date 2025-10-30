# Irium Quick Start (v1.0)

- Latest Release: v1.0 (stable mining + P2P sync)
- Release Notes: https://github.com/iriumlabs/irium/releases/tag/v1.0

## Install Dependencies
```bash
pip3 install --user pycryptodome qrcode pillow
```

## 1) Download & Install
```bash
wget https://iriumlabs.org/releases/v1.0/irium-bootstrap-v1.0.tar.gz
tar -xzf irium-bootstrap-v1.0.tar.gz
cd irium-bootstrap-v1.0
chmod +x install.sh
./install.sh
```

## 2) Start Node
```bash
sudo systemctl start irium-node
sudo systemctl enable irium-node
sudo journalctl -u irium-node -f
```

## 3) Create Wallet
```bash
python3 scripts/irium-wallet-proper.py create
python3 scripts/irium-wallet-proper.py new-address
```

## 4) Start Mining

Single miner (full P2P):
```bash
export IRIUM_WALLET_FILE="$HOME/.irium/irium-wallet.json"
nohup python3 -u scripts/irium-node.py 38291 > /tmp/node.log 2>&1 &
python3 scripts/irium-miner.py 38292
```

Multicore (full P2P):
```bash
export IRIUM_WALLET_FILE="$HOME/.irium/irium-wallet.json"
nohup python3 -u scripts/irium-node.py 38291 > /tmp/node.log 2>&1 &
bash scripts/irium-miner-multicore.sh 4
./scripts/tail-mining-logs.sh 4 38292
```

## 5) Status / Troubleshooting
```bash
sudo journalctl -u irium-node -n 20
ls ~/.irium/blocks/ | wc -l
```

## APIs
Base: https://api.iriumlabs.org/
```bash
curl https://api.iriumlabs.org/api/stats
curl https://api.iriumlabs.org/api/block/1
curl "https://api.iriumlabs.org/api/blocks?limit=10"
```

### Miner port usage (important)

- The full miner expects the P2P port as a positional argument (no flag).
  - Default (uses 38292):
    python3 scripts/irium-miner.py
  - Specific port (example 39292):
    python3 scripts/irium-miner.py 39292
- Do NOT include parentheses or URLs around the command; that breaks it.
- If you prefer a flag, use the individual miner:
  python3 scripts/irium-miner-individual.py --wallet "$HOME/.irium/irium-wallet.json" --port 39292

#### Multicore
- Launch N workers (base port 38292 increments by 1 per worker):
  bash scripts/irium-miner-multicore.sh 4
- Tail logs:
  ./scripts/tail-mining-logs.sh 4 39292

#### Troubleshooting
- Ensure you’re on the main branch (not gh-pages):
  git branch --show-current  # should be: main
- Make sure a node is running (example 39291):
  nohup python3 -u scripts/irium-node.py 39291 > /tmp/node-39291.log 2>&1 &
- Check miner log shows “Starting mining loop” / “Nonce:” / “Hashrate:”:
  tail -n 120 /tmp/miner-39292.log
- If specifying a port “doesn’t work”:
  - Remove any parentheses/links from the command
  - Try a different free port: python3 scripts/irium-miner.py 40292
  - Or use: python3 scripts/irium-miner-individual.py --wallet "$HOME/.irium/irium-wallet.json" --port 39292
