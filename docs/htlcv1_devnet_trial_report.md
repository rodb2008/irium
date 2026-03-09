# HTLCv1 Devnet Trial Report (Controlled, Activation-Gated)

Date: 2026-03-09

## Topology
- Node A host: `irium-vps` (`vmi2780294`)
- Node B host: `irium-eu` (`vmi2995746`)
- Trial code/workdir: `/tmp/htlc-tcbm`
- Trial data dirs:
  - Node A: `/home/irium/.htlc-devtrial/node1`
  - Node B: `/home/irium/.htlc-devtrial-eu/node2`
- Trial ports:
  - Node A P2P/RPC: `59291` / `127.0.0.1:58400`
  - Node B P2P/RPC: `59292` / `127.0.0.1:58401`

## Activation Used
- `IRIUM_HTLCV1_ACTIVATION_HEIGHT=5` on both trial nodes.
- HTLCv1 is activation-gated and remains OFF unless explicitly configured.
- Mainnet activation remains OFF.

## Root Causes Encountered and Fixes
1. **Template/miner activation mismatch**
- Symptom: post-activation HTLC tx accepted by RPC/mempool but miner logged `Skipping invalid template tx: HTLCv1 output before activation`.
- Cause: miner `ChainParams` used `htlcv1_activation_height: None`.
- Fix: `src/bin/irium-miner.rs` now reads `IRIUM_HTLCV1_ACTIVATION_HEIGHT` and passes it into miner `ChainParams`.

2. **Cross-host trial peering instability**
- Symptom: trial nodes failed to maintain direct connectivity.
- Cause: seed/connect trial config drift (host/port mismatch in runtime seed list).
- Fix: corrected isolated trial seed entries and restarted trial user services.

## Scenario Matrix

| Scenario | Result | Evidence |
|---|---|---|
| Pre-activation HTLC rejection | PASS | Controlled run with activation unset rejected HTLC path (recorded during trial bring-up). |
| Post-activation HTLC funding accepted by RPC/mempool | PASS | `SC1_TXID=d8f3c0ff190c93f0b6f4c6e052122876c48a105f353e9cc8e2102fd77a96f140` accepted. |
| Post-activation HTLC funding included in mined block | PASS | `SC1_IN_TEMPLATE=true`; funding appears in `block_408.json` (Node A). |
| Valid claim path mined/included | PASS | Claim spend of SC1 funding appears in `block_701.json` (Node A). |
| Wrong-preimage claim rejection | PASS | `WRONG_CLAIM_HTTP=400` (scenario evidence). |
| Refund before timeout rejection | PASS | `SC4_REFUND_BEFORE_HTTP=400` with `TO=5982` at current height `1017`. |
| Refund after timeout success | PASS | `SC3` funding tx `bbd0f7...` later refunded via tx `6aba787e...`, included in `block_983.json`. |
| Node restart during lifecycle | PASS | trial services restarted; peer_count recovered to `1` on each node; chain/tip aligned. |
| Mempool persistence/reload | PARTIAL | basic mempool accept->mine validated; crash/restart persistence matrix not exhaustively executed. |
| Reorg handling | NOT RUN | deferred; requires deliberate fork orchestration harness. |
| Legacy P2PKH path after activation | PASS | normal mining/relay continued; no legacy path regression observed. |

## Evidence Summary
- Node A status (`127.0.0.1:58400/status`): height `1017`, peer_count `1`.
- Node B status (`127.0.0.1:58401/status`): height `1017`, peer_count `1`.
- Funding lifecycle evidence files:
  - `/home/irium/.htlc-devtrial/evidence/sc1.txt`
  - `/home/irium/.htlc-devtrial/evidence/sc3.txt`
  - `/home/irium/.htlc-devtrial/evidence/sc4.txt`
- On-chain inclusion evidence (Node A block files):
  - HTLC funding included: `/home/irium/.htlc-devtrial/node1/blocks/block_408.json`
  - Claim spend included: `/home/irium/.htlc-devtrial/node1/blocks/block_701.json`
  - Refund spend included: `/home/irium/.htlc-devtrial/node1/blocks/block_983.json`

## Remaining Limitations
1. Reorg-specific HTLC behavior not yet exercised in automated trial.
2. Mempool persistence/reload matrix not fully exhaustive.
3. `inspecthtlc` focuses on UTXO-state-derived status; historical spend metadata remains limited.

## Mainnet Safety Statement
- HTLCv1 is not activated on mainnet.
- No production config in this trial enabled mainnet HTLCv1.
- This report validates devnet/test-only behavior with explicit activation height.
