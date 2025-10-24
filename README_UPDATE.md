## 🚀 Quick Start (v1.1.8)

### 1. Download & Install

```bash
# Download latest release
wget https://iriumlabs.org/releases/v1.1.8/irium-bootstrap-v1.1.8.tar.gz

# Extract
tar -xzf irium-bootstrap-v1.1.8.tar.gz
cd irium-bootstrap-v1.1.8

# Install
chmod +x install.sh
./install.sh
```

### 2. Start Node

```bash
# Start as service (recommended)
sudo systemctl start irium-node
sudo systemctl enable irium-node

# Check status
sudo journalctl -u irium-node -f
```

### 3. Create Wallet

```bash
python3 scripts/irium-wallet-proper.py create
# Save your address - mining rewards go here!
```

### 4. Start Mining

```bash
# Single-core
sudo systemctl start irium-miner
sudo systemctl enable irium-miner

# Multi-core (4 cores)
bash scripts/irium-miner-multicore.sh 4
```

### 5. Check Status

```bash
# Node status
sudo journalctl -u irium-node -n 20

# Mining progress
sudo journalctl -u irium-miner -n 20

# Blockchain height
ls ~/.irium/blocks/ | wc -l
```

## ⚡ Update Existing Installation

```bash
cd ~/irium
git pull origin main
sudo systemctl restart irium-node
sudo systemctl restart irium-miner
```

