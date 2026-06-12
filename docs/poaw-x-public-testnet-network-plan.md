# PoAW-X Public Testnet Network Plan

**Status:** READY FOR REAL MINER — Phase 11-F validation in progress
**Date:** 2026-06-12
**Phase:** 11-F

---

## Overview

This document describes the proposed network layout for the PoAW-X public testnet.
No changes have been applied to cloud firewall, DNS, or production services.
All items marked PENDING require explicit operator approval before execution.

---

## Node Topology

```
Internet (miners / testers)
    |
    +-- TCP 39512 --> VPS-1 testnet stratum (irium-stratum)
    |                       |
    |                       +-- HTTP 127.0.0.1:39511 --> VPS-1 testnet iriumd
    |
    +-- TCP 39510 --> VPS-1 testnet iriumd P2P
                            |
                            +-- TCP 39610 --> VPS-2 testnet iriumd P2P (peer)
```

---

## Port Assignments

| Service | VPS | Public Bind | Private Port | Status |
|---------|-----|-------------|--------------|--------|
| testnet iriumd P2P | VPS-1 | 0.0.0.0:39510 | — | PENDING firewall |
| testnet iriumd RPC | VPS-1 | 127.0.0.1:39511 | — | Ready (local only) |
| testnet stratum | VPS-1 | 0.0.0.0:39512 | — | PENDING firewall |
| testnet iriumd P2P | VPS-2 | 0.0.0.0:39610 | — | OPTIONAL |
| testnet iriumd RPC | VPS-2 | 127.0.0.1:39611 | — | OPTIONAL |

Mainnet ports (38291/38300/3333) are completely separate and must not be changed.

---

## Cloud Firewall Changes Required (PENDING USER APPROVAL)

### VPS-1 (207.244.247.86)

| Rule | Protocol | Port | Source | Purpose |
|------|----------|------|--------|---------|
| ADD | TCP inbound | 39510 | 0.0.0.0/0 | Testnet P2P |
| ADD | TCP inbound | 39512 | 0.0.0.0/0 | Testnet stratum |

Do NOT open port 39511 (testnet RPC — operator-only, stays local).

### VPS-2 (157.173.116.134) — Optional

| Rule | Protocol | Port | Source | Purpose |
|------|----------|------|--------|---------|
| ADD | TCP inbound | 39610 | 0.0.0.0/0 | VPS-2 testnet P2P |

---

## Environment Files (template — no secrets)

### VPS-1 testnet iriumd

```bash
# /etc/irium/testnet-iriumd.env
IRIUM_NETWORK=devnet
IRIUM_POAWX_MODE=active
IRIUM_P2P_BIND=0.0.0.0:39510
IRIUM_NODE_HOST=127.0.0.1
IRIUM_NODE_PORT=39511
IRIUM_DATA_DIR=/home/irium/irium-poawx-testnet
IRIUM_BOOTSTRAP_DIR=/home/irium/irium/bootstrap
IRIUM_DEV_EASY_BITS_TEMPLATE=1
IRIUM_RATE_LIMIT_PER_MIN=60000
# IRIUM_RPC_TOKEN=<set at deployment — never in git>
```

### VPS-1 testnet stratum

```bash
# /etc/irium/testnet-stratum.env
IRIUM_STRATUM_POAWX=1
IRIUM_RPC_BASE=http://127.0.0.1:39511
IRIUM_STRATUM_PORT=39512
IRIUM_POW_LIMIT_HEX=7fffff0000000000000000000000000000000000000000000000000000000000
# IRIUM_RPC_TOKEN=<same token as iriumd>
```

### VPS-2 testnet iriumd

```bash
# /etc/irium/testnet-iriumd-vps2.env
IRIUM_NETWORK=devnet
IRIUM_POAWX_MODE=active
IRIUM_P2P_BIND=0.0.0.0:39610
IRIUM_NODE_HOST=127.0.0.1
IRIUM_NODE_PORT=39611
IRIUM_ADDNODE=207.244.247.86:39510
IRIUM_DATA_DIR=/home/irium/irium-poawx-testnet-vps2
IRIUM_BOOTSTRAP_DIR=/home/irium/irium/bootstrap
IRIUM_DEV_EASY_BITS_TEMPLATE=1
# IRIUM_RPC_TOKEN=<same token>
```

---

## Proposed systemd Unit Files (not yet installed)

### testnet-iriumd.service (VPS-1)

```ini
[Unit]
Description=Irium PoAW-X Testnet Node
After=network-online.target
Wants=network-online.target

[Service]
User=irium
WorkingDirectory=/home/irium/irium
EnvironmentFile=/etc/irium/testnet-iriumd.env
ExecStart=/home/irium/irium/target/release/iriumd
Restart=on-failure
RestartSec=10
TimeoutStopSec=90
LimitNOFILE=65535
StandardOutput=append:/home/irium/irium-poawx-testnet/iriumd.log
StandardError=append:/home/irium/irium-poawx-testnet/iriumd.log

[Install]
WantedBy=multi-user.target
```

### testnet-stratum.service (VPS-1)

```ini
[Unit]
Description=Irium PoAW-X Testnet Stratum
After=testnet-iriumd.service
Requires=testnet-iriumd.service

[Service]
User=irium
EnvironmentFile=/etc/irium/testnet-stratum.env
ExecStart=/opt/irium-pool/irium-stratum/target/release/irium-stratum
Restart=on-failure
RestartSec=5
TimeoutStopSec=30
LimitNOFILE=65535
StandardOutput=append:/home/irium/irium-poawx-testnet/stratum.log
StandardError=append:/home/irium/irium-poawx-testnet/stratum.log

[Install]
WantedBy=multi-user.target
```

---

## DNS Seed Node Plan (NOT YET PROVISIONED)

A testnet DNS seed would allow miners to bootstrap without a hardcoded IP.

Proposed hostname: `testnet-seed.irium.io` (or subdomain of existing irium domain)

Requirements before provisioning:
1. Cloud firewall port 39510 open (Task A above)
2. VPS-1 testnet iriumd confirmed stable for 48+ hours
3. DNS TTL set to 60s for easy failover
4. DNS record points to VPS-1 public IP (207.244.247.86)

Do NOT provision DNS until both firewall rules and 48-hour stability are confirmed.

---

## Monitoring

```bash
# Quick testnet health check
echo "--- testnet iriumd ---"
curl -sf http://127.0.0.1:39511/status | python3 -c "
import sys, json
d = json.load(sys.stdin)
print(f'height={d["height"]} peers={d.get("peer_count",0)} poawx={d.get("poawx_mode","?")}')
"
echo "--- stratum ---"
nc -z -w3 127.0.0.1 39512 && echo STRATUM-OK || echo STRATUM-DOWN
echo "--- mainnet ---"
ss -lntp sport = :38300 | grep iriumd | head -1 && echo MAINNET-IRIUMD-OK
ss -lntp sport = :3333  | grep irium-stratum | head -1 && echo MAINNET-STRATUM-OK
```

---

## Rollback / Emergency

See `docs/poaw-x-testnet-reset-rollback-policy.md`.
