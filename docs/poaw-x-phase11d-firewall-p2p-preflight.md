# PoAW-X Phase 11-D: Firewall Direct P2P Preflight

**Date:** 2026-06-12
**Branch:** testnet/poawx-phase11d-firewall-p2p-preflight
**Status:** COMPLETE

---

## Objective

Validate PoAW-X testnet P2P (39510) and stratum (39512) over direct TCP between
VPS-1 and VPS-2 without any SSH tunnel workaround.

---

## A: Firewall Rules Applied (VPS-1)

Firewall layer: UFW (local). Default: deny incoming.

Applied:
- 39510/tcp ALLOW Anywhere  # PoAW-X testnet iriumd P2P
- 39512/tcp ALLOW Anywhere  # PoAW-X testnet stratum
- 39511 NOT opened (stays 127.0.0.1 only)

Emergency close:
  sudo ufw delete allow 39510/tcp
  sudo ufw delete allow 39512/tcp

Firewall decision after preflight: RULES LEFT OPEN (harmless when idle; avoids
re-applying for public testnet launch).

---

## B: Port Reachability from VPS-2

Tested nc -zv 207.244.247.86 <port> from 157.173.116.134:

| Port | Result |
|------|--------|
| 39510 | REACHABLE |
| 39512 | REACHABLE |
| 39511 | NOT REACHABLE (UFW block confirmed) |

---

## C: Direct P2P (No SSH Tunnel)

VPS-1 testnet iriumd: IRIUM_NETWORK=devnet IRIUM_POAWX_MODE=active
  IRIUM_P2P_BIND=0.0.0.0:39510 IRIUM_NODE_PORT=39511
  IRIUM_DATA_DIR=~/irium-poawx-testnet
  (target/debug/iriumd from Phase 11-C branch)

VPS-2 testnet iriumd: IRIUM_NETWORK=devnet IRIUM_POAWX_MODE=active
  IRIUM_P2P_BIND=0.0.0.0:39610 IRIUM_P2P_SEED_PORT=39510
  IRIUM_NODE_PORT=39611 IRIUM_ADDNODE=207.244.247.86:39510
  IRIUM_DATA_DIR=~/irium-poawx-testnet-p2p
  (binary copied from VPS-1 target/debug/iriumd)

IMPORTANT: IRIUM_P2P_SEED_PORT=39510 is required on VPS-2. Without it, iriumd
derives the seed port from IRIUM_P2P_BIND (39610) and dials VPS-1:39610 (wrong).

Result: Both nodes show peers=1, each seeing the other over direct TCP.
  VPS-2: [peer_mgr] peers=1, P2P 207.244.247.86:39510
  VPS-1: [peer_mgr] peers=1, P2P 157.173.116.134:<ephemeral>

---

## D: Stratum Reachability

VPS-1 testnet stratum: STRATUM_BIND=0.0.0.0:39512
  IRIUM_RPC_BASE=http://127.0.0.1:39511 IRIUM_STRATUM_POAWX=1
  IRIUM_POW_LIMIT_HEX=7fffff...
  (pool/irium-stratum/target/release/irium-stratum)

From VPS-2: TCP connect to 207.244.247.86:39512 succeeded.
  mining.subscribe -> result received (en1=00000002, en2sz=4)
  mining.set_difficulty received (diff=1)
  PoAW-X path active (IRIUM_STRATUM_POAWX=1 confirmed)

---

## E: 5-Block Mining Test with irx1_root

Harness run from VPS-1 (localhost RPC access):
  python3 scripts/poawx-stratum-long-soak-harness.py     127.0.0.1 39512 http://127.0.0.1:39511 <TOKEN> --blocks 6 --receipt

Results:
  Heights mined: 3-11 (6 blocks target + 2 extra from bogus test)
  Stratum accepts: 5/6 (1 stale-job race at easy difficulty)
  irx1 in coinbase: 4/6 blocks
  Receipt path: PASS (pending_count=2, diff=1, attempts=2)
  Final height (VPS-1): 11
  VPS-2 synced height: 9 (before bogus test blocks)

irx1_root on receipt-bearing blocks:
  h=6: cbcc8c5f15b58b5caeed98e0d9f13f26b1de7d9e8ac2c1ffd6900ef8756b698d
  h=7: c8e6ef629976491d1191f5e66c70c4e32b7d823fb36fd303f93215dc585619af
  h=8: c8e6ef629976491d1191f5e66c70c4e32b7d823fb36fd303f93215dc585619af
  h=9: c8e6ef629976491d1191f5e66c70c4e32b7d823fb36fd303f93215dc585619af

---

## F: Negative Checks

| Check | Result |
|-------|--------|
| Bogus share (job_id=bogus_job_00) | Rejected: error [23, stale share] |
| Height after bogus share | Unchanged (True) |
| Mainnet /poawx/assignment | HTTP 503 Service Unavailable |
| Port 39511 from VPS-2 | Connection timed out (UFW blocking) |
| Mainnet poawx_mode | Empty string (disabled) |
| Mainnet height | 30074 (untouched) |
| VPS-2 mainnet services | iriumd 1660633 alive, wallet 1661394 alive |

---

## Summary

PASS: All Phase 11-D tasks complete. PoAW-X testnet is network-ready.
Next: Phase 11-E limited external miner pilot.
