# Phase 12-K: PoAW-X Controlled Two-Node VPS Live E2E

**Status:** Complete (documentation, code fixes in test script only)
**Branch:** testnet/poawx-phase12-completion-rc-hardening
**Test date:** 2026-06-12
**Run host:** VPS-1 (207.244.247.86, devnet node at 127.0.0.1:39511)
**P2P target:** VPS-2 (157.173.116.134, devnet node at 127.0.0.1:39511 / P2P 0.0.0.0:39514)
**Binary:** iriumd release (rebuilt clean — debug eprintln removed)
**Verdict:** PARTIAL

---

## 1. Topology

| Node | Role | RPC | Status | P2P |
|------|------|-----|--------|-----|
| VPS-1 | Block submitter | 127.0.0.1:39511 | 127.0.0.1:39508 | 0.0.0.0:39510 |
| VPS-2 | P2P sync target | 127.0.0.1:39511 | 127.0.0.1:39508 | 0.0.0.0:39514 |

Both nodes started with:
- `IRIUM_NETWORK=devnet`
- `IRIUM_POAWX_MODE=active`
- `IRIUM_POAWX_ACTIVATION_HEIGHT=1`
- `IRIUM_POAWX_PUZZLE_DIFFICULTY_BITS=4`
- Fresh state dirs (`/home/irium/irium-devnet-state-vps{1,2}/`)

VPS-2 used `--add-seed 207.244.247.86:39510` to seed from VPS-1.

---

## 2. Test Script

**File:** `phase12k_e2e.py` (run on VPS-1)

Two bugs found during Phase 12-K and fixed in the test script (not in iriumd):

### Bug A — Irium custom transaction format

`decode_full_tx_at` in `src/tx.rs` expects a 1-byte txid_len prefix before each input's txid.
The original `serialize_tx` wrote raw txids without this prefix, causing silent HTTP 400 from
`submit_block_extended` (the parser returned `Err("invalid prev_txid length")` which mapped
to `StatusCode::BAD_REQUEST` with no log).

Also: input count, output count, and input script_len must be `u8` (1-byte), not varint.
Output script_len stays varint (as in `TxOutput::serialize`).

### Bug B — Merkle root byte order

`block.merkle_root()` (Rust) returns wire-order sha256d of the serialized coinbase.
`validate_block_header` compares `block.header.merkle_root` against this value directly.

The original code sent display-order (reversed sha256d), causing a mismatch in
`validate_block_header` merkle root check (for height > 0).

Fix: send `sha256d(coinbase_bytes).hex()` (wire order, no reversal).

`compute_header_hash` is unaffected: it reverses wire to display for the pre-activation hash
input, matching what `hash_for_height` does in Rust.

---

## 3. Positive Flow Results

| Step | Check | Result | Detail |
|------|-------|--------|--------|
| 1 | VPS-1 testnet node responding | PASS | HTTP 200 |
| 2 | VPS-1 at genesis (height 0) | PASS | height=0 |
| 3 | Worker keypair generated | PASS | pkh_len=40 |
| 4 | Seed/nonce derived | PASS | from genesis hash + parent_h=0 |
| 5 | Puzzle solution found (4-bit) | PASS | elapsed=0.00s |
| 6 | Challenge signed | PASS | sig_len=64, low-S normalized |
| 7 | Receipt accepted | PASS | HTTP 200, pending_count=1 |
| 8 | Receipt in pending state | PASS | visible in getblocktemplate |
| 9 | receipts_root non-empty | PASS | root=da10de502af7c282... |
| 10 | Coinbase built | PASS | len=171 bytes (Irium custom format) |
| 11 | Merkle root computed | PASS | merkle=82fa15be... (wire order) |
| 12 | Block template height=1 | PASS | confirmed from RPC |
| 13 | Block header mined | PASS | nonce=2 hash=16c2521a... |
| 14 | **submit_block_extended accepted** | **PASS** | HTTP 200 `{"accepted":true,"height":1,"tip":"16c2521aa681..."}` |
| 15 | VPS-1 at height 1 | PASS | height=1 confirmed via status |
| 16 | Receipt cleared after commit | PASS | pending_count=0 after block |
| VPS-2 preflight | VPS-2 testnet node responding | **FAIL** | curl exit 7 — status port not ready 4s after start |
| VPS-2 sync | VPS-2 synced to height 1 | SKIP | skipped due to preflight fail |

**Positive flow summary: 15/15 steps PASS.**
VPS-2 preflight failed due to a timing race (status port not yet bound when checked 4 seconds after node start).

---

## 4. Negative Checks

| Check | Expected | Result | Detail |
|-------|----------|--------|--------|
| N-1: legacy submit_block (C-2 gate) | HTTP 405 | PASS | Correctly blocked when PoAW-X active |
| N-2: empty receipts (C-3 gate) | HTTP 400 | PASS | active+testnet+empty rejected |
| N-3: bad worker sig | HTTP 400 | PASS | ECDSA verification failed at receipt POST |
| N-4: spoofed pkh | HTTP 400 | PASS | pubkey to pkh binding check failed |
| N-5: insufficient PoW | HTTP 400 | PASS | sha256d leading zeros < 4 |
| N-6: mainnet PoAW-X not active | Not HTTP 200 | PASS | HTTP 404 (devnet token rejected on mainnet — confirms inactive) |
| N-7: RPC 39511 not publicly exposed | Connection refused | PASS | Binds 127.0.0.1 only |

**All 7 negative checks: PASS.**

---

## 5. VPS-2 P2P Sync Gap

After VPS-1 reached height 1, VPS-2 was polled for 30 seconds. It remained at height 0.

### Root cause: devnet P2P seeds not isolated from mainnet

VPS-2 was started with `--add-seed 207.244.247.86:39510` but the binary hardcoded mainnet seed
list dominated. VPS-2 devnet connected to VPS-1 mainnet P2P (38291) instead of VPS-1 devnet P2P
(39510). VPS-1 mainnet returned mainnet headers (best height 30476), which VPS-2 devnet cannot
accept. VPS-2 devnet chain stayed at height 0.

VPS-1 devnet log confirmed `peers=0` throughout — no inbound connections from VPS-2 devnet.
VPS-1 devnet was also trying to reach VPS-2 at port 38291 (mainnet), failing with connection refused.

This is distinct from the P2P bypass gap (Phase 12-J section 2.4), which concerns receipt
validation bypass in P2P-relayed blocks. This is a devnet network isolation failure: the two
devnet nodes never peered with each other because both connected to the mainnet P2P network instead.

**Classification:** Infrastructure limitation — devnet P2P not isolated from mainnet seed list.

---

## 6. Mainnet Safety Verification

| Node | Mainnet Status |
|------|---------------|
| VPS-1 | HEALTHY — height 30477, 6 peers |
| VPS-2 | PRE-EXISTING DOWN — iriumd.service stopped at 15:31 UTC (before test at 16:30 UTC, clean exit 0). Not caused by testnet work. |

Hard rules verified:
- No mainnet blocks touched or submitted
- RPC 39511 confirmed localhost-only (N-7 PASS)
- No private keys, wallets, miner IPs, or tokens appear in this report
- No PR, no push, no merge to main

---

## 7. Summary

| Dimension | Result |
|-----------|--------|
| Single-node positive submit-path E2E | **COMPLETE** — all 15 positive steps PASS |
| PoAW-X gates (N-1 through N-7) | **COMPLETE** — all 7 negative checks PASS |
| VPS-2 P2P sync | **NOT DEMONSTRATED** — devnet P2P connectivity gap |
| Mainnet | **UNTOUCHED** — VPS-2 down condition pre-existed test |

**Verdict: PARTIAL**

The full positive submit-path flow is proven end-to-end on a live devnet node:
worker keypair → seed/nonce → puzzle → sign → receipt POST → coinbase (Irium format) →
submit_block_extended → height 1 → receipt cleared.

Two bugs were found and fixed in the E2E test script:
- Bug A: Irium custom tx format (missing txid_len prefix, 1-byte counts vs varint)
- Bug B: merkle root must be wire-order (sha256d, not reversed display-order)

These are test script fixes only; iriumd source is unchanged.

The P2P two-node sync was not demonstrated due to devnet network isolation not being configured
correctly. Both devnet nodes connected to the shared mainnet P2P seed list, and their devnet P2P
ports (39510/39514) never linked. Resolving this requires either disabling mainnet hardcoded seeds
in devnet mode or a dedicated devnet seed override that takes precedence.

---

## 8. Known Limitations (Carried Forward from Phase 12-J)

1. **P2P bypass gap**: P2P-relayed blocks bypass receipt validation (irx1 presence only).
   Impact: zero for controlled private testnet. Blocker for any uncontrolled environment.
2. **Block-contained receipt data (P-1)**: worker pubkeys and solutions not in block wire format.
3. **Consensus-level PoW (P-2)**: puzzle PoW only verified on submit path, not in connect_block.
4. **Reorg handling (R-2)**: receipts cleared on commit not restored on disconnect.

---

## 9. Next Steps

Before declaring two-node E2E complete:
- Fix devnet P2P seed isolation (disable mainnet hardcoded seeds when IRIUM_NETWORK=devnet, or
  implement a `--devnet-only-seed` flag that overrides the default list entirely).
- Re-run Phase 12-K with VPS-2 preflight warm-up (at least 15s sleep before preflight check).
- After VPS-2 sync succeeds, verify block at height 1 is accepted via P2P (documents bypass gap).

VPS-2 mainnet: iriumd.service was already stopped (pre-existing, not caused by this test).
Restarting VPS-2 mainnet is a user decision — not actioned here per hard rules.
