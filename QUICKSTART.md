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
