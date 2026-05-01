# Irium Merged Mining Guide

## Algorithm

Irium uses **SHA-256d** — the same double-SHA256 hashing algorithm as Bitcoin. The proof-of-work function is identical byte for byte:

```
block_hash = SHA256(SHA256(80-byte-header))
```

This means every SHA-256d ASIC and every SHA-256d software miner in existence can mine Irium blocks without any hardware or firmware change.

## What Is Merged Mining?

Merged mining (AuxPoW) is a protocol that lets a miner solve a single SHA-256d puzzle that satisfies the difficulty requirements of two or more chains simultaneously. A Bitcoin ASIC mining Bitcoin would also solve Irium blocks at zero extra energy cost if both chains implement the AuxPoW protocol.

## AuxPoW Status in Irium

**AuxPoW is not currently implemented in Irium.**

The block format is the standard 80-byte Bitcoin header:

```
4 bytes  — version
32 bytes — previous block hash
32 bytes — merkle root
4 bytes  — timestamp
4 bytes  — bits (compact target)
4 bytes  — nonce
```

There is no AuxPoW extension. The node accepts only vanilla SHA-256d headers. Merged mining submissions that embed an Irium block inside a Bitcoin coinbase transaction are not valid and will be rejected.

The source confirming this is `src/pow.rs` (the `sha256d` function) and `src/block.rs` (the `BlockHeader` struct with the 80-byte layout). Neither file contains any AuxPoW parent block structure, coinbase branch, or chain merkle branch.

## What This Means for Miners

**Any SHA-256d ASIC can mine Irium today.** You point your ASIC at an Irium Stratum pool and it mines Irium as a standalone chain, the same way you mine any other SHA-256d chain that does not support AuxPoW. You earn the block reward on the Irium chain when you find a valid block.

**Mining Irium alongside Bitcoin simultaneously is not yet possible.** That requires AuxPoW, which is a planned future addition. When AuxPoW is added, this guide will be updated with the specific coinbase encoding required.

## SHA-256d Compatibility

Because Irium uses the same algorithm as Bitcoin, hardware compatibility is complete:

| Hardware | Status |
|----------|--------|
| Antminer S19 / S21 | Full compatibility — point at pool.iriumlabs.org:3333 |
| Whatsminer M50 / M60 | Full compatibility |
| Any Bitmain / MicroBT ASIC | Full compatibility |
| Any SHA-256d USB miner | Full compatibility |
| cpuminer-opt (software) | Full compatibility — use sha256d algorithm flag |
| CGMiner / BFGMiner | Full compatibility — use sha256d |

## Chain Parameters for Pool Operators

| Parameter | Value |
|-----------|-------|
| Algorithm | SHA-256d |
| Block time target | 600 seconds |
| Difficulty algorithm | LWMA (60-block window) |
| Max difficulty retarget per block | 2× in either direction |
| Block reward (current) | 50 IRM |
| Halving interval | 210,000 blocks |
| Max supply | 100,000,000 IRM |
| Coinbase maturity | 100 blocks |
| P2P port (default) | 38291 |
| RPC port (default) | 38300 |
| Block header format | Standard Bitcoin 80-byte |

## Connecting to the Public Pool

The official public Irium pool uses SOLO payout — the full coinbase reward goes to the miner address in the worker username.

```
ASIC:      stratum+tcp://pool.iriumlabs.org:3333
CPU/GPU:   stratum+tcp://pool.iriumlabs.org:3335
Username:  YOUR_IRIUM_ADDRESS.worker1
Password:  x
```

## Running Your Own Node for Direct Mining

To mine solo without a pool, run iriumd and point a SHA-256d getblocktemplate miner at the local node:

```bash
# Requires IRIUM_RPC_TOKEN if set
GET http://localhost:38300/rpc/getblocktemplate
POST http://localhost:38300/rpc/submit_block
```

The getblocktemplate response contains `height`, `prev_hash`, `bits`, `target`, `time`, `txs`, and `coinbase_value`. See `docs/API.md` for the full field reference.

## Path to AuxPoW

When AuxPoW is implemented, this guide will cover:

1. The version flag required in the block header to signal AuxPoW
2. The exact coinbase encoding to embed the Irium block hash
3. Configuration for pool software to submit AuxPoW blocks
4. How to contact pool operators to add Irium to an existing SHA-256d merge-mining pool

Pool operators interested in adding Irium once AuxPoW is available should register interest on the GitHub repository or in the Telegram group.

## Contact

For pool integration questions: Telegram — https://t.me/iriumlabs
For technical questions: GitHub Issues — https://github.com/iriumlabs/irium
