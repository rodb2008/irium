# Irium Mainnet Architecture

## Consensus

* **Algorithm:** Proof-of-Work using SHA-256d.
* **Target block time:** 600 seconds.
* **Difficulty retarget:** every 2016 blocks, bounded between 4x and 0.25x adjustments.
* **Monetary policy:** 3,500,000 IRM minted in genesis, 96,500,000 IRM mined over 1,930,000 blocks, then fee-only rewards.
* **Coinbase maturity:** 100 blocks.

The reference `ChainState` verifies that every block extends the active tip, recomputes merkle roots, enforces SHA-256d proof-of-work targets, and rejects coinbase payouts that exceed the permitted subsidy plus collected fees while keeping an in-memory UTXO set for subsequent transaction validation. It now also enforces canonical output encodings and tracks the cumulative subsidy so the chain can never issue more than the fixed 100 million IRM supply.

## Genesis Distribution

The genesis block coinbase transaction mints only the 3,500,000 IRM founder allocation. No other addresses receive genesis funds
—the remaining 96,500,000 IRM is unlocked exclusively through proof-of-work mining.

* **Founder vesting:** 3,500,000 IRM locked via `OP_CHECKLOCKTIMEVERIFY` for three years (expressed in block height).
* **Mining supply:** 96,500,000 IRM reserved for 50 IRM subsidies on blocks 1–1,930,000; after that point the network becomes fee only.

The distribution is declared in `configs/genesis.json` with explicit scriptPubKeys so anyone can validate the CLTV commitments.

## Bootstrap Strategy

Irium launches without DNS seeds. Nodes bootstrap by downloading signed `seedlist.txt` and `anchors.json` files. The shell helper `scripts/irium-zero.sh` fetches the latest artifacts, verifies their secp256k1 signatures, and exports the raw multiaddresses. Clients then connect directly to the listed peers and merge them with the runtime cache maintained at `bootstrap/seedlist.runtime`.

### Self-healing Discovery

Nodes gossip uptime proofs for peers via libp2p. Each node stores a rolling cache of verified peers (address, services, proof). When a new node joins it requests these caches, enabling peer discovery even if the original seed list disappears. The Python `PeerDirectory` utility persists these observations to `state/peers.json` and refreshes `seedlist.runtime` so tooling can immediately benefit from the expanding peer surface.

### Anchor File Consensus

`anchors.json` contains rolling header commitments. Each entry hashes the height, block hash, and timestamp into a deterministic digest that is signed by designated bootstrap keys via secp256k1. During initial sync a node cross-validates downloaded headers against these anchors to limit eclipse and long-range attacks.

## Relay Reward Commitments

Miners can voluntarily dedicate a portion of transaction fees to their upstream relay peers. The reference `irium.miner.Miner` encodes each `RelayCommitment` as a standard P2PKH output funded from collected fees and, optionally, an `OP_RETURN` memo tagged with `relay:` metadata for auditability. The miner verifies that the aggregated commitment amounts never exceed the fee pool before finalizing the coinbase.

### Block Assembly Pipeline

The miner consumes `TxCandidate` objects—each containing a transaction, fee, and weight—and sorts them by satoshi-per-weight to maximize yield within the 4,000,000 weight limit. It then builds the block header using the latest chain tip, crafts a BIP34-style coinbase script with an incrementing extra nonce, and refreshes the merkle root after every modification. When the 32-bit nonce space is exhausted the extra nonce is bumped and the coinbase is regenerated automatically. Runtime metrics (attempt count, elapsed time, accumulated fees) are exported via `MiningStats` so production deployments can coordinate hardware sweeps while relying on the shared block template logic.

## P2P Handshake Hardening

Handshake flow uses ephemeral X25519 keys. Each side signs the handshake transcript with its long-term node key and includes a lightweight Proof-of-Uptime token (hashcash-like). Nodes refusing to present valid proofs are deprioritized.

## Light Client Support

The `irium.wallet` package now bundles deterministic key management, UTXO tracking, and fully signed P2PKH transaction assembly. These primitives are shared between full nodes and SPV clients, which can verify anchors and NiPoPoW proofs while relying on the same wallet core. Future work will layer networked SPV synchronization on top of the shipped wallet foundation.

## On-chain Metadata Commitments

Blocks may include a single coinbase `OP_RETURN` output referencing off-chain payloads via a SHA-256 multihash. This keeps the chain lean while notarizing documents such as updated seedlists or governance announcements.
