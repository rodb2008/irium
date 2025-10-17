# Irium Quick Start Guide

## Installation

```bash
git clone https://github.com/iriumlabs/irium.git
cd irium
pip3 install --user pycryptodome qrcode pillow
```

## Commands

**Create Wallet:**
```bash
python3 scripts/irium-wallet-proper.py new-address
```

**Run Node:**
```bash
python3 scripts/irium-node.py
```

**Start Mining:**
```bash
python3 scripts/irium-miner.py
```

**Check Balance:**
```bash
python3 scripts/irium-wallet-proper.py balance
```

## Resources
- Website: https://www.iriumlabs.org
- Whitepaper: https://www.iriumlabs.org/whitepaper.html
- Telegram: https://t.me/iriumlabs

## Wallet Management

### Creating Addresses

```bash
# Create a new address
python3 scripts/irium-wallet-proper.py new-address

# List all addresses
python3 scripts/irium-wallet-proper.py list

# Check balance
python3 scripts/irium-wallet-proper.py balance
```

### Important: Mining Address

**The miner uses the wallet that exists when it starts.**

If you create a new address while mining:
```bash
# Restart miner to use the new address
sudo systemctl restart irium-miner.service

# Verify it's using the new address
sudo journalctl -u irium-miner.service -n 20 | grep "Mining address"
```

