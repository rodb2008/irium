# Irium: DNS-Free Proof-of-Work Mainnet

## Abstract

Irium is a purpose-built proof-of-work blockchain designed to maximize network independence and long-term survivability. The protocol eliminates reliance on DNS infrastructure, enforces transparent founder vesting via on-chain timelocks, incentivizes transaction relay quality without inflation, and prioritizes light client usability from genesis. This whitepaper outlines the core design principles, system architecture, monetary policy, and roadmap for the IRM asset and its supporting network.

## 1. Introduction

Most established proof-of-work networks inherit architectural assumptions from Bitcoin, including DNS-based bootstrapping, addrman-driven peer discovery, and an absence of protocol-level incentives for fast relay. Irium rethinks these components to produce a mainnet that can launch and sustain itself even if all founding infrastructure disappears. The project ships with no test or demo networks; development centers solely on a production-ready mainnet.

### 1.1 Goals
- Ensure permanent bootstrap viability without DNS dependencies or trusted third-party domains.
- Deliver transparent, consensus-enforced founder vesting with irreversible timelocks.
- Provide miners and relay nodes with optional fee-sharing incentives that reward high-quality propagation.
- Harden the peer discovery layer against Sybil attacks and botnet saturation.
- Enable NiPoPoW-ready light clients and SPV tooling from the network’s first block.
- Maintain a lean on-chain footprint while enabling notarization of off-chain metadata.

## 2. System Architecture Overview

Irium separates responsibilities into modular subsystems that interoperate through explicit interfaces:

1. **Consensus Core** – Handles block production, validation, and difficulty retargeting using SHA-256d proof-of-work. The reference implementation is authored in Python to remain auditable and succinct.
2. **Genesis Framework** – A deterministic JSON configuration (`configs/genesis.json`) together with a builder script constructs the genesis block and CLTV-locked founder output while leaving the mining supply unallocated until proof-of-work issuance.
3. **Bootstrap Layer** – Uses signed `seedlist.txt` and `anchors.json` artifacts, distributed through multiple channels (GitHub releases, IPFS, torrents). Nodes bootstrap exclusively from these signed resources.
4. **Peer-to-Peer Network** – Built atop libp2p gossip protocols with self-healing peer state, uptime proofs, and Sybil-resistant handshakes.
5. **Client Ecosystem** – Provides primitives for full nodes and SPV wallets, enabling users to verify transactions via NiPoPoW proofs and anchor checkpoints.

## 3. Consensus Mechanics

### 3.1 Proof-of-Work
- Algorithm: SHA-256d (double SHA-256 hashing of block headers).
- Target Block Time: 600 seconds.
- Difficulty Retarget: Every 2016 blocks using a bounded adjustment to avoid oscillations.
- Coinbase Maturity: 100 blocks to mitigate short-range reorg exploitation.

The `ChainState` reference implementation recalculates merkle roots, enforces that each block header links to the current tip, validates the SHA-256d target, and refuses coinbase payouts that exceed the allowed subsidy plus accrued fees. While accepting blocks it maintains an in-memory UTXO set so subsequent transactions are checked for double spends and value conservation, additionally rejecting outputs whose encodings overflow consensus limits and accounting for cumulative subsidy so issuance can never surpass the 100 million IRM cap.

### 3.2 Monetary Policy
- Maximum Supply: 100,000,000 IRM.
- Genesis Mint: 3,500,000 IRM is created in the block 0 coinbase transaction and timelocked for three years.
- Mining Emission: Blocks 1 through 1,930,000 award a fixed 50 IRM subsidy (5,000,000,000 satoshis) to miners; after this window the subsidy drops to zero and rewards become fee-only.
- Supply Invariance: Consensus validation rejects coinbase payouts that exceed the scheduled subsidy plus collected fees, preventing inflation or effective burning via negative fee accounting, while tracking minted subsidy totals against the immutable 100 million IRM ceiling.

### 3.3 Genesis Distribution
- Total Founder Allocation: 3,500,000 IRM held in a single output.
- Timelock: CLTV-enforced at the block height corresponding to three years of blocks from genesis.
- Mining Supply: 96,500,000 IRM left unallocated at genesis and released solely through proof-of-work subsidies.

The genesis layout is encoded in human-readable JSON and reproducible with deterministic tooling to support third-party audits.

## 4. Bootstrapping Without DNS

Traditional blockchain clients query DNS seeds to discover peers, creating a single point of failure. Irium replaces this mechanism with cryptographically authenticated artifacts:

- **seedlist.txt** – Contains IPv4/IPv6 multiaddresses along with libp2p peer IDs. A detached secp256k1 signature ensures authenticity.
- **seedlist.runtime** – A node-maintained cache that automatically records newly observed peers. Tooling merges the signed list with this runtime cache to remain connected as the network evolves.
- **anchors.json** – Publishes a rolling set of block header checkpoints. Each record hashes the height, block hash, and timestamp and is signed by designated anchor signers via secp256k1. Clients cross-check header sync against these anchors to detect potential eclipse attacks.
- **irium-zero.sh** – Automates signature verification, seed extraction, and node initialization. Users can run the script offline after mirroring the artifacts.

Multiple distribution channels (GitHub, IPFS, torrents) provide redundancy. Because bootstrap never depends on DNS, the network can persist even if all official domains vanish.

## 5. Peer Networking and Security

### 5.1 Self-Healing Discovery
Peers exchange uptime attestations over libp2p gossip. Each attestation references other peers, allowing the network to “remember” reliable participants even if the initial seedlist disappears. Nodes adjust peer scoring based on observed performance, enabling organic evolution of the peer set. The reference `PeerDirectory` persists these observations to disk and refreshes `seedlist.runtime`, giving fresh nodes immediate access to recently verified peers without touching DNS.

### 5.2 Sybil-Resistant Handshakes
Every handshake uses ephemeral key pairs that sign a proof-of-uptime token (a small proof-of-work or time-bound capability). Peers verify these tokens before admitting connections, discouraging large-scale Sybil attacks and botnet flooding.

### 5.3 Relay Reward Commitments
Miners may include optional relay rewards in their coinbase transactions. Each reward is expressed as a standard P2PKH output that directs a portion of the block’s fee pool to the designated relay peer, ensuring payout enforcement by consensus without inflating supply. An accompanying `OP_RETURN` memo tagged with `relay:` data can be emitted for auditability, allowing operators to prove their relay contributions via merkle proofs while keeping fee accounting transparent.

### 5.4 Anchor-File Consensus
`anchors.json` acts as an audit layer that complements the canonical blockchain history. Anchor signers publish checkpoints containing block hashes, heights, and aggregated signatures. Clients validate downloaded headers against these checkpoints to ensure they follow a chain vetted by multiple independent signers, reducing the risk of eclipse or long-range attacks.

### 5.5 Metadata Commitments
Coinbase transactions may embed hash pointers referencing documentation, open-source mirrors, regulatory filings, or updated seedlists. Because only hash commitments are stored on-chain, the ledger avoids unbounded growth while preserving an immutable link to critical off-chain information.

## 6. Light Client Strategy

Irium is light-client-first. The reference Python package now exposes:
- Header validation routines compatible with SPV wallets.
- Merkle proof verification utilities.
- Anchor checkpoint validation functions.
- A deterministic hot wallet that manages keys, UTXOs, and RFC6979-signed P2PKH transactions.

These tools support NiPoPoW techniques where clients verify short proofs of work instead of downloading the full chain. By combining NiPoPoWs with anchor checkpoints and the shared wallet core, light clients can securely operate even in adversarial network environments while remaining interoperable with full-node tooling.

### 5.6 Reference Mining Loop
The open-source `irium.miner` module packages block assembly logic: transaction selection by fee density, BIP34-compliant coinbase creation with automatic extra-nonce cycling, relay reward validation, and nonce iteration. It emits `MiningStats` so miners coordinating heterogeneous hardware can track attempt counts and wall-clock mining time while sharing a deterministic block template generator.

## 7. Implementation Status

- **Complete:** Genesis specification, deterministic builder, proof-of-work primitives, block and transaction serialization, bootstrap verification tooling, peer directory with automatic seedlist maintenance, hot wallet integration, reference mining loop, protocol documentation.
- **In Progress:** Production-ready libp2p networking layer, relay reward accounting integration, comprehensive wallet UX.
- **Planned:** External security audits, formal specification of relay reward algorithms, integration of hardware wallet signing flows for CLTV outputs.

## 8. Governance and Upgrade Path

Irium favors rough consensus and public review over on-chain governance. Proposed consensus changes follow this pipeline:
1. Draft improvement proposals referencing the whitepaper and architectural documentation.
2. Prototype implementations in dedicated branches or repositories (never in mainnet by default).
3. Conduct multi-stakeholder audits (miners, developers, service providers).
4. Coordinate network upgrades via signed anchor announcements and miner signaling.

## 9. Conclusion

Irium delivers a mainnet-first blockchain architecture that prioritizes independence, verifiability, and user sovereignty. By removing DNS from bootstrap, enforcing founder vesting at the consensus layer, and launching with robust light client support, the project sets a new baseline for sustainable proof-of-work networks.

## 10. References

1. Satoshi Nakamoto, “Bitcoin: A Peer-to-Peer Electronic Cash System,” 2008.
2. Aggelos Kiayias et al., “Non-Interactive Proofs of Proof-of-Work (NiPoPoWs),” 2017.
3. libp2p Project, “libp2p Specification,” https://github.com/libp2p/specs.

