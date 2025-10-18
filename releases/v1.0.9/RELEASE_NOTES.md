# Irium v1.0.9 - Complete Fix Release 🔧

## All Critical Bugs Fixed

✅ **Nonce overflow** - Miner wraps at 2^32 and updates timestamp
✅ **Wrong prev_hash** - Reads from actual tip block on disk  
✅ **Block sync** - Intelligently serves blocks based on peer height
✅ **Peer height tracking** - Updates when receiving blocks, auto-requests if behind
✅ **Random mining address** - Miner saves new address to wallet file
✅ **Wallet path bug** - Wallet script uses correct ~/.irium path

## Tested & Verified

✅ Sync tested - works 100%
✅ Mining address persists across restarts
✅ Wallet saves addresses correctly
✅ Miner uses correct prev_hash
✅ Network stable

## Mandatory Update

**All users must upgrade to v1.0.9!**

Previous block 4s are invalid. After updating, delete corrupted blocks:
```bash
rm -f ~/.irium/blocks/block_4.json
```

## Upgrade

```bash
cd irium-bootstrap-*
git pull origin main
rm -f ~/.irium/blocks/block_4.json
sudo systemctl restart irium-node.service
sudo systemctl restart irium-miner.service
```

Or fresh install: https://github.com/iriumlabs/irium/releases/download/v1.0.9/irium-bootstrap-v1.0.9.tar.gz

---

**v1.0.9 is production ready!**
