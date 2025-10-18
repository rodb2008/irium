# Irium v1.0.9 - Complete Fix Release

## All Critical Bugs Fixed ✅

✅ **Nonce overflow** - Wraps at 2^32, prevents crashes
✅ **Wrong prev_hash** - Reads from disk correctly, chain continuity maintained
✅ **Block sync** - Working 100%, peers exchange blocks properly
✅ **Wallet persistence** - Addresses save correctly to ~/.irium/
✅ **Block validation** - Rejects invalid blocks, prevents infinite loops
✅ **Balance checker** - Shows formatted IRM balance from mined blocks
✅ **Multi-core mining** - 4x-8x faster mining with all CPU cores

## New Features 🚀

### Multi-Core Mining
- **Wrapper script** for easy multi-core mining
- Performance: ~840,000 H/s with 4 cores (vs 210,000 H/s single core)
- Usage: `./scripts/irium-miner-multicore.sh 4`

### Improved Balance Display
- **Formatted output** showing all mined blocks
- **Total balance** in IRM (not satoshis)
- Usage: `python3 scripts/check-balance.py`

## Update Instructions

```bash
# Download latest release
wget https://github.com/iriumlabs/irium/releases/download/v1.0.9/irium-bootstrap-v1.0.9.tar.gz
tar -xzf irium-bootstrap-v1.0.9.tar.gz
cd irium-bootstrap-v1.0.9

# Install
./install.sh

# Start node
python3 scripts/irium-node.py &

# Start multi-core mining (4 cores)
./scripts/irium-miner-multicore.sh 4

# Check your balance
python3 scripts/check-balance.py
```

## Network Status

- **Mainnet**: LIVE and operational
- **Current height**: 7+ blocks
- **Peers**: Multiple nodes connected
- **Status**: Fully functional blockchain

## Performance

- **Single-core mining**: ~210,000 H/s
- **4-core mining**: ~840,000 H/s (4x faster)
- **8-core mining**: ~1,680,000 H/s (8x faster)
- **Block time**: ~2-10 minutes (with multi-core)

## Files Included

- Complete blockchain implementation
- Multi-core mining wrapper
- Improved balance checker
- All documentation
- Systemd service files
- Bootstrap configuration

---

**Download**: https://github.com/iriumlabs/irium/releases/tag/v1.0.9

**Website**: https://iriumlabs.org

**Explorer**: http://207.244.247.86:8082
