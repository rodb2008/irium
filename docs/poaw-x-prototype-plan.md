# PoAW-X Prototype Plan

**Last updated:** 2026-06-12 (Phase 11-E limited miner pilot COMPLETE)
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

**Cumulative testnet blocks with PoAW-X path exercised: ~296**

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
| Regression soak | DONE (v6, 2026-06-11) | PASS=17 FAIL=0 SKIP=0 — all receipt/SBE/irx1/mainnet-safety checks pass |
| Firewall | PENDING | Open cloud firewall ports 39510 and 39512 on VPS-1 |
| VPS-2 P2P | PENDING | Fix direct P2P (remove SSH tunnel dependency) |
| DNS seed | PENDING | Register testnet DNS seed or publish known seed list |
| Chain reset policy | PENDING | Document and implement chain reset procedure |

### Phase 11-C: Networking Readiness

| Item | Status | Description |
|------|--------|-------------|
| P2P audit | DONE | Cloud firewall blocks 39510/39512; exact rules documented |
| Direct P2P test | BLOCKED | Awaiting cloud firewall approval for port 39510 |
| getblock irx1_root | DONE | `/rpc/block` now returns `irx1_root` field |
| poawx_mode descriptor | DONE | getblocktemplate returns `disabled` (was ``) |
| Chain reset policy | DONE | `docs/poaw-x-testnet-reset-rollback-policy.md` |
| Network plan | DONE | `docs/poaw-x-public-testnet-network-plan.md` |
| Tester guide update | DONE | Phase 11-C hardening |
| Runbook update | DONE | Updated for Phase 11-C findings |
| Cloud firewall TCP 39510/39512 | DONE | UFW rules applied VPS-1; left open after preflight |
| systemd service files | PENDING | Templates documented; not yet installed |

### Phase 11-D: Firewall Direct P2P Preflight -- COMPLETE

| Item | Status |
|------|--------|
| UFW rules 39510/39512 on VPS-1 | DONE (left open) |
| Port reachability from VPS-2 | PASS |
| Direct VPS-to-VPS P2P (no SSH tunnel) | PASS: peers=1 both sides |
| Stratum TCP reachability from VPS-2 | PASS |
| 6-block mining test with irx1 | PASS: receipt_test=PASS |
| VPS-2 P2P sync to height 9 | PASS |
| Bogus share rejection | PASS |
| Disabled-mode 503 / RPC private | PASS |
| Mainnet isolation | PASS: height 30074 untouched |

### Phase 11-E: Limited Miner Pilot — COMPLETE (VPS-2 simulation)

**Branch:** `testnet/poawx-phase11e-limited-miner-pilot`

| Item | Status | Detail |
|------|--------|--------|
| VPS-1 pilot iriumd + stratum | PASS | poawx_mode=active, 39510/39512 public |
| VPS-2 simulation (external peer) | PASS | peers=1, stratum TCP reachable |
| Real external miner | NOT RUN | Package: docs/poaw-x-limited-miner-pilot-guide.md |
| 12-block pilot test | PASS | 12/12 blocks, 9/12 irx1, receipt PASS, bogus rejected |
| VPS-2 P2P sync | PASS | height=12 |
| Negative checks | PASS | 422/503/bogus/RPC-private/mainnet-intact |

---

### Phase 11-F: Public Testnet Launch Candidate

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
| `scripts/poawx-phase11b-canonical-receipts-validation.py` | Phase 11-B self-contained regression soak (v6, PASS=17) |
| `scripts/testnet-poawx-phase11b-canonical-receipts-validation.sh` | Phase 11-B wrapper (delegates to Python soak) |
| `docs/poaw-x-phase11c-networking-readiness.md` | Phase 11-C networking audit |
| `docs/poaw-x-public-testnet-network-plan.md` | Testnet network plan and service layout |
| `docs/poaw-x-testnet-reset-rollback-policy.md` | Testnet chain reset and rollback policy |
| `scripts/testnet-poawx-phase10f-receipt-two-vps-soak.sh` | Two-VPS soak automation |
| `docs/poaw-x-phase11a-public-testnet-readiness-audit.md` | Phase 11-A audit |
| `docs/poaw-x-phase11d-firewall-p2p-preflight.md` | Phase 11-D preflight results |
| `docs/poaw-x-phase11e-limited-miner-pilot.md` | Phase 11-E pilot results |
| `docs/poaw-x-limited-miner-pilot-guide.md` | External miner connection guide |
| `docs/poaw-x-public-testnet-draft-runbook.md` | Operator runbook (draft) |
| `docs/poaw-x-public-tester-miner-draft-guide.md` | Miner guide (draft) |
