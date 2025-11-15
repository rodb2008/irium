# Irium Quick Start (v1.0)

This guide walks through installing dependencies, running an Irium node, creating a wallet, and starting mining on a single VPS or workstation.

> For production deployments with systemd units and nginx frontends, see `docs/nginx-config.md` and `IRIUM_NETWORK_TRACKER.md`.

## 1. Install Dependencies

On a fresh Linux system:

```bash
sudo apt update
sudo apt install -y python3 python3-venv python3-pip

cd ~/irium
python3 -m venv .venv
. .venv/bin/activate
pip install -r requirements.txt
```

If you prefer installing only the minimal Python packages:

```bash
pip3 install --user pycryptodome qrcode pillow requests
```

## 2. Download & Install Bootstrap Bundle (optional convenience)

A pre‑packaged bootstrap tarball is available for some environments:

```bash
wget https://github.com/iriumlabs/irium/releases/download/v1.0/irium-bootstrap-v1.0.tar.gz
tar -xzf irium-bootstrap-v1.0.tar.gz
cd irium-bootstrap-v1.0
chmod +x install.sh
./install.sh
```

This helper installs the repository, systemd units, and nginx snippets appropriate for a typical VPS deployment.

## 3. Start the Node

### Via systemd (recommended on servers)

```bash
sudo systemctl start irium-node
sudo systemctl enable irium-node
sudo journalctl -u irium-node -f
```

### Directly from the repository (development)

```bash
cd ~/irium
. .venv/bin/activate  # if using a venv
export PYTHONPATH="$PWD"
python3 scripts/irium-node.py 38291
```

## 4. Create a Wallet

Use the wallet CLI to create a wallet file and at least one address:

```bash
cd ~/irium
. .venv/bin/activate
export PYTHONPATH="$PWD"

python3 scripts/irium-wallet-proper.py create-wallet
python3 scripts/irium-wallet-proper.py new-address
python3 scripts/irium-wallet-proper.py balance
```

Wallet data is stored at `~/.irium/irium-wallet.json`. Back this file up securely before mining.

## 5. Start Mining

### Single‑core miner (with full P2P)

```bash
cd ~/irium
. .venv/bin/activate
export PYTHONPATH="$PWD"
export IRIUM_WALLET_FILE="$HOME/.irium/irium-wallet.json"

nohup python3 -u scripts/irium-node.py 38291 > /tmp/node-38291.log 2>&1 &
python3 -u scripts/irium-miner.py 38292
```

### Multicore miner (with full P2P)

```bash
cd ~/irium
. .venv/bin/activate
export PYTHONPATH="$PWD"
export IRIUM_WALLET_FILE="$HOME/.irium/irium-wallet.json"

nohup python3 -u scripts/irium-node.py 38291 > /tmp/node-38291.log 2>&1 &
bash scripts/irium-miner-multicore.sh 4
./scripts/tail-mining-logs.sh 4 38292
```

### Miner port usage

- The reference miner expects the P2P port as a **positional argument**:

  ```bash
  # Default (38292)
  python3 scripts/irium-miner.py

  # Explicit port (example 39292)
  python3 scripts/irium-miner.py 39292
  ```

- If you prefer a CLI flag, use the individual miner:

  ```bash
  python3 scripts/irium-miner-individual.py \
    --wallet "$HOME/.irium/irium-wallet.json" \
    --port 39292
  ```

## 6. Status & Troubleshooting

### Basic checks

```bash
# Node logs (systemd)
sudo journalctl -u irium-node -n 20

# Number of blocks stored locally
ls ~/.irium/blocks/ | wc -l
```

Ensure you are on the main branch:

```bash
cd ~/irium
git branch --show-current   # should print: main
```

### Miner logs

If you run miners with `nohup`, logs typically go to `/tmp/miner-<port>.log`. For example:

```bash
tail -n 120 /tmp/miner-38292.log | tr '\r' '\n'
```

If specifying a port “doesn’t work”:

- Remove any extra parentheses or shell prompt decorations if copying from docs.
- Try a different free port:

  ```bash
  python3 scripts/irium-miner.py 40292
  ```

- Or switch to the individual miner:

  ```bash
  python3 scripts/irium-miner-individual.py \
    --wallet "$HOME/.irium/irium-wallet.json" \
    --port 39292
  ```

## 7. Updated Mining Commands (v1.0)

End‑to‑end example on a fresh clone:

```bash
cd ~/irium
python3 -m venv .venv && . .venv/bin/activate
export PYTHONPATH="$PWD"
pip install -r requirements.txt

nohup python3 -u scripts/irium-node.py 38291 > /tmp/node.log 2>&1 &
export IRIUM_WALLET_FILE="$HOME/.irium/irium-wallet.json"
python3 -u scripts/irium-miner.py 38292

# Multicore
export IRIUM_WALLET_FILE="$HOME/.irium/irium-wallet.json"
bash scripts/irium-miner-multicore.sh 4
```

Tips:

- Miner port is positional (no `--port` flag on `irium-miner.py`).
- If logs appear empty, make sure you are tailing the right file and replacing carriage returns as shown above.

