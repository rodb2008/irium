# Irium: DNS-Free Proof-of-Work Mainnet (Rust Implementation)

## Abstract
Irium is a production-only proof-of-work blockchain designed for network independence, transparent founder vesting, and light-client usability from genesis. Bootstrap avoids DNS entirely, relying on signed seedlists and anchors. The Rust implementation provides the full node, miner, and SPV tooling.

## 1. Introduction
- Mainnet-first: no testnet code or demo networks.
- DNS-free bootstrap: signed `seedlist.txt` + `anchors.json` distributed via multiple channels.
- Consensus-enforced vesting: founder allocation timelocked at genesis; supply capped at 100 M IRM.
- Light-client friendly: header-first sync, SPV helpers; anchor enforcement planned.

## 2. System Architecture
- **Consensus Core (Rust):** SHA-256d PoW, block/tx validation, difficulty retarget, UTXO set.
- **Genesis Framework:** locked genesis JSON, deterministic loader in `src/genesis.rs`.
- **Bootstrap Layer:** signed seedlist + runtime cache; anchors file for checkpoints.
- **P2P:** custom sybil-resistant handshake, header-first sync, inv/getdata for tx/blocks; peer scoring and libp2p parity planned.
- **Client Ecosystem:** full node, miner, SPV binary; wallet primitives for P2PKH spends.

## 3. Consensus Mechanics
- PoW: SHA-256d, 600s target, retarget every 2016 blocks.
- Coinbase maturity: 100 blocks.
- Monetary policy: 50 IRM start, halving every 210,000 blocks, capped at 100 M IRM.
- Genesis: 3,500,000 IRM CLTV-locked for 3 years; ~96,500,000 IRM issued via PoW.
- Validation: merkle root and PoW checks, bits match target, fee + subsidy bound to schedule, UTXO double-spend/maturity checks.

## 4. Bootstrap Without DNS
- `bootstrap/seedlist.txt` (+ `.sig`, `trust/allowed_signers`) verified via `ssh-keygen -Y verify`.
- `bootstrap/seedlist.runtime` caches discovered peers.
- `bootstrap/anchors.json` provides checkpoints (anchor validation to be wired into sync).

## 5. Networking and Security
- Sybil-resistant handshake with PoW challenge/response.
- Header-first sync with fork-aware reorg by cumulative work.
- INV/GETDATA for txs/blocks; relay-address advertising for optional relay payouts.
- Planned: peer scoring/uptime attestations, anchor-enforced header validation, libp2p-compatible discovery.

## 6. Light Client Strategy
- SPV helper (`irium-spv`) verifies merkle proofs against stored blocks.
- Header tracking in `ChainState`; anchor enforcement planned.
- NiPoPoW not yet implemented.

## 7. Implementation Status
- **Complete:** locked genesis loader, PoW validation, block/tx serialization, miner, mempool, header tracking, fork-aware reorg, signed-seed verification.
- **In progress/planned:** anchor enforcement in sync, peer scoring/libp2p parity, NiPoPoW, relay reward accounting beyond coinbase outputs, production monitoring.

## 8. Operations
- Systemd example at `scripts/iriumd.service.example` (journald logging, auto-restart).
- Config overrides via `configs/node.json` and env vars (`IRIUM_NODE_CONFIG`, `IRIUM_RELAY_ADDRESS`, `IRIUM_NODE_HOST/PORT`).

## 9. Governance & Upgrades
- Off-chain coordination; upgrades announced via anchors and releases. No on-chain governance.

## 10. Conclusion
Irium targets a resilient mainnet that can bootstrap without DNS, enforce vesting at consensus, and support light clients from genesis. Remaining work focuses on anchor enforcement, richer peer scoring/discovery, and NiPoPoW support.
