# Understanding Your IRM Balance

## How Blockchain Balances Work

Your balance = Blocks YOU mined (not blocks you synced from others)

### Example:

**You start mining at 14:16 on Oct 18**

When you sync the blockchain, you get all existing blocks (2-8).
These blocks were mined by OTHER people before you started.

**Blocks 2-8:** Synced from network = 0 IRM for you
**Blocks 9-10:** YOU mined them = 100 IRM for you! ✅

## How to Verify Your Balance

### 1. Check when YOU started mining
```bash
ps -p <your_miner_PID> -o lstart
```

### 2. Check when blocks were created
```bash
ls -lt ~/.irium/blocks/
```

### 3. Match timestamps

Any block created AFTER you started mining = YOURS!

Example:
- Started mining: 14:16
- block_9.json created: 16:09 ✅ YOURS (50 IRM)
- block_10.json created: 16:39 ✅ YOURS (50 IRM)
- **Total: 100 IRM**

## Current Limitation

The `check-balance.py` script doesn't parse the coinbase transaction yet.

It can't automatically tell which blocks are yours vs synced.

**For now:** Manually check block creation times vs your mining start time.

**Coming in v1.1.0:** Full UTXO scanning with automatic address matching.

## Your Mining Address

Check your mining address:
```bash
cat ~/.irium/irium-wallet.json
```

This address receives rewards when you mine blocks.

## Summary

✅ If you mined blocks 9 & 10: You have 100 IRM
✅ Keep mining: You'll earn 50 IRM per block
✅ With 14 cores: You'll mine MANY more blocks!

Happy mining! ⛏️
