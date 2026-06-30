# PoAW-X Phase 11-E: Limited Miner Pilot

**Date:** 2026-06-12
**Branch:** testnet/poawx-phase11e-limited-miner-pilot
**Status:** COMPLETE (VPS-2 simulation; real external miner package READY)

---

## Scope

Limited controlled PoAW-X testnet pilot:

- Network: isolated devnet (not mainnet)
- Pilot type: VPS-2 as external-miner simulation
- Real external miners: NOT RUN in this session (package prepared)
- Duration: approx. 30 min test window
- Public ports used: 39510 (P2P), 39512 (stratum)
- RPC 39511: private (127.0.0.1 only) throughout

---

## Pre-flight Safety Audit

VPS-1 mainnet confirmed intact:
  iriumd PID 1919705, stratum PIDs 1919708/1919710/1919713/1919715
  explorer PID 1920064, stats-proxy PID 1880921
  Ports: 38291, 38300, 3333, 8080
  No IRIUM_POAWX_MODE in mainnet iriumd env

VPS-2 mainnet confirmed intact:
  iriumd PID 1660633, wallet-api PID 1661394, explorer PID 1661402
  Ports: 38291, 38300, 8080
  No IRIUM_POAWX_MODE in mainnet iriumd env

No stale testnet processes, no SSH tunnels, all testnet ports free at start.

---

## Task C: VPS-1 Pilot Services

Isolated data dir:  ~/irium-poawx-phase11e/
Logs:               ~/irium-poawx-phase11e/logs/

iriumd:
  IRIUM_NETWORK=devnet IRIUM_POAWX_MODE=active
  IRIUM_P2P_BIND=0.0.0.0:39510 IRIUM_NODE_HOST=127.0.0.1 IRIUM_NODE_PORT=39511
  IRIUM_DATA_DIR=~/irium-poawx-phase11e
  Binary: target/debug/iriumd (Phase 11-D testnet build)

Stratum:
  STRATUM_BIND=0.0.0.0:39512 IRIUM_RPC_BASE=http://127.0.0.1:39511
  IRIUM_STRATUM_POAWX=1 STRATUM_DEFAULT_DIFF=1 IRIUM_STRATUM_VARDIFF_ENABLED=0
  Binary: pool/irium-stratum/target/release/irium-stratum

Confirmed:
  39510: 0.0.0.0 LISTEN (P2P)
  39511: 127.0.0.1 LISTEN (RPC, private)
  39512: 0.0.0.0 LISTEN (stratum)
  poawx_mode: active, bits: 207fffff

---

## Task D: VPS-2 External-Miner Simulation

Isolated data dir:  ~/irium-poawx-phase11e-peer/
Binary: ~/irium-poawx-testnet-p2p/iriumd-testnet

Startup:
  IRIUM_NETWORK=devnet IRIUM_POAWX_MODE=active
  IRIUM_P2P_BIND=0.0.0.0:39610 IRIUM_P2P_SEED_PORT=39510
  IRIUM_NODE_PORT=39611 IRIUM_ADDNODE=VPS1_PUBLIC_IP:39510

P2P result: peers=1 on both VPS (direct TCP, no SSH tunnel)
  VPS-1 log: heartbeat peers=1, P2P from 157.173.116.134
  VPS-2 log: heartbeat peers=1, dialing VPS1_PUBLIC_IP:39510

Stratum reachability from VPS-2:
  TCP connect to VPS1_PUBLIC_IP:39512: SUCCESS
  mining.subscribe: result received (en1=00000003, en2sz=4)
  mining.set_difficulty: diff=1

---

## Task E: Real External Miner

NOT RUN. No external miner details provided for this session.
External miner package: docs/poaw-x-limited-miner-pilot-guide.md (READY)
Status: READY for future pilot with trusted participants.

---

## Task F: Pilot Mining Test

Harness: poawx-stratum-long-soak-harness.py
Run from: VPS-1 (localhost RPC access)
Arguments: --blocks 12 --receipt --bogus

| Metric | Value |
|--------|-------|
| Blocks target | 12 |
| Blocks passed | 12/12 |
| Share accepts | 12 |
| Share rejects | 0 |
| irx1 in coinbase | 9/12 blocks |
| Receipt test | PASS (pending_count=1, diff=1, attempts=1) |
| Bogus share | Rejected: error [23, stale share], height unchanged |
| Elapsed | 27.5s |

irx1_root on receipt-bearing blocks:

| Height | irx1_root |
|--------|-----------|
| 3 | None (template fetched before receipt included) |
| 4 | ba57a3b6b62fdc97891fc52587e92d5c5ac9557022f7a8935ae0fce583113a82 |
| 5 | ba57a3b6b62fdc97891fc52587e92d5c5ac9557022f7a8935ae0fce583113a82 |
| 12 | ba57a3b6b62fdc97891fc52587e92d5c5ac9557022f7a8935ae0fce583113a82 |

VPS-2 sync after pilot: height 12 (matched VPS-1, direct P2P)

---

## Task G: Negative Checks

| Check | Expected | Result |
|-------|---------|--------|
| Bogus share | Rejected | error [23, stale share]; height unchanged |
| Invalid receipt (empty body) | Reject | HTTP 422 (JSON deserialization) |
| Mainnet /poawx/assignment | 503 | HTTP 503 |
| Port 39511 from VPS-2 | Not reachable | Connection timed out |
| RPC bind | localhost only | 127.0.0.1:39511 |
| Mainnet poawx_mode | Empty/disabled | "" (not active) |
| Mainnet height | Untouched | 30120 |
| VPS-2 mainnet iriumd | Untouched | PID 1660633 alive |
| origin/main | Unchanged | ea01149 |
| No PR | None | Confirmed |

---

## Task H: Monitoring and Emergency Stop

### Health checks

```bash
# Testnet iriumd height
curl -s http://127.0.0.1:39511/status

# Peer count (via heartbeat log)
tail -5 ~/irium-poawx-phase11e/logs/iriumd.log | grep heartbeat

# Stratum log
tail -10 ~/irium-poawx-phase11e/logs/stratum.log

# Block irx1_root at height N
# curl -s "http://127.0.0.1:39511/rpc/block?height=N" -H "Authorization: Bearer TOKEN"

# VPS-2 sync height
# ssh irium-eu "curl -s http://127.0.0.1:39611/status"

# Mainnet still alive
ps -p 1919705 -o pid,stat,comm --no-headers
```

### Emergency stop (pilot services only)

```bash
# VPS-1
fuser -k 39510/tcp 2>/dev/null; fuser -k 39512/tcp 2>/dev/null

# VPS-2
# kill $(cat /tmp/phase11e-vps2-iriumd.pid)

# Close firewall ports if needed
sudo ufw delete allow 39510/tcp
sudo ufw delete allow 39512/tcp
```

Firewall state after Phase 11-E: RULES LEFT OPEN (same as Phase 11-D decision).

### Log preservation

Logs at:
  VPS-1: ~/irium-poawx-phase11e/logs/iriumd.log
  VPS-1: ~/irium-poawx-phase11e/logs/stratum.log
  VPS-2: ~/irium-poawx-phase11e-peer/logs/iriumd.log

---

## Summary

All Phase 11-E tasks PASS (VPS-2 simulation). PoAW-X testnet is pilot-ready.

Next: Invite trusted external miners using docs/poaw-x-limited-miner-pilot-guide.md.
Blocker before broader public testnet: real external miner validation (Phase 11-E Task E).
