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
nohup python3 -u scripts/irium-node.py 39291 > /tmp/node-39291.log 2>&1 &
python3 scripts/irium-miner.py 39292
```

Multicore (full P2P):
```bash
export IRIUM_WALLET_FILE="$HOME/.irium/irium-wallet.json"
nohup python3 -u scripts/irium-node.py 39291 > /tmp/node-39291.log 2>&1 &
bash scripts/irium-miner-multicore.sh 4
./scripts/tail-mining-logs.sh 4 39292
```

### Miner port usage (important)

- Full miner expects the port as a positional argument (no flag).
  - Default (uses 38292):
    ```bash
    python3 scripts/irium-miner.py
    ```
  - Specific port (example 39292):
    ```bash
    python3 scripts/irium-miner.py 39292
    ```
- If you prefer a flag, use the individual miner:
  ```bash
  python3 scripts/irium-miner-individual.py --wallet "$HOME/.irium/irium-wallet.json" --port 39292
  ```

## 5) Status / Troubleshooting
```bash
sudo journalctl -u irium-node -n 20
ls ~/.irium/blocks/ | wc -l
```

- Ensure you’re on the main branch (not gh-pages):
```bash
git branch --show-current  # should be: main
```

- Check miner log shows “Starting mining loop” / “Nonce:” / “Hashrate:”
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

## Updated Mining Commands (v1.0)

```bash
cd ~/irium
python3 -m venv .venv && . .venv/bin/activate
export PYTHONPATH="$PWD"
pip install pycryptodome qrcode pillow requests

nohup python3 -u scripts/irium-node.py 38291 > /tmp/node.log 2>&1 &
export IRIUM_WALLET_FILE="$HOME/.irium/irium-wallet.json"
python3 -u scripts/irium-miner.py 38292

# Multicore
export IRIUM_WALLET_FILE="$HOME/.irium/irium-wallet.json"
bash scripts/irium-miner-multicore.sh 4
```

Tips:
- Miner port is positional (no --port).
- If logs look empty, use: `tail -n 120 /tmp/miner-38292.log | tr '\r' '\n'`
