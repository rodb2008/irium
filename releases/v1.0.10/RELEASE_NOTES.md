# Irium v1.0.10 - Auto-Sync Fix 🔄

## Critical Fix ✅

### **Automatic Block Detection**
- Node automatically rescans for new blocks every 30 seconds
- No more manual restarts needed after mining
- Peers stay synchronized automatically
- Both node and miner benefit from auto-detection

**This fixes the issue where:**
- Nodes would stay at old heights
- Manual restarts were required to see new blocks
- Peers couldn't sync properly

## What's Included

✅ Automatic block rescanning (every 30s)
✅ Multi-core mining support
✅ Improved balance checker
✅ Better peer synchronization
✅ All previous bug fixes from v1.0.9

## Network Status

- **Mainnet Height:** 10 blocks
- **Total Supply:** 450 IRM circulating
- **Peers:** Multiple nodes connected
- **Status:** Fully operational

## Update Instructions

```bash
# 1. Backup wallet
cp ~/.irium/irium-wallet.json ~/wallet-backup.json

# 2. Download v1.0.10
cd ~
wget https://github.com/iriumlabs/irium/releases/download/v1.0.10/irium-bootstrap-v1.0.10.tar.gz
tar -xzf irium-bootstrap-v1.0.10.tar.gz
cd irium-bootstrap-v1.0.10

# 3. Install
./install.sh

# 4. Restore wallet
cp ~/wallet-backup.json ~/.irium/irium-wallet.json

# 5. Start node
python3 scripts/irium-node.py &

# 6. Start mining (choose cores)
./scripts/irium-miner-multicore.sh 4 &

# 7. Check sync
sleep 10
python3 scripts/check-balance.py
```

## Performance

- **Auto-rescan:** Every 30 seconds
- **Single-core mining:** ~210k H/s
- **4-core mining:** ~840k H/s
- **8-core mining:** ~1.68M H/s

## Files Included

- Complete blockchain node
- Multi-core mining wrapper
- Improved balance checker
- Auto-sync functionality
- All documentation

---

**Download:** https://github.com/iriumlabs/irium/releases/tag/v1.0.10

**Website:** https://iriumlabs.org

**Explorer:** http://207.244.247.86:8082
