# PoAW-X Phase 11-A: Public Testnet Readiness Audit

**Date:** 2026-06-11  
**Branch:** `testnet/poawx-phase11a-public-testnet-readiness`  
**Starting checkpoint:** `a5e5feb` (Phase 10-F remote cleanup complete)  
**Scope:** Audit-only (Phase 11-A). Phase 11-C resolved: disabled-mode endpoints, irx1_root, reset policy. Phase 11-D resolved: firewall applied, direct P2P validated.

---

## A. Protocol Readiness

### A1. `/poawx/assignment` â€” GET

**Status: READY (testnet/devnet)**

- Returns 503 when `IRIUM_POAWX_MODE != active` or network is Mainnet. Hard gate.
- Returns 404 at chain tip height 0 (genesis not yet mined).
- Derives assignment seed: `SHA256(tip_hash_bytes || height_le64 || b"poawx_assignment_seed_v1")`
- Derives commitment nonce: `SHA256(seed || b"commitment_nonce")`
- Returns JSON: `{height, seed, commitment_nonce, puzzle_difficulty, lane, pow_bits}`
- `puzzle_difficulty` is hardcoded `1u64` â€” no adaptive difficulty. Suitable for devnet.
- `pow_bits` echoes the current chain target (devnet: `207fffff`, easy mining).
- `lane` is hardcoded `"cpu"`.
- Tested in: Phase 10-D, Phase 10-E, Phase 10-F.

**Gaps:**
- No per-worker puzzle nonce uniqueness; same seed issued to all callers at same height.
- `puzzle_difficulty` not derived from PoW difficulty; hardcoded 1. Acceptable for testnet.
- No authentication required. Public endpoint acceptable for testnet.

### A2. `/poawx/receipt` â€” POST

**Status: READY (testnet/devnet)**

- Returns 503 if PoAW-X mode not active or network is Mainnet.
- Validates `solution` and `commitment_nonce` as valid hex strings.
- Height staleness check: rejects if `req.height == 0 || req.height + 2 < current_height`.
- Deduplicates by `(height, lane, worker_pkh)` â€” retain latest, drop previous for same key.
- Appends to in-memory `AppState.poawx_pending_receipts` (Arc<Mutex<Vec<...>>>).
- Returns `{ok: true, pending_count: N}`.
- Tested in: Phase 10-D (single receipt), Phase 10-E (single receipt, duplicate dedup), Phase 10-F (multi-segment, 180 blocks).

**Gaps:**
- Receipts are **in-memory only**. Lost on iriumd restart. No persistence to disk.
  - For testnet this is acceptable if the node is stable; miners re-submit on reconnect.
  - For production: persistence needed.
- No signature/proof validation on `solution` â€” solution is accepted as opaque hex.
  - The PoAW-X puzzle proof is not verified by iriumd; only committed on-chain.
  - Planned for future phases.
- No rate limiting per worker beyond global RPC rate limit.

### A3. `/rpc/getblocktemplate` â€” PoAW-X fields

**Status: READY (testnet/devnet)**

Extended fields populated when `IRIUM_POAWX_MODE=active` and non-mainnet:

| Field | Type | Description |
|-------|------|-------------|
| `poawx_mode` | `String` | `"active"` when PoAW-X enabled, `""` otherwise |
| `poawx_pending_receipts` | `Vec<PoawxPendingReceipt>` | Current pending receipts |
| `receipts_root` | `String` | Hex SHA256 root of pending receipts, `""` if empty |

Mainnet: all three fields are empty strings/empty vec â€” safe.  
Tested in: Phase 10-D (template verification), Phase 10-E (Python root verification), Phase 10-F (non-empty throughout 180-block soak).

### A4. `/rpc/submit_block_extended` â€” POST

**Status: READY (testnet/devnet)**

Full validation chain:
1. Rate limit + RPC auth check.
2. receipts_root consistency: if receipts non-empty, computed root must equal submitted root.
3. Block header byte-level validation (prev_hash, merkle_root, hash all 32 bytes).
4. Hash derivation check: derived hash must match submitted hash.
5. tx_hex: non-empty, <= MAX_SUBMIT_BLOCK_TXS transactions.
6. AuxPoW decode if version bit set.
7. irx1 OP_RETURN validation: when receipts non-empty, coinbase must contain 38-byte output: `0x6a 0x24 "irx1" <receipts_root>`.
8. Height match: `req.height == chain.height`.
9. `connect_block` (same chain validation as submit_block).
10. Anchor check (bootstrap/anchors.json).
11. Pending receipts cleared for committed height.

**irx1 OP_RETURN format (38 bytes):**
```
byte[0]    = 0x6a  (OP_RETURN)
byte[1]    = 0x24  (PUSH 36 bytes)
byte[2..6] = "irx1"
byte[6..38] = receipts_root (32 bytes)
```

Tested in: Phase 10-D through Phase 10-F. No invalid acceptances in any log scan.

**Gap:**
- `submit_block_extended` does NOT require PoAW-X mode to be active. It can be called with empty receipts on mainnet (behaves like submit_block). This is intentional for backward compatibility. The PoAW-X guard is at the receipt endpoint, not submit_block_extended.

### A5. Assignment Difficulty Derivation

**Status: DEFERRED (known gap)**

`puzzle_difficulty` is hardcoded `1u64`. For public testnet, adaptive difficulty based on
worker hash rate, time since last block, or network-wide puzzle submission rate would be
needed for a realistic simulation. For the first public testnet pilot, difficulty=1 is
acceptable.

`pow_bits` correctly echoes the chain's `target.bits` from `target_for_height()`. On
devnet this is `207fffff`. On a real testnet chain, it would reflect the LWMA DAA target.

### A6. receipts_root Canonical Format

**Verified canonical algorithm (consistent between iriumd and stratum):**

```
outer_hasher = SHA256()
for receipt in pending_receipts (insertion order):
    inner_hasher = SHA256()
    inner_hasher.update(height.to_le_bytes())     // u64, 8 bytes
    inner_hasher.update(lane.as_bytes())           // UTF-8, variable
    inner_hasher.update(hex::decode(worker_pkh))  // 20 bytes decoded
    inner_hasher.update(hex::decode(solution))    // variable
    inner_hasher.update(hex::decode(commitment_nonce)) // 32 bytes
    outer_hasher.update(inner_hasher.finalize())
root = outer_hasher.finalize()  // 32 bytes
```

Independently verified in Python in Phase 10-E Section 5 (PASS).
Same implementation in both `iriumd.rs::compute_poawx_receipts_root` and
`pool/irium-stratum/src/block.rs::compute_receipts_root_from_pending`.

**HIGH RISK NOTE:** Order is insertion order, not sorted. Multiple workers submitting
receipts for the same block could produce different roots depending on arrival order.
For testnet with a single worker this is fine; for multi-worker use a canonical sort
order (e.g. by worker_pkh asc) must be implemented before multi-miner testnet.

### A7. Pending Receipt Lifecycle

```
POST /poawx/receipt
  -> validated, deduped, appended to AppState.poawx_pending_receipts
  -> next GET /rpc/getblocktemplate returns non-empty pending_receipts + receipts_root
  -> stratum injects irx1 OP_RETURN into coinbase
  -> stratum calls submit_block_extended with receipts + root
  -> submit_block_extended validates irx1 in coinbase
  -> connect_block succeeds
  -> pending receipts for that height are cleared (retain(|r| r.height != committed_height))
  -> next template returns empty pending_receipts / empty receipts_root
```

Tested across 180 blocks in Phase 10-F with no stuck-receipt or double-clear bugs.

### A8. Stratum PoAW-X Path

**Status: READY (testnet/devnet)**

Controlled by `IRIUM_STRATUM_POAWX=1`.

When enabled and template has `poawx_mode=="active"` and non-empty pending receipts:
1. Computes receipts_root independently (must match iriumd's).
2. Calls `build_irx1_commitment_script(&root)` â€” produces 38-byte OP_RETURN.
3. Injects as `coinbase_extras.push((0u64, irx1_script))`.
4. On share accepted: calls `/rpc/submit_block_extended` with full receipts payload.

When `poawx_enabled` but no receipts: logs `"poawx_enabled but job has no receipts_root (mode=...); using legacy submit"` and falls back to `/rpc/submit_block`.

Log probe at job creation: `[poawx] to_job: job=... mode=active pending=N irx1_len=38 receipts_root=...`

Tested in: Phase 10-F (464 irx1_injections in stratum log, 180/180 blocks accepted).

### A9. Mainnet Hard-Disable

**Status: VERIFIED SOLID**

Two independent guards prevent PoAW-X activation on mainnet:
1. `network_kind_from_env() != NetworkKind::Mainnet` â€” checked in both `/poawx/assignment`
   and `/poawx/receipt` and in `getblocktemplate` assembly.
2. `IRIUM_POAWX_MODE=active` env var must be explicitly set.

Both conditions must be true. Mainnet cannot accidentally activate PoAW-X.

Verified in: Every phase from 10-B through 10-F. VPS-1 and VPS-2 mainnet iriumd environ
checked directly via `/proc/<PID>/environ` â€” no `IRIUM_POAWX_MODE` present. No
`IRIUM_POAWX` in systemd service overrides on either VPS.

### A10. `/rpc/block` â€” getblock endpoint

**Status: IMPLEMENTED (not missing)**

The soak scripts reported "404 SKIP" because they used the wrong path `/rpc/getblock`.
The correct endpoint is `/rpc/block?height=N`. Routes confirmed:

```
/rpc/block          GET  by height
/rpc/blocks         GET  range
/rpc/block_by_hash  GET  by hash
```

`/rpc/block?height=N` returns 404 only if `height >= chain.len()` â€” correct behavior.
**This is not a protocol gap.** Soak harnesses should be updated to use correct path.

### A11. Receipt Persistence Across Restarts

**Status: KNOWN GAP â€” IN-MEMORY ONLY**

`AppState.poawx_pending_receipts` is `Arc<Mutex<Vec<PoawxPendingReceipt>>>`, initialized
empty on startup. If iriumd restarts between `/poawx/receipt` POST and block submission,
receipts are lost.

For the public testnet pilot, this is acceptable with documented behavior (miners must
re-call `/poawx/receipt` after reconnecting). For production, receipts must be persisted
to the block storage backend.

---

## B. Test Evidence Summary

| Phase | Commit | Description | Testnet Blocks | PASS | FAIL | SKIP |
|-------|--------|-------------|---------------|------|------|------|
| 10-B | `dda5af7` | Stratum TCP miner path, submit_block_extended wired | 10 | 22 | 0 | 0 |
| 10-C | `1aa0ba5` | Stratum long soak, two-VPS; irx1 blocked by binary | 50 | 48 | 1 | 0 |
| 10-D | `844b7d5` | Full assignment/receipt/irx1/submit path proven | 10 | 30 | 0 | 0 |
| 10-E | `8aa432d` | Receipt regression soak, 30+5 blocks, restart, bogus | 35 | 62 | 0 | 4 |
| 10-F | `e15ce4e` | Two-VPS soak, 3x60 blocks, peer propagation confirmed | 180 | 106 | 0 | 2 |

**Cumulative PoAW-X testnet blocks:** 285  
**irx1 in coinbase (Phase 10-F):** 180/180 (100%)  
**submit_block_extended accepted (Phase 10-F):** 360  
**VPS-2 propagation confirmed:** heights 62, 122, 122 (restart), 184  
**Mainnet safety:** PASS all phases â€” no PoAW-X env in production on either VPS

**Phase 10-C FAIL note:** Stratum compiled without PoAW-X (binary rebuilt after cleanup).
Resolved in Phase 10-D by rebuilding both binaries from PoAW-X branch.

**Phase 10-E/10-F SKIP notes:**
- Port 39513 (disabled-mode ephemeral iriumd): not reliably responsive â€” timing issue, not a protocol bug.
- getblock 404: soak used wrong URL `/rpc/getblock`; correct path is `/rpc/block?height=N`.

---

## C. Public-Testnet Blocker Matrix

| Item | Status | Notes |
|------|--------|-------|
| Core PoAW-X protocol (assignment/receipt/irx1/submit) | **READY** | Proven 285 testnet blocks |
| Mainnet hard-disable | **READY** | Double-gated, verified both VPS |
| Stratum PoAW-X path (IRIUM_STRATUM_POAWX=1) | **READY** | 180/180 irx1 blocks |
| Two-node propagation | **READY WITH MANUAL OPS** | SSH tunnel workaround required |
| Public seed node setup | **NEEDS FIX** | No public seed node provisioned |
| Public firewall / ports | **NEEDS FIX** | Cloud firewall blocks 39510/39512 |
| VPS-2 P2P (SSH tunnel workaround) | **NEEDS FIX** | 127.0.0.2 not suitable for public peers |
| Anti-abuse / rate limits | **DEFER** | Global rate limit exists; per-worker PoAW-X limit not implemented |
| Testnet bootstrap docs | **NEEDS FIX** | No public doc; draft prepared this phase |
| Miner setup docs | **NEEDS FIX** | No public guide; draft prepared this phase |
| Stratum setup docs | **READY WITH MANUAL OPS** | POOL_STRATUM.md + PoAW-X env vars documented |
| RPC auth / token handling | **READY WITH MANUAL OPS** | IRIUM_RPC_TOKEN; set per deployment |
| Monitoring / logging | **READY WITH MANUAL OPS** | Structured logs exist; no metrics dashboard |
| Metrics endpoint | **DEFER** | /rpc/mining_metrics exists; no PoAW-X metrics |
| Peer discovery | **NEEDS FIX** | No public DNS seed; manual ADDNODE only |
| Chain reset policy | **DONE (11-C)** | docs/poaw-x-testnet-reset-rollback-policy.md |
| Testnet faucet / reward policy | **DEFER** | Devnet coins worthless; faucet needed for external testers |
| Block explorer for testnet | **DEFER** | irium-explorer exists; no public testnet instance |
| Wallet compatibility | **READY WITH MANUAL OPS** | Existing wallet works; no PoAW-X wallet UI |
| Rollback / shutdown plan | **NEEDS FIX** | Not documented for public testnet |
| Git branch hygiene | **READY** | Remote testnet branch deleted; main clean |
| Secret hygiene (committed files) | **READY WITH MANUAL OPS** | Testnet RPC tokens in scripts (low risk); PAT removed |
| Receipt in-memory only | **KNOWN GAP** | Lost on restart; miners re-submit; defer to production |
| Disabled-mode endpoints | **RESOLVED (11-C)** | 503 on assignment/receipt, poawx_mode=disabled confirmed |
| receipts_root insertion-order dependency | **HIGH RISK** | Multi-worker unsafe; needs canonical sort |
| Solution/proof not validated | **HIGH RISK** | `solution` opaque hex; no cryptographic check |
| Adaptive puzzle difficulty | **DEFER** | Hardcoded 1; fine for single-miner testnet |
| getblock endpoint path | **READY (11-C)** | Exists at /rpc/block?height=N; irx1_root field added in Phase 11-C |
| Cargo fmt drift | **DEFER** | Cosmetic; not checked this phase |
| Public communication plan | **NEEDS FIX** | No announcement plan |

**Hard blockers before any external participants:**
- Public seed node
- Cloud firewall opening (ports 39510, 39512)
- VPS-2 P2P without SSH tunnel
- Peer discovery (DNS seed or known seed list)
- receipts_root canonical sort (for multi-miner)
- Chain reset policy and rollback plan

---

## D. Network / Ops Architecture Plan

> **PLAN ONLY. No changes applied this phase.**

### Topology

```
Internet
    |
    +-- Public 39510/tcp --> VPS-1 testnet iriumd (seed node, IRIUM_NETWORK=devnet)
    +-- Public 39512/tcp --> VPS-1 testnet stratum (IRIUM_STRATUM_POAWX=1)
    |
    +-- SSH tunnel (VPS-2 initiates) --> VPS-2 testnet iriumd peer
    |   (temporary until cloud firewall opens 39610)
    |
    +-- RPC 39511/tcp --> PRIVATE ONLY (localhost / management VPN)
```

### Port Assignment

| Port | Service | Public |
|------|---------|--------|
| 39510 | testnet iriumd P2P (VPS-1) | YES (cloud firewall must open) |
| 39511 | testnet iriumd RPC (VPS-1) | NO (localhost only) |
| 39512 | testnet stratum (VPS-1) | YES (cloud firewall must open) |
| 39610 | testnet iriumd P2P (VPS-2) | NO (SSH tunnel workaround) |
| 39611 | testnet iriumd RPC (VPS-2) | NO (localhost only) |

### Data Directory Isolation

| Node | Data dir | Binary |
|------|---------|--------|
| VPS-1 | `/home/irium/irium-poawx-testnet/` | `/home/irium/irium/target/release/iriumd` |
| VPS-2 | `/home/irium/irium-poawx-testnet-vps2/` | Deployed from VPS-1 |

Must never overlap with production `~/.irium/`.

### Environment Variables

VPS-1 iriumd:
```
IRIUM_NETWORK=devnet
IRIUM_POAWX_MODE=active
IRIUM_DATA_DIR=/home/irium/irium-poawx-testnet
IRIUM_RPC_TOKEN=<rotate before each public run, keep private>
IRIUM_PORT=39510
IRIUM_RPC_PORT=39511
```

VPS-1 stratum:
```
IRIUM_STRATUM_POAWX=1
IRIUM_RPC_BASE=http://127.0.0.1:39511
IRIUM_RPC_TOKEN=<same as iriumd>
IRIUM_STRATUM_PORT=39512
```

### Rollback Plan

1. Stop testnet stratum and iriumd on both VPS.
2. Delete testnet data directories.
3. Re-deploy binary from clean testnet branch.
4. Restart with fresh genesis.
5. Mainnet services unaffected (separate ports, data dirs, services).

### Emergency Stop

1. `kill <testnet-iriumd-pid>` and `kill <testnet-stratum-pid>` on VPS-1.
2. SSH to VPS-2: `kill <testnet-iriumd-pid>`.
3. Kill SSH tunnel on VPS-2.
4. Close ports 39510 and 39512 at cloud firewall.
5. Mainnet remains running.

---

## E. Miner / Tester Onboarding

> See separate file: `docs/poaw-x-public-tester-miner-draft-guide.md`

Key points:
- Use PoAW-X soak harness: `scripts/poawx-stratum-long-soak-harness.py`
- Connect to stratum at `<VPS-1-IP>:39512` (TCP, Stratum v1)
- Username: any worker label. Password: `x`
- Do not use production wallet or mainnet seed phrase
- Testnet coins have zero monetary value
- Report logs if requested

---

## F. Security and Secret Hygiene Audit

### git remote URL

```
https://github.com/iriumlabs/irium.git
```
No token. CLEAN.

### Committed tokens (testnet scripts only)

| File | Token | Risk |
|------|-------|------|
| `scripts/testnet-poawx-phase10b-stratum-tcp-miner.sh` | `phase10b_soak_devnet` | LOW â€” testnet only |
| `scripts/testnet-poawx-phase10c-stratum-long-soak.sh` | `phase10c_soak_devnet` | LOW â€” testnet only |
| `scripts/testnet-poawx-phase10f-receipt-two-vps-soak.sh` | `poawx-phase10f-token` | LOW â€” testnet only; local branch only (remote deleted) |

No mainnet RPC tokens, private keys, wallet seeds, PATs, or VPS credentials in any
committed file. No production env file contents committed.

### PAT status

Removed from `.git/config` in Phase 10-F-Remote Cleanup.
Manual rotation from GitHub settings still recommended but not yet confirmed.

---

## G. Known Skips and Unresolved Items

| Item | Detail |
|------|--------|
| Port 39513 disabled-mode | Ephemeral iriumd not reliably responsive; 503 check skipped in 10-E/10-F. Timing issue, not protocol bug. |
| Solution proof not validated | `solution` field accepted as opaque hex; no cryptographic puzzle proof check. |
| receipts_root insertion order | Multi-worker root depends on arrival order; single-worker safe. |
| Receipt in-memory only | Lost on iriumd restart; miners re-submit after reconnect. |
| Public firewall not opened | Cloud firewall blocks 39510/39512 from public Internet. |
| External miners not onboarded | No external participants yet. |
| SSH tunnel workaround | Not suitable for arbitrary external peers. |
| Cargo fmt drift | Not checked. Cosmetic. |
| PAT rotation | Recommended; not confirmed completed. |
| VPS-1 explorer/wallet-api failed | Since mainnet double-restart at 16:10. Not related to PoAW-X. |

---

## H. Recommended Next Phases

| Phase | Goal |
|-------|------|
| **11-B** | Fix blockers: open firewall ports 39510/39512, fix VPS-2 P2P direct routing, add receipts_root canonical sort, basic solution hex format validation |
| **11-C** | Networking readiness: P2P audit, firewall plan, getblock irx1_root, disabled-mode fix, reset policy, tester guide — DONE (2026-06-12) |
| **11-D** | Limited external miner pilot: 1-3 trusted testers, confirm stratum PoAW-X with external participants |
| **11-E** | Public-testnet launch candidate: DNS seed, public explorer instance, chain reset policy, faucet |

Do not recommend mainnet PoAW-X activation. Unresolved: solution verification,
multi-worker receipts_root ordering, receipt persistence.
