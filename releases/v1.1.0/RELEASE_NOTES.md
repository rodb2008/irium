# Irium v1.1.0 - Accurate Balance Tracking 💰

## Major Feature: True Balance Detection ✅

### Miner Address in Blocks
- Every mined block now includes the miner's address
- Balance checker accurately detects which blocks are yours
- No more guessing based on timestamps

## What's New

✅ **Miner address tracking** - Every block now records who mined it
✅ **Accurate balance checker (v2)** - Shows YOUR blocks vs others' blocks
✅ **Auto-sync** - Nodes rescan every 30 seconds (from v1.0.10)
✅ **Multi-core mining** - Full support for 1-16+ cores
✅ **Balance guide** - Complete documentation

## Network Status

- **Height:** 10+ blocks
- **Supply:** 450+ IRM circulating
- **Miners:** Multiple active nodes
- **Status:** Fully operational

## Update Instructions

```bash
# Backup wallet
cp ~/.irium/irium-wallet.json ~/wallet-backup.json

# Download v1.1.0
cd ~
wget https://github.com/iriumlabs/irium/releases/download/v1.1.0/irium-bootstrap-v1.1.0.tar.gz
tar -xzf irium-bootstrap-v1.1.0.tar.gz
cd irium-bootstrap-v1.1.0
./install.sh

# Restore wallet
cp ~/wallet-backup.json ~/.irium/irium-wallet.json

# Start
python3 scripts/irium-node.py &
./scripts/irium-miner-multicore.sh 4 &

# Check YOUR accurate balance
python3 scripts/check-balance-v2.py
```

---

Download: https://github.com/iriumlabs/irium/releases/tag/v1.1.0
Website: https://iriumlabs.org
