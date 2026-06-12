# PoAW-X Phase 11-F: Live Run Results

**Date:** 2026-06-12
**Branch:** testnet/poawx-phase11f-real-external-miner-validation
**Verdict:** READY FOR REAL MINER — control evidence complete, no external miner connected

---

## Run State

Phase 11-F live stack started 2026-06-12 ~03:02 UTC on VPS-1.

| Service | PID | Bind | Status |
|---------|-----|------|--------|
| iriumd (testnet) | 2073705 | P2P 0.0.0.0:39510, RPC 127.0.0.1:39511 | Running |
| irium-stratum | 2073828 | 0.0.0.0:39512 | Running |
| iriumd-testnet (VPS-2 peer) | 1745453 | P2P 0.0.0.0:39610, RPC 127.0.0.1:39611 | Running (control) |

Data dir: `~/irium-poawx-phase11f/`
Logs: `~/irium-poawx-phase11f/logs/`

Chain state at audit: height=6, VPS-2 synced peers=1

---

## Control Test Evidence (Phase 11-F prep — VPS-2 harness)

Six blocks synced from VPS-2 control harness run. Operator RPC confirmed
irx1_root on three of the six blocks.

| Height | irx1_root |
|--------|-----------|
| 1 | None (legacy) |
| 2 | None (legacy) |
| 3 | None (legacy) |
| 4 | 3afe341f77c8ca4eed66a51e30f7ea14c9737702e763aadaf5666a7394186c96 |
| 5 | 3afe341f77c8ca4eed66a51e30f7ea14c9737702e763aadaf5666a7394186c96 |
| 6 | 3afe341f77c8ca4eed66a51e30f7ea14c9737702e763aadaf5666a7394186c96 |

irx1_root confirmed non-null on heights 4, 5, 6 via private RPC 127.0.0.1:39511.
RPC token required; endpoint not publicly bound.

Control harness summary (from Phase 11-F prep session):
  Blocks accepted: 6/6
  irx1_root present: 3/6
  Receipt test: PASS
  Bogus share test (Phase 11-E methodology): PASS

---

## External Miner Validation

Stratum listening on 0.0.0.0:39512 since 2026-06-12 03:02:46 UTC.

| Metric | Value |
|--------|-------|
| Real external miner connected | NO |
| subscribe received | — |
| authorize received | — |
| mining.notify sent | — |
| Accepted shares | — |
| Rejected shares | — |
| Blocks submitted by external miner | — |
| irx1_root from external miner block | — |
| Connection duration | — |

No real external miner connected during this window.
Stack is healthy and accepting connections at 39512.

Stratum log (connection events only, 03:02:46–03:08 UTC):
  [03:02:46] INFO [stratum] listening on 0.0.0.0:39512 (cpuminer_compat, vardiff=false)
  [03:02:46] INFO [sse] connected to http://127.0.0.1:39511/events
  (no subscribe/authorize/share events recorded in window)

---

## Task G: Negative Checks (2026-06-12 ~03:08 UTC)

| Check | Expected | Actual | Result |
|-------|---------|--------|--------|
| Mainnet /poawx/assignment | HTTP 404 or 503 | 404 | PASS |
| Empty-body receipt POST | HTTP 422 | 422 | PASS |
| RPC 39511 bind | 127.0.0.1 only | 127.0.0.1:39511 only | PASS |
| Mainnet iriumd alive | PID 2065028 alive | 2065028 Ssl iriumd | PASS |
| Mainnet port 8080 | 127.0.0.1 only | 127.0.0.1:8080 only | PASS |
| origin/main | 5c945ee unchanged | 5c945ee | PASS |
| No open PR | None | None | PASS |
| Bogus share | Rejected | error 23 "no active job" | PASS |
| Stratum subscribe handshake | OK | session_id, extranonce issued | PASS |

**Bogus share detail:**
- Connected to 127.0.0.1:39512 via socket
- subscribe: OK — session ID and extranonce issued
- authorize with address containing 'l' (invalid base58): error 20 "invalid address"
- share submit: error 23 "no active job" — correctly rejected, no auth/job state

---

## Mainnet Safety

VPS-1 mainnet iriumd: PID 2065028, port 8080 (127.0.0.1), no IRIUM_POAWX_MODE=active.
VPS-1 stratum PIDs: 2065029/2065030/2065031/2065032 (production ports 3333/3335/3336).
VPS-2 mainnet iriumd: PID 1744330, port 8080 (127.0.0.1).

No mainnet services were touched. Mainnet continued serving during entire Phase 11-F window.

---

## Verdict

**READY FOR REAL MINER**

All stack validation checks pass. Control test confirms PoAW-X receipt injection,
irx1_root embedding, P2P sync, and stratum handshake work correctly in isolation.

Remaining blocker: at least 1 accepted share from a real non-harness external
miner connecting over the public internet to 207.244.247.86:39512.

PASS criteria (not yet met):
  [ ] External miner connected through public stratum 39512
  [ ] At least one share accepted
  [ ] irx1_root verified via private RPC for at least one accepted block
  [ ] RPC 39511 remained private (CONFIRMED)
  [ ] Mainnet untouched (CONFIRMED)

To complete Phase 11-F: invite a trusted external miner using
docs/poaw-x-real-miner-pilot-invite.md, then re-run this window.
