# Mining Setup Guide

## How to Start Mining with Your Address

### Quick Start (Recommended)

```bash
# 1. Create your wallet address
python3 scripts/irium-wallet-proper.py new-address

# This creates:
# - New address: Q1abc...xyz (where your rewards go)
# - Private key: Saved in ~/.irium/irium-wallet.json
# - IMPORTANT: Backup your wallet file!

# 2. Start mining
python3 scripts/irium-miner.py

# You'll see:
# 💰 Mining address: Q1abc...xyz
# ⛏️  Mining block 4...

# 3. Mining rewards will go to your address!
```

### Verify Your Mining Address

```bash
# Check what address the miner is using
journalctl -u irium-miner.service | grep "Mining address" | tail -1

# Or if running manually:
# Look for "💰 Mining address: Q..."
```

### Important: Wallet is Loaded at Startup

**The miner loads your wallet when it starts!**

If you create a new address AFTER the miner started:
```bash
# 1. Stop miner
sudo systemctl stop irium-miner.service

# 2. Create new address
python3 scripts/irium-wallet-proper.py new-address

# 3. Restart miner to use new address
sudo systemctl start irium-miner.service

# 4. Verify
sudo journalctl -u irium-miner.service -n 20 | grep "Mining address"
```

### Managing Multiple Addresses

```bash
# List all your addresses
python3 scripts/irium-wallet-proper.py list

# Check balance
python3 scripts/irium-wallet-proper.py balance

# The miner uses the FIRST address in your wallet
```

### Starting Fresh with New Address

```bash
# 1. Backup existing wallet (optional)
cp ~/.irium/irium-wallet.json ~/.irium/wallet-backup.json

# 2. Remove old wallet
rm ~/.irium/irium-wallet.json

# 3. Create new address
python3 scripts/irium-wallet-proper.py new-address

# 4. Start mining
python3 scripts/irium-miner.py
# OR
sudo systemctl restart irium-miner.service
```

### Backup Your Wallet!

**CRITICAL: Always backup your wallet file!**

```bash
# Backup wallet
cp ~/.irium/irium-wallet.json ~/irium-wallet-backup-$(date +%Y%m%d).json

# Or copy to safe location
scp ~/.irium/irium-wallet.json user@backup-server:~/irium-backups/
```

**Without your wallet file, you LOSE ACCESS to your mining rewards!**

### Troubleshooting

**"Miner is using a different address than I created"**

This means the miner was already running. Restart it:
```bash
sudo systemctl restart irium-miner.service
```

**"I want to mine to multiple addresses"**

The miner uses one address at a time. To switch:
1. Stop miner
2. Edit wallet or create new one
3. Restart miner

**"How do I check my mining rewards?"**

```bash
python3 scripts/irium-wallet-proper.py balance
```

Rewards are spendable after 100 block confirmations (coinbase maturity).

### Mining Tips

- ✅ Always backup your wallet
- ✅ Write down your address and WIF
- ✅ Verify mining address after starting miner
- ✅ Keep your system updated (git pull regularly)
- ✅ Monitor miner logs for errors

### Need Help?

- GitHub Issues: https://github.com/iriumlabs/irium/issues
- Discussions: https://github.com/iriumlabs/irium/discussions
- Email: info@iriumlabs.org
