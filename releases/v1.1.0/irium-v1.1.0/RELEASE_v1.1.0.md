# Irium v1.1.0 Release Notes

**Release Date:** October 26, 2025  
**Status:** Stable  
**Network:** irium-mainnet

## 🔒 Security Enhancements

### Critical Fix: Difficulty Bits Validation
- **Issue:** Block headers' difficulty bits were not validated against expected values
- **Impact:** Miners could potentially set arbitrary difficulty bits
- **Fix:** Added validation in irium/chain.py to enforce correct difficulty bits per block height
- **Status:** ✅ Deployed and verified

## 🐛 Bug Fixes

- Fixed Explorer API timestamp field error (changed to time)
- Fixed Explorer API supply_irm field (was showing null)
- Updated all version strings to 1.1.0
- Consistent agent strings across node and miner

## ✅ Verification Results

All parameters verified against whitepaper specifications:

- **Mining Difficulty:** 1.0 (Bitcoin-style base difficulty)
- **Block Time:** 600 seconds (10 minutes)
- **Retarget Interval:** 2016 blocks (~14 days)
- **Adjustment Limits:** 0.25x to 4x (clamped)
- **Max Supply:** 100,000,000 IRM
- **Block Reward:** 50 IRM (halving every 210,000 blocks)
- **Coinbase Maturity:** 100 blocks
- **Genesis Vesting:** 3,500,000 IRM

## 🔧 Technical Changes

### Modified Files:
- irium/chain.py - Added bits validation
- irium/__init__.py - Version bump to 1.1.0
- irium/p2p.py - Updated agent string and version check
- scripts/irium-node.py - Updated agent string
- scripts/irium-miner.py - Updated agent string
- scripts/irium-explorer-api.py - Fixed timestamp and supply_irm fields

### Genesis Block:
Hash: cbdd1b9134adc846b3af5e2128f68214e1d8154912ff8da40685f47700000000
Time: 1735689601
Bits: 0x1d00ffff

## 📊 Current Network Stats

- **Height:** 49 blocks
- **Supply:** 2,200 IRM mined
- **Status:** Stable and operational

---

**Status:** Production-ready ✅
