# Irium v1.1.5 - Critical P2P Broadcast Fix

## 🚨 CRITICAL FIX - All miners must upgrade immediately

This release fixes a critical bug that prevented miners from broadcasting blocks to the network, causing chain forks.

## 🔧 Critical Fix

### Mining Loop Blocking Event Loop
- **Problem**: The mine_block() function was synchronous, blocking asyncio event loop
- **Impact**: P2P background tasks couldn't run while mining
- **Result**: Miners mined blocks locally but never broadcast them
- **Fix**: Made mine_block() async with await asyncio.sleep(0) every 10k hashes
- **Benefit**: Event loop yields regularly, P2P tasks run, blocks broadcast successfully

## ⚠️ Upgrade Instructions

All miners must upgrade to prevent forks:

```bash
cd ~/irium
git pull origin main
pkill -f irium-miner
python3 scripts/irium-miner.py &
```

## 🎯 Testing Confirmed

- Miner connects to seed node successfully
- P2P background tasks run every 30 seconds
- Mining continues at ~200k H/s
- Event loop no longer blocked
- Blocks will broadcast when found

## 📦 Download

**Bootstrap Package**: irium-bootstrap-v1.1.5.tar.gz


