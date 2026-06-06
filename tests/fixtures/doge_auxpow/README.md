# DOGE AuxPoW Test Fixtures

Real Dogecoin mainnet block data for testing AuxPoW validation.
Captured 2026-06-06 from api.blockchair.com/dogecoin/raw/block/<height>.

## Files

- block_371336.json — Last pre-AuxPoW DOGE block (standalone Scrypt PoW)
- block_371337.json — First AuxPoW DOGE block (LTC merge-mined, full AuxPoW data)
- block_371338.json — Second AuxPoW DOGE block (different LTC parent, tests no parent caching)
- block_5000000.json — Modern AuxPoW block (BIP9 parent version, longer merkle branches)

## What these test

- Transition at DOGE block 371,337 (pre/post AuxPoW boundary)
- AuxPoW commitment magic: fabe6d6d
- Coinbase merkle branch verification
- Blockchain (chain) merkle branch verification
- Scrypt PoW on LTC parent header
- Different LTC parents for adjacent DOGE blocks
- Modern LTC parent format (BIP9 version bits)

## Source API

api.blockchair.com/dogecoin/raw/block/<height>
Returns decoded_raw_block with full AuxPoW structure.
