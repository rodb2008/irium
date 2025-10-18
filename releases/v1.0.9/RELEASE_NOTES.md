# Irium v1.0.9 - Complete Fix Release

## All Critical Bugs Fixed

✅ Nonce overflow - Wraps at 2^32
✅ Wrong prev_hash - Reads from disk correctly
✅ Block sync - Working 100%
✅ Wallet persistence - Addresses save correctly
✅ Block validation - Rejects invalid blocks
✅ Infinite loop - Fixed request loops
✅ Balance checker - New utility script

## Update Instructions

```bash
cd irium-bootstrap-*
git pull origin main
rm -f ~/.irium/blocks/block_4.json
sudo systemctl restart irium-node.service
sudo systemctl restart irium-miner.service
```

Download: https://github.com/iriumlabs/irium/releases/download/v1.0.9/irium-bootstrap-v1.0.9.tar.gz

**All users must update!**
