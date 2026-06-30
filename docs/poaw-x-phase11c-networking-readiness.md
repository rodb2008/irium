# PoAW-X Phase 11-C: Public Testnet Networking Readiness

**Branch:** `testnet/poawx-phase11c-networking-readiness`
**Date:** 2026-06-12
**Status:** AUDIT COMPLETE — blockers documented, code changes applied

---

## Summary

Phase 11-C audits the networking layer required for a public PoAW-X testnet and applies
two small code fixes to `iriumd`. Direct VPS-to-VPS P2P is blocked by the cloud firewall;
the exact required rules are documented below but NOT yet applied.

---

## Task A: Direct VPS-to-VPS P2P Audit

### Topology

| Host | Role | Public IP |
|------|------|-----------|
| VPS-1 | Seed node, stratum, iriumd | 207.244.247.86 |
| VPS-2 | Peer node, explorer, wallet | 157.173.116.134 |

### P2P Bind Behavior

Both VPS run mainnet iriumd with `IRIUM_P2P_BIND=0.0.0.0:38291`. iriumd binds to
`0.0.0.0:PORT` when specified — no local-IP filtering blocks external connections.
The bind behavior is correct for public testnet use.

### Firewall Test Results

Tested from VPS-2 to VPS-1 via `nc -z -w5`:

| Port | Purpose | Reachable from VPS-2 |
|------|---------|----------------------|
| 38291 | Mainnet iriumd P2P | YES (cloud firewall open) |
| 39510 | Testnet iriumd P2P | NO (cloud firewall blocked) |
| 39511 | Testnet iriumd RPC | NO (cloud firewall blocked) |
| 39512 | Testnet stratum | NO (cloud firewall blocked) |

### Root Cause

Cloud provider firewall blocks all ports not explicitly permitted. Mainnet P2P (38291)
is already open. Testnet ports have no rule. iriumd local-IP filtering is NOT involved.

This was the sole reason Phase 10-F required the SSH tunnel workaround.

### Required Firewall Rules (NOT YET APPLIED — PENDING USER APPROVAL)

```
Cloud provider firewall — VPS-1 (207.244.247.86)
TCP 39510  inbound  0.0.0.0/0   testnet iriumd P2P (seed node)
TCP 39512  inbound  0.0.0.0/0   testnet stratum (miner connections)

DO NOT OPEN without explicit instruction:
39511  testnet RPC (keep private, operator-only)
38291/38300/3333  mainnet (already managed, do not change)
```

Optional — VPS-2 as named public peer:
```
TCP 39610  inbound  0.0.0.0/0   testnet iriumd P2P on VPS-2
```

---

## Task B: Direct P2P Test

**Status: BLOCKED — cloud firewall blocks port 39510.**

The SSH tunnel workaround remains required until the firewall rule is applied.

Once the firewall rule is applied:
1. Start VPS-1 testnet iriumd with `IRIUM_P2P_BIND=0.0.0.0:39510`
2. Start VPS-2 testnet iriumd with `IRIUM_ADDNODE=207.244.247.86:39510`
3. Confirm peer count increases on both sides within 30s
4. Mine blocks on VPS-1, confirm VPS-2 syncs

No SSH tunnel needed after firewall is open.

---

## Task C: getblock RPC Audit and Fix

### Endpoint Status (all already existed)

| Endpoint | Query | Returns |
|----------|-------|---------|
| `GET /rpc/block` | `?height=N` | Single block by height |
| `GET /rpc/blocks` | `?from=N&count=M` | Up to 500 blocks |
| `GET /rpc/block_by_hash` | `?hash=HEX` | Block by 32-byte hash |

The earlier "getblock returns 404" was a harness bug — it used `/rpc/getblock`
(Bitcoin naming) instead of `/rpc/block?height=N`. The endpoint is implemented.

Auth: `check_rate_with_auth` — accessible without auth (rate-limited), or with
bearer auth to bypass rate limit. No mutation. Safe for public read access.

### Code Change: `irx1_root` field in block response

Added `fn irx1_root_from_block(block: &Block) -> Option<String>` that scans the
coinbase outputs for a 38-byte irx1 OP_RETURN (`0x6a 0x24 "irx1" <32-byte-root>`)
and returns the hex-encoded root, or `null` if absent.

Added `"irx1_root": irx1_root_from_block(block)` to `block_json_for`. All three
block endpoints now include this field. Purely read-only. Backward-compatible.
Returns `null` for all mainnet blocks and pre-Phase-11-B testnet blocks.

---

## Task D: Disabled-Mode Endpoint Stability

Tested with ephemeral iriumd on port 39521 — no `IRIUM_POAWX_MODE` set, `devnet`,
`IRIUM_DEV_EASY_BITS_TEMPLATE=1`. Clean startup and shutdown confirmed.

| Check | Expected | Result |
|-------|----------|--------|
| `GET /poawx/assignment` | 503 | PASS |
| `POST /poawx/receipt` | 503 | PASS |
| `getblocktemplate.poawx_mode` | "disabled" | PASS |
| `GET /rpc/block?height=0` irx1_root | null | PASS |
| `IRIUM_DEV_EASY_BITS_TEMPLATE` bits | 207fffff | PASS |
| Mainnet PIDs unchanged | 1919705/1919715 | PASS |

### Code Change: `poawx_mode` = `"disabled"` in getblocktemplate

Changed from `""` to `"disabled"` when POAWX is not active. The stratum only checks
`== "active"`, so this is non-breaking. External testers can now distinguish modes.

### Startup Constraint (documented)

Ephemeral testnet iriumd requires:
- `IRIUM_DATA_DIR` must be under `$HOME` (storage.rs rejects `/tmp` paths)
- `cwd=~/irium` (anchor signer path: `./bootstrap/trust/allowed_anchor_signers`)
- `IRIUM_BOOTSTRAP_DIR` pointing to dir with `anchors.json` + `trust/`

---

## Task E: Seed Node and Service Layout Plan

See `docs/poaw-x-public-testnet-network-plan.md`.

---

## Task F: Chain Reset / Rollback Policy

See `docs/poaw-x-testnet-reset-rollback-policy.md`.

---

## Task G: Tester Guide

See `docs/poaw-x-public-tester-miner-draft-guide.md` (updated this phase).

---

## Task H: Security/Secret Scan

Ran git grep on all `*.py`, `*.sh`, `*.md` files. All matches are documentation
references (env var name mentions, template placeholders). No actual secrets found.
Remote URL: `origin https://github.com/iriumlabs/irium.git` — no PAT.

---

## Code Changes

| File | Change |
|------|--------|
| `src/bin/iriumd.rs` | Added `irx1_root_from_block` helper + `"irx1_root"` in `block_json_for` |
| `src/bin/iriumd.rs` | `poawx_mode` returns `"disabled"` (was `""`) in getblocktemplate |

---

## Tests

| Test | Result |
|------|--------|
| `cargo build --bin iriumd` (debug) | PASS |
| `cd pool/irium-stratum && cargo test` | 29/29 PASS |
| Disabled-mode endpoint checks | 5/5 PASS |
| Mainnet PIDs unchanged after all tests | PASS |

---

## Remaining Blockers Before Phase 11-D

| Blocker | Status |
|---------|--------|
| Cloud firewall TCP 39510 on VPS-1 | PENDING USER APPROVAL |
| Cloud firewall TCP 39512 on VPS-1 | PENDING USER APPROVAL |
| Direct VPS-to-VPS P2P test | BLOCKED on firewall |
| DNS seed node | NOT YET |
| systemd service file installation | NOT YET |
