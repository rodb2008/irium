# Irium v1.0.8 - Critical Hotfix

## Critical Bugs Fixed

### Fixed: Nonce Overflow
Miner crashed when nonce exceeded 2^32. Now wraps at 2^32 and updates timestamp.

### Fixed: Wrong prev_hash
Miner used genesis hash instead of tip block. Now reads from disk correctly.

**Block 4 was invalidated. All miners must update immediately.**

## Upgrade: git pull origin main && sudo systemctl restart irium-miner.service

## Documentation Updates
✅ Added blockchain sync explanation to prevent confusion
✅ Clarified that same height = already in sync
