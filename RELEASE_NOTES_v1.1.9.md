# Irium v1.1.9 Release Notes

**Release Date:** October 22, 2025
**Status:** Production Ready

## 🎯 Major Improvements

### 1. PUSH-Based Block Broadcasting
- Blocks now automatically propagate to all connected peers
- Solves sync issues for NAT peers
- Bidirectional PUSH (works when ahead OR behind)

### 2. Stable P2P Connections
- Fixed ghost peer bug (address correction)
- Fixed duplicate message handlers
- Connections now stable for extended periods

### 3. NAT Traversal Support
- Adaptive ping interval: 60s (public nodes), 30s (NAT nodes)
- Automatic keepalive for NAT connections
- Full compatibility with firewalled miners

### 4. Production Optimization
- Peer timeout: 180 seconds (allows 3 pings)
- Message timeout: 180 seconds
- Max peers: 8000 (production scale)
- Clean, readable logs (95% noise reduction)

### 5. Code Quality
- Removed all hardcoded IPs/paths
- Generic code works for all miners
- Complete protocol implementation (13 message types)
- Proper error handling

## 🔧 Technical Changes

**Files Modified:**
- `irium/p2p.py` - 645 lines (7 major fixes)
- `irium/protocol.py` - Added GetHeaders, Headers, Mempool messages
- `irium/network.py` - Relative path for .env
- `scripts/irium-miner.py` - max_peers=8000
- `WHITEPAPER.md` - Updated with NAT documentation

**Fixes Applied:**
1. PUSH broadcasting in _handle_block
2. Bidirectional PUSH on connection
3. IP-based peer deduplication
4. Ghost peer bug fixed
5. Message handler duplication fixed
6. Adaptive ping interval (NAT support)
7. Production-optimized timeouts

## 🧪 Test Results

- ✅ Stable connections: 10+ minutes continuous
- ✅ Block propagation: Tested and working
- ✅ NAT traversal: 30s keepalive successful
- ✅ No hardcoded values: Portable code
- ✅ Clean logs: Minimal noise

## 📦 Installation

```bash
git pull origin main
sudo systemctl restart irium-node
sudo systemctl restart irium-miner
```

## 🔄 Upgrade from v1.1.8

All changes are backward compatible. Simply pull and restart services.

---

**Download:** https://github.com/iriumlabs/irium/releases/tag/v1.1.9
