# Irium v1.3.0 - Consensus Hard Fork & Critical Fixes

**Release Date:** October 25, 2025  
**Type:** Major Release (Hard Fork)  
**Status:** ⚠️ BREAKING CHANGES - All nodes must upgrade

---

## 🚨 BREAKING CHANGES (Consensus Hard Fork)

This release implements critical consensus rules from the Irium whitepaper:

1. **Coinbase Maturity Enforcement (100 blocks)**
   - Coinbase outputs cannot be spent until 100 confirmations
   - Prevents premature spending of mining rewards
   - Whitepaper Section 3.3 compliance

2. **Timestamp Validation**
   - Blocks cannot be more than 2 hours in the future
   - Block timestamps must be strictly increasing
   - Prevents timestamp manipulation attacks

3. **Transaction Signature Verification**
   - All non-coinbase transactions now verify cryptographic signatures
   - Enhanced security and transaction validity

4. **UTXO Height Tracking**
   - UTXOs now track creation height and coinbase status
   - Enables maturity-based validation rules

---

## 🐛 Bug Fixes

### Mining
- **Nonce Overflow Protection**: Fixed integer overflow in miners when nonce exceeds 4.29 billion
- **Graceful Shutdown**: Miners now handle SIGTERM/SIGINT properly

### P2P Networking
- **Memory Leak Fix**: `relayed_blocks` dictionary now limited to last 100 blocks
- **Task Cancellation**: Improved asyncio task cleanup on peer disconnect
- **Exception Handling**: Replaced 5 bare `except:` clauses with proper error handling
- **Dead Seed Cleanup**: Removed unreachable node from seedlist

### Security
- **Wallet Permissions**: Changed wallet file permissions to 600 (owner-only access)

### API
- **Explorer Stats**: Fixed `/api/stats` endpoint (changed 'time' to 'timestamp')

---

## 📦 Installation

### New Installation
```bash
wget https://github.com/iriumlabs/irium/releases/download/v1.3.0/irium-v1.3.0-complete.tar.gz
sha256sum -c irium-v1.3.0-complete.tar.gz.sha256
tar -xzf irium-v1.3.0-complete.tar.gz
cd irium-v1.3.0
```

### Upgrade from v1.2.0 or earlier

⚠️ **CRITICAL**: This is a hard fork. All nodes must upgrade simultaneously.

```bash
# Stop all services
sudo systemctl stop irium-node.service irium-miner.service

# Backup your wallet
cp ~/.irium/irium-wallet.json ~/.irium/irium-wallet.json.backup

# Pull latest code
cd /home/irium/irium
git fetch origin
git checkout v1.3.0

# Clear chainstate (will rebuild from blocks)
rm -rf ~/.irium/chainstate/*

# Restart services
sudo systemctl start irium-node.service
sudo systemctl start irium-miner.service

# Verify
sudo journalctl -u irium-node.service -f
```

---

## 🔒 Security Improvements

- **Wallet File**: Now requires 600 permissions (owner read/write only)
- **Signature Verification**: All transactions cryptographically verified
- **Timestamp Validation**: Prevents time-based attacks
- **Better Error Handling**: No more silent failures

---

## 📊 Technical Details

### Files Modified
- `irium/chain.py`: +83 lines (UTXOEntry, validation logic)
- `irium/constants.py`: +4 lines (new consensus constants)
- `irium/__init__.py`: +1 line (export UTXOEntry)
- `irium/p2p.py`: Memory leak fix, exception handling
- `irium/block.py`: Nonce overflow protection
- `scripts/irium-miner.py`: Nonce overflow, graceful shutdown
- `scripts/irium-simple-miner.py`: Nonce overflow, signal handlers
- `scripts/irium-explorer-api.py`: Stats endpoint fix
- `bootstrap/seedlist.txt`: Dead seed removed

### Commits Included
- `ef5d6a2`: Add seedlist.runtime to .gitignore
- `e7b1c9f`: Remove dead seed 106.219.158.52
- `8326d34`: CONSENSUS HARD FORK implementation
- `2721b7f`: Critical bugfixes (nonce, memory, exceptions)

---

## ⚠️ Upgrade Requirements

**ALL NODES MUST UPGRADE** to avoid chain split.

### Coordination
- Coordinate upgrade time with all miners
- Expected downtime: ~5 minutes per node
- Chainstate will rebuild from block files

### Compatibility
- ❌ Not compatible with v1.2.0 or earlier
- ✅ All nodes on v1.3.0 will sync correctly

---

## 🧪 Testing

Tested on:
- Ubuntu 22.04 LTS
- Python 3.10+
- 50 blocks of blockchain data
- 2-node network

---

## 📝 Changelog

### Added
- Coinbase maturity enforcement (100 blocks)
- Timestamp validation (2h max future)
- Transaction signature verification
- UTXO height tracking
- Signal handlers for graceful shutdown

### Fixed
- Nonce overflow in miners
- P2P memory leak
- Bare except clauses
- Asyncio task cancellation
- Explorer API stats bug
- Dead seed in seedlist

### Changed
- UTXO structure: TxOutput → UTXOEntry
- Wallet permissions: 664 → 600

### Security
- Enhanced transaction validation
- Timestamp manipulation prevention
- Wallet file access control

---

## 🔗 Links

- **GitHub**: https://github.com/iriumlabs/irium
- **Whitepaper**: https://github.com/iriumlabs/irium/blob/main/WHITEPAPER.md
- **Issues**: https://github.com/iriumlabs/irium/issues

---

## 👥 Contributors

Special thanks to all contributors who helped identify and fix these issues.

---

**Full Changelog**: https://github.com/iriumlabs/irium/compare/v1.2.0...v1.3.0
