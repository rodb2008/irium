# Phase 12-M: P2P New-Block Announce Sync Fix

**Status:** Complete
**Branch:** testnet/poawx-phase12-completion-rc-hardening
**Date:** 2026-06-13
**Root cause fixed:** `submit_block_extended` never called `broadcast_block`, so PoAW-X submitted blocks were never announced to pre-connected peers.

---

## 1. Root Cause

`submit_block_extended` is the only block submission path available when
`IRIUM_POAWX_MODE=active` on non-mainnet (the legacy `submit_block` returns HTTP 405).
However, `submit_block_extended` ended its success path with a JSON write + log + return,
with no call to `broadcast_block`. The relay code existed only inside `submit_block`
(legacy path, now disabled for PoAW-X devnet).

**Effect**: Every block accepted via `submit_block_extended` was persisted locally and
applied to the chain but never announced to connected peers. VPS-2 only received the
block after `recent_requested_blocks` TTL (30 s) expired and it re-sent a GetBlocks
request — typically 3–4 minutes after submission.

**Evidence** from Phase 12-L VPS-1 devnet log:
- No `[relay]` log entry appeared after block 1 acceptance at 18:10:36.
- VPS-1 did not log "sending 1 blocks" until 18:14:06 — 4 minutes later (GetBlocks path).
- VPS-2 never logged "inbound block" — confirming it never received the broadcast.

---

## 2. Fix — One Insertion in `src/bin/iriumd.rs`

Inserted after `storage::write_block_json` and before the accepted `eprintln!` in
`submit_block_extended` (function starting at line ~13996):

```rust
// Phase 12-M: broadcast accepted block to pre-connected peers via P2P.
if let Some(ref p2p) = state.p2p {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&block.header.serialize_for_height(new_height));
    for tx in &block.transactions {
        bytes.extend_from_slice(&tx.serialize());
    }
    if let Err(e) = p2p.broadcast_block(&bytes).await {
        eprintln!("Failed to broadcast accepted block over P2P: {}", e);
    }
}
```

This is identical to the broadcast block code already present in `submit_block` (line ~14710).
`broadcast_block` handles duplicate-relay suppression via `recent_relayed_blocks` cache.

---

## 3. Files Changed

| File | Change |
|------|--------|
| `src/bin/iriumd.rs` | 12 lines inserted into `submit_block_extended` (broadcast after accept) |

---

## 4. Checks Run

| Check | Result |
|-------|--------|
| `cargo fmt` | OK |
| `cargo build --release` | OK — 3 pre-existing warnings, 0 new |
| `cargo test` | 236 passed, 0 failed |

---

## 5. Two-Node Live E2E Result

**Setup**: VPS-1 (207.244.247.86, devnet P2P 39510) and VPS-2 (157.173.116.134,
devnet P2P 39514). Both started fresh from genesis. VPS-2 connected to VPS-1 via
`--add-seed 207.244.247.86:39510` before block submission.

**Block submission log (VPS-1)**:
```
[05:26:51] [relay] P2P relay: announced block 228be8b7dfef to 1 peers (headers_first=true, full_block_fallback=true)
```

**Block receipt log (VPS-2)**:
```
[05:26:51] [🔁 sync] P2P 207.244.247.86:39510: inbound block 228be8b7dfef prev=0000000028f2 prev_in_headers=false prev_in_blocks=true
[05:26:51] [🔌 p2p] P2P 207.244.247.86:39510: accepted block height 1 hash 228be8b7dfef
```

Both events at the same timestamp — instantaneous propagation.

### E2E Score: 25/25 PASS

| # | Check | Phase 12-L | Phase 12-M |
|---|-------|-----------|-----------|
| P-1 | VPS-1 node responding | PASS | PASS |
| P-2 | VPS-1 at genesis | PASS | PASS |
| P-3 | VPS-2 node responding | PASS | PASS |
| P-4 | Worker keypair generated | PASS | PASS |
| P-5 | Seed/nonce derived | PASS | PASS |
| P-6 | Puzzle solution found | PASS | PASS |
| P-7 | Challenge signed | PASS | PASS |
| P-8 | Receipt accepted | PASS | PASS |
| P-9 | Receipt in pending state | PASS | PASS |
| P-10 | receipts_root non-empty | PASS | PASS |
| P-11 | Coinbase built | PASS | PASS |
| P-12 | Merkle root computed | PASS | PASS |
| P-13 | Block template height=1 | PASS | PASS |
| P-14 | Block header mined | PASS | PASS |
| P-15 | Block accepted by VPS-1 | PASS | PASS |
| P-16 | VPS-1 at height 1 | PASS | PASS |
| P-17 | Receipt cleared after commit | PASS | PASS |
| **P-18** | **VPS-2 synced via P2P** | **FAIL** | **PASS** |
| N-1 | Legacy submit_block → 405 | PASS | PASS |
| N-2 | Empty receipts → 400 | PASS | PASS |
| N-3 | Bad signature → 400 | PASS | PASS |
| N-4 | Spoofed pkh → 400 | PASS | PASS |
| N-5 | Insufficient PoW → 400 | PASS | PASS |
| N-6 | Mainnet PoAW-X not active | PASS | PASS |
| N-7 | RPC 39511 localhost-only | PASS | PASS |

**Phase 12-M turned check P-18 from FAIL to PASS, completing the full 25/25.**

---

## 6. VPS-2 Synced Without Restart?

**YES.** VPS-2 was pre-connected to VPS-1 at genesis before block submission and
advanced to height 1 within the 30-second polling window with no restart.
This is the exact stall scenario that was failing in Phase 12-L.

---

## 7. Mainnet Untouched

- VPS-1 mainnet `iriumd` (PID 2218918) not touched, running normally.
- VPS-2 mainnet `iriumd.service` remains stopped (pre-existing, not restarted).
- No mainnet blocks submitted or modified.
- No mainnet P2P connections made.
- All private keys, tokens, miner IPs redacted from this document.

---

## 8. Remaining Blockers Before Real Miner Pilot

Real miner testing remains frozen. Open items:

| ID | Description | Status |
|----|-------------|--------|
| P-1 | Receipt data (worker pubkeys, solutions) in block wire format | Open |
| P-2 | Consensus-level puzzle PoW in `connect_block` (not just submit path) | Open |
| R-2 | Reorg receipt restore (receipts cleared on commit, not restored on disconnect) | Open |

The P2P bypass gap from Phase 12-J audit (relayed blocks bypass receipt validation — only irx1 presence enforced at consensus) remains open. Safe for controlled private testnet; not safe for uncontrolled environments.

---

## 9. Summary

| Dimension | Result |
|-----------|--------|
| Root cause | `submit_block_extended` missing `broadcast_block` call |
| Fix | 12 lines in `src/bin/iriumd.rs` |
| cargo build | 0 errors, 0 new warnings |
| cargo test | 236/236 pass |
| VPS-2 sync without restart | **PROVEN** |
| Full 25/25 E2E | **PASS** |
| Mainnet | **UNTOUCHED** |
| Real miner testing | **FROZEN** (P-1, P-2, R-2 open) |
