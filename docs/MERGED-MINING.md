# Irium Merged Mining Guide

## Algorithm

Irium uses **SHA-256d** — the same double-SHA256 hashing algorithm as Bitcoin. The proof-of-work function is identical byte for byte:

```
block_hash = SHA256(SHA256(80-byte-header))
```

Every SHA-256d ASIC and every SHA-256d software miner in existence can mine Irium blocks without any hardware or firmware change.

## What Is Merged Mining?

Merged mining (AuxPoW — Auxiliary Proof of Work) is a protocol that lets a miner solve a single SHA-256d puzzle that simultaneously satisfies the difficulty requirement of two or more chains. A Bitcoin ASIC mining Bitcoin can also mine Irium blocks at zero additional energy cost. The same hashing work counts for both chains.

The protocol is based on the Namecoin AuxPoW standard. Bitcoin implements it by embedding an Irium block commitment in the parent coinbase transaction. The Irium node validates the proof by verifying the commitment and the parent block hash against the Irium target.

## AuxPoW Status in Irium

**AuxPoW is implemented and activates at block height 26,347.**

As of block height 20,299 (current as of May 2026), approximately 6,048 blocks remain until activation. At the 10-minute target block interval, activation is expected around **12 June 2026**.

After activation, the Irium node accepts both standard blocks (vanilla 80-byte header, version bit 8 clear) and AuxPoW blocks (version bit 8 set, AuxPoW extension appended). Standard solo mining continues to work with no changes required.

Source: `src/activation.rs`
```rust
pub const MAINNET_AUXPOW_ACTIVATION_HEIGHT: Option<u64> = Some(26_347);
```

## How AuxPoW Works in Irium

### Block Version Flag

An Irium block signals AuxPoW by setting bit 8 (value 256) in the block version:

```
AUXPOW_VERSION_BIT = 1 << 8 = 0x00000100

Standard block:  version = 0x00000001 = 1
AuxPoW block:    version = 0x00000101 = 257
```

Source: `src/auxpow.rs`
```rust
pub const AUXPOW_VERSION_BIT: u32 = 1 << 8;
```

### Coinbase Commitment Format

The merged-mining commitment is embedded in the parent (Bitcoin) coinbase transaction script. The commitment is identified by a 4-byte magic prefix:

```
AUXPOW_COMMIT_MAGIC = [0xfa, 0xbe, 0x6d, 0x6d]
```

Commitment structure (44 bytes total):
```
Offset  Length  Field
0       4       Magic: 0xfa 0xbe 0x6d 0x6d
4       32      aux_hash: sha256d(80-byte Irium header)
36      4       chain_count: number of merged chains (1 = Irium only), little-endian
40      4       nonce: auxiliary nonce (0 for single-chain), little-endian
```

The `aux_hash` is computed over the Irium block header bytes in natural (wire) order:
```
aux_hash = sha256d(irium_header_80_bytes)
```

Source: `src/auxpow.rs` — `pub fn build_commitment(aux_hash, chain_count, nonce)`

### AuxPoW Wire Format

When an Irium block carries an AuxPoW proof, the following data is appended after the standard 80-byte header:

```
Field                   Size
coinbase_txn            varint + raw bytes (the full parent coinbase transaction)
parent_hash             32 bytes (sha256d of parent_header, informational)
coinbase_branch         varint count + count x 32 bytes (Merkle branch: coinbase to parent block root)
coinbase_branch_index   4 bytes LE (leaf position in the parent block Merkle tree)
blockchain_branch       varint count + count x 32 bytes (Merkle branch: aux_hash to committed root)
blockchain_branch_index 4 bytes LE (Irium chain index in the merge-mined set)
parent_header           80 bytes (the parent block header)
```

For a pool mining Irium as the only merged chain (the common case), both `coinbase_branch` and `blockchain_branch` are empty (count = 0) and `chain_count = 1`.

Source: `src/auxpow.rs` — `pub fn serialize(ap: &AuxPoW)` and `pub fn deserialize(...)`

### Validation Logic

Source: `src/auxpow.rs` — `pub fn validate(ap: &AuxPoW, aux_header_bytes: &[u8], target: Target)`

1. `aux_hash = sha256d(aux_header_bytes)` — compute the Irium block hash
2. `(committed_root, chain_count) = find_commitment(coinbase_txn)` — scan coinbase for the magic prefix
3. If `chain_count == 1`: verify `committed_root == aux_hash` directly
4. If `chain_count > 1`: verify `compute_merkle_root(aux_hash, blockchain_branch, blockchain_branch_index) == committed_root`
5. `coinbase_txid = sha256d(coinbase_txn)` — compute the coinbase transaction ID
6. Verify `compute_merkle_root(coinbase_txid, coinbase_branch, coinbase_branch_index) == parent_header[36..68]`
7. Verify `sha256d(parent_header)` (reversed to display order) meets the Irium block target

Maximum allowed Merkle branch depth is 20 (`MAX_BRANCH_DEPTH`).

## SHA-256d Hardware Compatibility

Every existing SHA-256d ASIC is compatible with Irium, both for standard solo mining today and for merged mining after activation.

| Hardware | Status |
|----------|--------|
| Antminer S19 / S21 series | Full compatibility |
| Whatsminer M50 / M60 series | Full compatibility |
| Any Bitmain ASIC | Full compatibility |
| Any MicroBT ASIC | Full compatibility |
| Any SHA-256d USB miner | Full compatibility |
| cpuminer-opt (software) | Full compatibility (`--algo sha256d`) |
| CGMiner / BFGMiner | Full compatibility |

## For Pool Operators: Adding Irium Merged Mining

Irium ships a Stratum server (`pool/irium-stratum`) that handles both standard solo mining and AuxPoW merged mining automatically. The server detects the activation height and switches modes at the right block.

### Quick Start

```bash
# Clone and build
git clone https://github.com/iriumlabs/irium.git
cd irium/pool/irium-stratum
cargo build --release

# Configure and run
export IRIUM_RPC_BASE=http://localhost:38300
export IRIUM_RPC_TOKEN=your_secret_token_here
export STRATUM_BIND=0.0.0.0:3333
export IRIUM_AUXPOW_ACTIVATION_HEIGHT=26347

./target/release/irium-stratum
```

The server fetches work from the node via `GET /rpc/getblocktemplate`, serves jobs to miners via Stratum V1, and submits solved blocks via `POST /rpc/submit_block`. After activation height, it automatically adds the AuxPoW commitment and parent coinbase for each job.

See `docs/POOL-OPERATOR.md` for complete configuration documentation.

### Adding Irium to an Existing Bitcoin Pool

If you already operate a Bitcoin pool and want to add Irium merged mining:

1. Run an `iriumd` node on your pool infrastructure (see `docs/POOL-OPERATOR.md`)
2. Deploy the `irium-stratum` server pointing at your `iriumd` node
3. Your miners continue pointing at your Bitcoin pool — no miner-side change required
4. Your pool software embeds the Irium commitment in the Bitcoin coinbase using the format above
5. When a share solves the Irium target, construct and submit the AuxPoW block

The commitment embedding step is the only change to your coinbase construction. The `build_commitment(aux_hash, chain_count, nonce)` function in `src/auxpow.rs` is the reference implementation.

### Stratum Configuration for Miners

Miners do not need to know about AuxPoW. They point at the Stratum endpoint and mine as normal. The pool handles all commitment embedding and block submission.

```
Endpoint:   stratum+tcp://your-pool:3333
Username:   YOUR_IRIUM_ADDRESS.worker_name
Password:   x
```

## Mining Irium as a Standalone Chain (Pre-Activation and Post-Activation)

Standard solo mining works the same before and after AuxPoW activation. AuxPoW blocks are accepted alongside standard blocks — the network does not require AuxPoW.

```bash
# cpuminer-opt example (solo)
cpuminer-opt --algo sha256d \
  --url stratum+tcp://pool.iriumlabs.org:3333 \
  --user YOUR_IRIUM_ADDRESS.worker1 \
  --pass x
```

## Chain Parameters for Pool Operators

| Parameter | Value |
|-----------|-------|
| Algorithm | SHA-256d |
| Block time target | 600 seconds (10 minutes) |
| Difficulty algorithm | LWMA v2 (30-block window) from height 19,740 |
| Max difficulty step per block | 2x in either direction |
| Block reward (current era) | 50 IRM |
| Halving interval | 210,000 blocks |
| Max supply | ~24,500,000 IRM |
| Coinbase maturity | 100 blocks |
| AuxPoW version bit | bit 8 (value 256) |
| AuxPoW commitment magic | `0xfa 0xbe 0x6d 0x6d` |
| AuxPoW activation height | 26,347 |
| AuxPoW estimated activation | ~12 June 2026 |

## Connecting to the Public Pool

The official public Irium pool uses SOLO payout — the full coinbase reward goes to the miner address.

```
ASIC:      stratum+tcp://pool.iriumlabs.org:3333
CPU/GPU:   stratum+tcp://pool.iriumlabs.org:3335
Username:  YOUR_IRIUM_ADDRESS.worker1
Password:  x
```

## Contact

For pool integration questions: Telegram — https://t.me/iriumlabs
For technical questions: GitHub Issues — https://github.com/iriumlabs/irium
