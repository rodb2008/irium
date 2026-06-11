# PoAW-X Prototype Plan

**Last updated:** 2026-06-11 (Phase 11-A)  
**Status:** Active development â€” testnet proven, public testnet pending

---

## Overview

PoAW-X (Proof of Assigned Work â€” Extended) adds a secondary work commitment layer to
Irium. Miners must solve an assigned CPU puzzle and commit its receipt on-chain via an
`irx1` OP_RETURN in the coinbase before a block is accepted as PoAW-X-validated.

---

## Completed Phases

### Phase 9: Foundation

| Sub-phase | Description | Result |
|-----------|-------------|--------|
| 9-A | PoAW-X readiness audit | Design complete |
| 9-B | Receipt wiring in iriumd | Merged to testnet branch |
| 9-C | Template difficulty integration | Merged to testnet branch |
| 9-D | Stratum PoAW-X path | Merged to testnet branch |
| 9-E | Local readiness audit | PASS |
| 9-F | Two-VPS validation | PASS |

### Phase 10: Soak and Validation

| Sub-phase | Commit | Description | Blocks | Result |
|-----------|--------|-------------|--------|--------|
| 10-A | (pre-10B) | Long soak (single VPS) | ~100 | PASS |
| 10-B | `dda5af7` | Real Stratum TCP miner test | 10 | PASS=22 FAIL=0 |
| 10-C | `1aa0ba5` | Stratum long soak, two-VPS | 50 | PASS=48 FAIL=1* |
| 10-D | `844b7d5` | Full assignment/receipt path | 10 | PASS=30 FAIL=0 |
| 10-E | `8aa432d` | Receipt regression soak | 35 | PASS=62 FAIL=0 SKIP=4 |
| 10-F | `e15ce4e` | Two-VPS receipt soak, 180 blocks | 180 | PASS=106 FAIL=0 SKIP=2 |

*Phase 10-C FAIL: stratum binary compiled without PoAW-X; resolved in 10-D.

**Cumulative testnet blocks with PoAW-X path exercised: 285**

### Phase 10-F Post-ops

| Step | Commit | Description |
|------|--------|-------------|
| 10-F Safety Audit | `a9b489e` | Confirmed origin/main clean, no PoAW-X on remote |
| 10-F Remote Cleanup | `a5e5feb` | Remote testnet branch deleted; PAT removed from config |

---

## Current Phase

### Phase 11-A: Public Testnet Readiness Audit

**Branch:** `testnet/poawx-phase11a-public-testnet-readiness`  
**Status:** COMPLETE (audit only)  
**Commit:** (this file)

**Key findings:**
- Protocol core is READY for single-miner testnet.
- 2 HIGH RISK items for multi-miner: receipts_root sort order, solution validation.
- 8 NEEDS FIX items before external participants: seed node, firewall, DNS, docs, reset policy.
- 7 DEFERRED items acceptable for first testnet pilot.
- Mainnet hard-disable verified solid on both VPS.

**Readiness verdict:** READY for internal single-miner testnet. NOT YET READY for external participants.

---

## Planned Phases

### Phase 11-B: Blocker Fixes

| Item | Status | Description |
|------|--------|-------------|
| receipts_root canonical sort | DONE (163b558) | Both iriumd + stratum sort before hashing |
| Solution proof validation | DONE (163b558) | commitment_nonce + SHA256d PoW check at POST and submit_block_extended |
| Firewall | PENDING | Open cloud firewall ports 39510 and 39512 on VPS-1 |
| VPS-2 P2P | PENDING | Fix direct P2P (remove SSH tunnel dependency) |
| DNS seed | PENDING | Register testnet DNS seed or publish known seed list |
| Chain reset policy | PENDING | Document and implement chain reset procedure |

### Phase 11-C: Operator Runbook

| Item | Description |
|------|-------------|
| systemd service files | Testnet iriumd and stratum service files (separate from mainnet) |
| Log rotation | logrotate config for testnet logs |
| Monitoring | Key log patterns and alerting |
| Runbook | finalize `docs/poaw-x-public-testnet-draft-runbook.md` |

### Phase 11-D: Limited External Miner Pilot

- Onboard 1-3 trusted external testers.
- Confirm Stratum PoAW-X path with external participants.
- Collect feedback on miner guide.
- Confirm irx1 end-to-end with non-harness miner (if available).

### Phase 11-E: Public Testnet Launch Candidate

- Public DNS seed.
- Public block explorer instance.
- Faucet for testnet coins.
- Finalize miner guide and operator runbook.
- Announce to community.

---

## Protocol Status

| Component | Status |
|-----------|--------|
| `/poawx/assignment` | READY |
| `/poawx/receipt` | READY (single-miner) |
| `/rpc/getblocktemplate` PoAW-X fields | READY |
| `/rpc/submit_block_extended` | READY |
| irx1 OP_RETURN format | FINAL (38 bytes) |
| receipts_root algorithm | CANONICAL SORT (Phase 11-B, multi-miner safe) |
| Stratum irx1 injection | READY |
| Mainnet hard-disable | SOLID |
| Solution proof validation | IMPLEMENTED (Phase 11-B, commitment_nonce + PoW) |
| Receipt persistence | IN-MEMORY ONLY (deferred) |
| Adaptive puzzle difficulty | DEFERRED (hardcoded 1) |

---

## Mainnet Activation

PoAW-X mainnet activation is NOT planned and NOT recommended at this time.
Required before mainnet consideration:
- Solution proof cryptographic validation
- Multi-worker receipts_root canonical ordering
- Receipt persistence across restarts
- Extended public testnet validation (Phase 11-D/E at minimum)
- Security audit

---

## Architecture Reference

```
Miner
  |
  +-- GET /poawx/assignment --> iriumd (seed, commitment_nonce, pow_bits)
  +-- [solve puzzle locally]
  +-- POST /poawx/receipt --> iriumd (stores receipt in pending)
  |
  +-- TCP Stratum connect --> irium-stratum
        |
        +-- GET /rpc/getblocktemplate --> iriumd (pending_receipts, receipts_root)
        +-- [inject irx1 OP_RETURN into coinbase]
        +-- [miner finds nonce]
        +-- POST /rpc/submit_block_extended --> iriumd (validates irx1, clears receipts)
```

---

## Key Files

| File | Description |
|------|-------------|
| `src/bin/iriumd.rs` | `/poawx/assignment`, `/poawx/receipt`, `submit_block_extended`, `getblocktemplate` |
| `pool/irium-stratum/src/stratum.rs` | Stratum irx1 injection and submit_block_extended dispatch |
| `pool/irium-stratum/src/block.rs` | `compute_receipts_root_from_pending`, `build_irx1_commitment_script` |
| `pool/irium-stratum/src/template.rs` | `GetBlockTemplate` PoAW-X fields |
| `scripts/poawx-stratum-long-soak-harness.py` | Test harness for PoAW-X stratum path |
| `scripts/testnet-poawx-phase10f-receipt-two-vps-soak.sh` | Two-VPS soak automation |
| `docs/poaw-x-phase11a-public-testnet-readiness-audit.md` | Phase 11-A audit |
| `docs/poaw-x-public-testnet-draft-runbook.md` | Operator runbook (draft) |
| `docs/poaw-x-public-tester-miner-draft-guide.md` | Miner guide (draft) |
