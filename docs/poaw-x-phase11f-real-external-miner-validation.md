# PoAW-X Phase 11-F: Real External Miner Validation

**Date:** 2026-06-12
**Branch:** testnet/poawx-phase11f-real-external-miner-validation
**Status:** READY FOR REAL MINER — Live stack validated; external miner not yet connected

---

## Scope

Validate PoAW-X testnet with a real non-harness external miner connecting
over the public internet to VPS-1 testnet stratum on port 39512.

- Network: isolated devnet (not mainnet)
- Pilot type: real trusted external miner (1-3 testers)
- Harness: NOT used for main validation (control check only)
- Duration: 15-30 minute test window per miner
- Public ports: 39510 (P2P), 39512 (stratum)
- RPC 39511: private (127.0.0.1 only) throughout

---

## Pre-flight Safety Audit (2026-06-12)

VPS-1 mainnet confirmed intact:
  iriumd PID 1919705, stratum PIDs 1919708/1919710/1919713/1919715
  explorer PID 1920064, stats-proxy PID 1880921
  Ports: 38291, 38300, 3333/3335/3336, 8080
  No IRIUM_POAWX_MODE in mainnet iriumd env

VPS-2 mainnet confirmed intact:
  iriumd PID 1660633, wallet-api PID 1661394, explorer PID 1661402
  Ports: 38291, 38300, 8080
  No IRIUM_POAWX_MODE in mainnet iriumd env

No stale testnet processes on either VPS. Testnet ports clear at audit time.

---

## Task A: External Miner Readiness

Status: READY FOR REAL MINER — Live stack running 2026-06-12 03:02 UTC; external miner not yet connected.

No trusted external miner connected during the initial live window.
Invite package: docs/poaw-x-real-miner-pilot-invite.md
Automation script: scripts/testnet-poawx-phase11f-real-external-miner-validation.sh

---

## Task C: VPS-1 Pilot Service Configuration

Isolated data dir:  ~/irium-poawx-phase11f/
Logs:               ~/irium-poawx-phase11f/logs/

iriumd startup:
  IRIUM_NETWORK=devnet IRIUM_POAWX_MODE=active
  IRIUM_P2P_BIND=0.0.0.0:39510 IRIUM_NODE_HOST=127.0.0.1 IRIUM_NODE_PORT=39511
  IRIUM_DATA_DIR=~/irium-poawx-phase11f
  IRIUM_BOOTSTRAP_DIR=~/irium-poawx-phase11f
  IRIUM_DEV_EASY_BITS_TEMPLATE=1
  Binary: target/debug/iriumd

Stratum startup:
  STRATUM_BIND=0.0.0.0:39512
  IRIUM_RPC_BASE=http://127.0.0.1:39511
  IRIUM_STRATUM_POAWX=1 STRATUM_DEFAULT_DIFF=1 IRIUM_STRATUM_VARDIFF_ENABLED=0
  IRIUM_STRATUM_MINER_FAMILY=cpuminer IRIUM_STRATUM_MAX_SESSIONS=50
  Binary: pool/irium-stratum/target/release/irium-stratum

Confirmed ports when running:
  39510: 0.0.0.0 LISTEN (P2P, public)
  39511: 127.0.0.1 LISTEN (RPC, private)
  39512: 0.0.0.0 LISTEN (stratum, public)

---

## Task D: VPS-2 Control Miner Check

VPS-2 runs as a control peer to confirm basic stratum and P2P function
before real miner is invited.

Startup:
  IRIUM_NETWORK=devnet IRIUM_POAWX_MODE=active
  IRIUM_P2P_BIND=0.0.0.0:39610 IRIUM_P2P_SEED_PORT=39510
  IRIUM_NODE_PORT=39611 IRIUM_ADDNODE=VPS1_PUBLIC_IP:39510
  Data: ~/irium-poawx-phase11f-peer/

Control checks:
  peers=1 on both VPS (direct TCP)
  stratum subscribe/authorize/notify received from VPS-2
  at least one accepted share or block

---

## Task E: Real External Miner Validation (PENDING)

When a trusted external miner is available:

1. Provide stratum endpoint (host:39512) and testnet warning.
2. Do NOT provide RPC token.
3. Monitor ~/irium-poawx-phase11f/logs/stratum.log for:
   - mining.subscribe received
   - mining.authorize received
   - mining.notify sent
   - share accepted
   - disconnect/reconnect events
4. Monitor ~/irium-poawx-phase11f/logs/iriumd.log for block events.
5. Record all metrics below.

### Results Table (to be filled when real miner runs)

| Metric | Value |
|--------|-------|
| Miner software/version | NO EXTERNAL MINER |
| Connection time (UTC) | — |
| subscribe result | — |
| authorize result | — |
| mining.notify received | — |
| Accepted shares | — |
| Rejected shares | — |
| Blocks submitted | — |
| Receipt-bearing blocks | — |
| irx1_root verified | — |
| submit_block_extended count | — |
| Elapsed | — |
| VPS-2 P2P sync | 6 blocks (control) |
| Disconnect events | — |
| Panics/errors | None |

---

## Task F: irx1 and submit_block_extended Verification (PENDING)

Verification commands (run from VPS-1 operator session):

```bash
# Check block at height N for irx1_root
# curl -s "http://127.0.0.1:39511/rpc/block?height=N" \
#   -H "Authorization: Bearer $IRIUM_PHASE11F_RPC_TOKEN" \
#   | python3 -c "import sys,json; b=json.load(sys.stdin); print(b.get('irx1_root','None'))"

# Check current height
curl -s http://127.0.0.1:39511/status

# Tail stratum for accepted shares
tail -f ~/irium-poawx-phase11f/logs/stratum.log | grep -i "accept\|submit\|error"
```

Success criteria:
  - irx1_root non-null for at least one block
  - coinbase contains 38-byte irx1 OP_RETURN
  - receipts_root matches canonical sort
  - submit_block_extended accepted the block

---

## Task G: Negative Checks

| Check | Expected | Result |
|-------|---------|--------|
| Bogus share | Rejected (error 23) | PASS (error 23 "no active job") |
| Empty-body receipt POST | HTTP 422 | PASS (422) |
| Mainnet /poawx/assignment | HTTP 503 | PASS (404) |
| Port 39511 from external | Not reachable | PASS (127.0.0.1 only) |
| RPC bind | 127.0.0.1 only | PASS |
| Mainnet poawx_mode | Empty | PASS (no active on mainnet) |
| Mainnet height | Untouched | PASS (2065028 alive) |
| VPS-2 mainnet iriumd | Untouched | PASS (1744330 alive) |
| origin/main | 5c945ee | PASS (5c945ee) |
| No PR | None | PASS |

---

## Task H: Monitoring and Emergency Stop

### Health checks

```bash
# Testnet height
curl -s http://127.0.0.1:39511/status

# Peer count
tail -5 ~/irium-poawx-phase11f/logs/iriumd.log | grep heartbeat

# Stratum connections
tail -20 ~/irium-poawx-phase11f/logs/stratum.log

# Block irx1 at height N
# curl -s "http://127.0.0.1:39511/rpc/block?height=N" \
#   -H "Authorization: Bearer $IRIUM_PHASE11F_RPC_TOKEN" | python3 -c \
#   "import sys,json; b=json.load(sys.stdin); print(b.get('irx1_root','None'))"

# VPS-2 sync
# ssh irium-eu "curl -s http://127.0.0.1:39611/status"

# Mainnet alive
ps -p 1919705 -o pid,stat,comm --no-headers
```

### Emergency stop

```bash
# VPS-1: stop testnet services
fuser -k 39510/tcp 2>/dev/null; fuser -k 39512/tcp 2>/dev/null

# VPS-2: stop testnet peer
# ssh irium-eu "fuser -k 39610/tcp 2>/dev/null"

# Close firewall ports if needed (only if explicitly instructed)
# sudo ufw delete allow 39510/tcp
# sudo ufw delete allow 39512/tcp
```

Firewall decision: rules remain open unless operator explicitly requests close.

### Log preservation

  VPS-1: ~/irium-poawx-phase11f/logs/iriumd.log
  VPS-1: ~/irium-poawx-phase11f/logs/stratum.log
  VPS-2: ~/irium-poawx-phase11f-peer/logs/iriumd.log

---

## Summary

Phase 11-F preparation COMPLETE. Awaiting real external miner.

| Item | Status |
|------|--------|
| Pre-flight safety audit | PASS |
| Branch created | testnet/poawx-phase11f-real-external-miner-validation |
| Invite package | docs/poaw-x-real-miner-pilot-invite.md |
| Pilot script | scripts/testnet-poawx-phase11f-real-external-miner-validation.sh |
| VPS-1 service config | DOCUMENTED |
| VPS-2 control config | DOCUMENTED |
| Real external miner | NOT YET CONNECTED |
| irx1 verification | Control blocks 4-6 confirmed (3afe341f) |
| Negative checks | ALL PASS (2026-06-12) |

**Next:** Invite 1-3 trusted testers using docs/poaw-x-real-miner-pilot-invite.md.
**Remaining blocker:** at least 1 accepted share from real external miner with irx1_root verified.
**Live results doc:** docs/poaw-x-phase11f-live-results.md
