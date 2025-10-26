# Irium v1.1.0 - Security & Auto-Update Release

**Release Date:** October 26, 2025  
**Status:** ✅ Production Stable  
**Network:** irium-mainnet  
**Security Audit:** 20/20 Passed (100%)

---

## 🔒 Security Enhancements

### Critical Fix: Difficulty Bits Validation
- **Issue:** Block headers' difficulty bits were not validated against expected values
- **Impact:** Miners could potentially set arbitrary difficulty bits
- **Fix:** Added validation in `irium/chain.py` to enforce correct difficulty bits per block height
- **Status:** ✅ Deployed and verified

### New: Rate Limiting Protection
- **Module:** `irium/rate_limiter.py` (NEW)
- **Feature:** Token bucket rate limiter for API endpoints
- **Limit:** 120 requests per minute per IP
- **Protection:** Prevents Denial-of-Service (DoS) attacks

### New: Protocol Message Limits
- **Module:** `irium/protocol.py`
- **Max Message Size:** 32 MB
- **Max Block Size:** 4 MB
- **Protection:** Prevents network abuse and memory attacks

---

## 🆕 New Features

### Auto-Update Notification System
- **Module:** `irium/update_checker.py` (NEW)
- **Feature:** Automatically checks GitHub for new releases
- **Frequency:** Every 6 hours
- **Integration:** Built into node and miner startup
- **Benefit:** Keeps miners informed of security updates

### Auto-Update Script
- **Script:** `scripts/auto-update.sh` (NEW)
- **Feature:** Optional automated update with backup
- **Usage:** Can be scheduled via cron for automatic deployment

---

## 🐛 Bug Fixes

- Fixed Explorer API `timestamp` field error (changed to `time`)
- Fixed Explorer API `supply_irm` field (was showing `null`)
- Fixed P2P import syntax errors
- Updated all version strings to 1.1.0
- Consistent agent strings across node and miner

---

## ✅ Verification Results

All parameters verified against whitepaper specifications:

| Parameter | Value | Status |
|-----------|-------|--------|
| Mining Difficulty | 1.0 (Bitcoin-style base) | ✅ |
| Block Time | 600 seconds (10 minutes) | ✅ |
| Retarget Interval | 2016 blocks (~14 days) | ✅ |
| Adjustment Limits | 0.25x to 4x (clamped) | ✅ |
| Max Supply | 100,000,000 IRM | ✅ |
| Block Reward | 50 IRM (halving every 210k) | ✅ |
| Coinbase Maturity | 100 blocks | ✅ |
| Genesis Vesting | 3,500,000 IRM | ✅ |

---

## 🔧 Technical Changes

### Modified Files
- `irium/__init__.py` - Version bump to 1.1.0
- `irium/chain.py` - Added difficulty bits validation
- `irium/p2p.py` - Updated agent string and version check
- `irium/protocol.py` - Added message/block size limits
- `scripts/irium-node.py` - Updated agent string + auto-update
- `scripts/irium-miner.py` - Updated agent string + auto-update
- `scripts/irium-explorer-api.py` - Fixed API bugs + rate limiting

### New Files
- `irium/rate_limiter.py` - DoS protection module
- `irium/update_checker.py` - Auto-update notification system
- `scripts/auto-update.sh` - Automated update script
- `API_SECURITY_POLICY.md` - API security documentation
- `AUTO_UPDATE_README.md` - Update system guide
- `AUDIT_CERTIFICATION.md` - Security audit report

### Genesis Block
- **Hash:** `cbdd1b9134adc846b3af5e2128f68214e1d8154912ff8da40685f47700000000`
- **Time:** `1735689601`
- **Bits:** `0x1d00ffff`
- **Status:** Locked and verified

---

## 📊 Release Statistics

- **Total Files:** 156
- **Files Changed:** 82
- **Insertions:** 9,119 lines
- **Bootstrap Files:** 7 (all included)
- **Config Files:** 4 (genesis locked)
- **Python Modules:** 80 (2 new security modules)

---

## 🌐 Network Status

- **Height:** 49+ blocks
- **Supply:** 2,200+ IRM mined
- **External Peers:** 4+ active
- **Decentralization:** High
- **Auto-updates:** Active

---

## 📥 Installation & Upgrade

### New Installation
```bash
git clone https://github.com/iriumlabs/irium.git
cd irium
git checkout v1.1.0
# Follow README.md for setup
```

### Upgrade from v1.0.x
```bash
cd ~/irium
git pull origin main
sudo systemctl restart irium-node.service
sudo systemctl restart irium-miner.service
```

### Verify Update
```bash
python3 -c "import irium; print(irium.__version__)"
# Should output: 1.1.0
```

---

## 📚 Documentation

- [Auto-Update System Guide](AUTO_UPDATE_README.md)
- [API Security Policy](API_SECURITY_POLICY.md)
- [Security Audit Certification](AUDIT_CERTIFICATION.md)
- [Release Checklist](RELEASE_CHECKLIST.md)

---

## 🎯 Security Audit Summary

✅ **Audit Score:** 20/20 checks passed (100%)  
✅ **Critical Issues:** 0  
✅ **Warnings:** 0 (after fixes)  
✅ **Code Quality:** Excellent  
✅ **Production Ready:** Certified

**Audited Components:**
- Consensus security (difficulty, timestamps, ordering)
- Supply controls (max supply, coinbase maturity, rewards)
- Transaction validation (double-spend, UTXO, signatures)
- P2P network security (version checks, peer limits)
- Genesis block integrity
- Fork prevention mechanisms
- API security (rate limiting, CORS)

---

## ⚠️ Breaking Changes

None. This release is fully backward compatible with v1.0.x.

---

## 🙏 Acknowledgments

Special thanks to all miners and node operators for testing and feedback during development.

---

**Status:** ✅ Production-ready and recommended for all users

**Download:** [Source Code (zip)](https://github.com/iriumlabs/irium/archive/refs/tags/v1.1.0.zip) | [Source Code (tar.gz)](https://github.com/iriumlabs/irium/archive/refs/tags/v1.1.0.tar.gz)
