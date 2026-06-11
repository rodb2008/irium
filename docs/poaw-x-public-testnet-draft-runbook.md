# PoAW-X Public Testnet Draft Runbook

**Status:** DRAFT â€” not yet applied. For planning purposes only.  
**Date:** 2026-06-11  
**Phase:** 11-A planning output

---

## Prerequisites

- VPS-1 (207.244.247.86): Irium node + stratum operator access
- VPS-2 (157.173.116.134): Second peer node operator access
- Cloud firewall control for both VPS machines
- Build: `cargo build --release` on Phase 11-A or later branch

## 1. Build Testnet Binary

On VPS-1:
```bash
cd ~/irium
git checkout testnet/poawx-phase11a-public-testnet-readiness
source ~/.cargo/env
cargo build --release 2>&1 | tail -5
ls -la target/release/iriumd
ls -la pool/irium-stratum/target/release/irium-stratum
```

Deploy to VPS-2:
```bash
scp target/release/iriumd irium@157.173.116.134:/tmp/iriumd-poawx-testnet
```

## 2. Prepare Data Directories

On VPS-1:
```bash
TESTNET_DIR=/home/irium/irium-poawx-testnet
mkdir -p "$TESTNET_DIR"
cp ~/irium/bootstrap/anchors.json "$TESTNET_DIR/"
cp ~/irium/bootstrap/trust.json "$TESTNET_DIR/" 2>/dev/null || true
```

On VPS-2:
```bash
TESTNET_DIR=/home/irium/irium-poawx-testnet-vps2
mkdir -p "$TESTNET_DIR"
# SCP anchors/trust from VPS-1:
scp irium@207.244.247.86:~/irium/bootstrap/anchors.json "$TESTNET_DIR/"
```

## 3. Open Cloud Firewall (Phase 11-B prerequisite)

> Not done in Phase 11-A. Required before Phase 11-D.

Open on VPS-1 at cloud provider:
- TCP 39510 inbound (testnet iriumd P2P)
- TCP 39512 inbound (testnet stratum)

Do NOT open:
- 39511 (testnet RPC â€” keep private)
- 38300/38310/38320/3333/8080 (mainnet â€” already managed)

## 4. Start VPS-1 Testnet iriumd

```bash
cd /home/irium/irium-poawx-testnet
IRIUM_NETWORK=devnet \
IRIUM_POAWX_MODE=active \
IRIUM_DATA_DIR=/home/irium/irium-poawx-testnet \
IRIUM_RPC_TOKEN=<your-testnet-rpc-token> \
IRIUM_PORT=39510 \
IRIUM_RPC_PORT=39511 \
  /home/irium/irium/target/release/iriumd \
  >> /home/irium/irium-poawx-testnet/iriumd.log 2>&1 &
echo $! > /tmp/testnet-iriumd.pid
```

Verify:
```bash
curl http://127.0.0.1:39511/rpc/getblocktemplate | python3 -m json.tool | grep -E 'height|poawx_mode'
```

Expected: `"poawx_mode": "active"`, `"height": 1`.

## 5. Start VPS-1 Testnet Stratum

```bash
IRIUM_STRATUM_POAWX=1 \
IRIUM_RPC_BASE=http://127.0.0.1:39511 \
IRIUM_RPC_TOKEN=<same-token> \
IRIUM_STRATUM_PORT=39512 \
  /opt/irium-pool/irium-stratum/target/release/irium-stratum \
  >> /home/irium/irium-poawx-testnet/stratum.log 2>&1 &
echo $! > /tmp/testnet-stratum.pid
```

Verify:
```bash
nc -z 127.0.0.1 39512 && echo stratum_open || echo stratum_not_ready
```

## 6. Start VPS-2 Testnet iriumd (SSH Tunnel Method)

> Current workaround due to cloud firewall blocking 39510/39610 between VPS.

On VPS-2, initiate forward SSH tunnel:
```bash
ssh -f -N -L 127.0.0.2:39510:127.0.0.1:39510 \
  -o StrictHostKeyChecking=no -o BatchMode=yes \
  -o ServerAliveInterval=30 -o ServerAliveCountMax=60 \
  irium@207.244.247.86
# Note the PID for cleanup
cat /tmp/vps2-tunnel.pid
```

Then start VPS-2 iriumd:
```bash
cd /home/irium/irium-poawx-testnet-vps2
IRIUM_NETWORK=devnet \
IRIUM_POAWX_MODE=active \
IRIUM_DATA_DIR=/home/irium/irium-poawx-testnet-vps2 \
IRIUM_RPC_TOKEN=<same-token> \
IRIUM_PORT=39610 \
IRIUM_RPC_PORT=39611 \
IRIUM_ADDNODE=127.0.0.2:39510 \
  /tmp/iriumd-poawx-testnet \
  >> /home/irium/irium-poawx-testnet-vps2/iriumd.log 2>&1 &
```

Why `127.0.0.2` and not `127.0.0.1`: `iriumd::local_ip_set()` hardcodes `127.0.0.1`
as self-address and silently filters ADDNODE peers matching it. `127.0.0.2` routes to
the loopback interface on Linux but is NOT in the filtered set.

## 7. Verify Peer Propagation

On VPS-1:
```bash
curl http://127.0.0.1:39511/rpc/getblocktemplate | python3 -c "import sys,json; t=json.load(sys.stdin); print('h=', t['height'])"
```

On VPS-2:
```bash
curl http://127.0.0.1:39611/rpc/getblocktemplate | python3 -c "import sys,json; t=json.load(sys.stdin); print('h=', t['height'])"
```

Both heights should converge within ~30 seconds.

## 8. Run PoAW-X Soak Harness

```bash
# On VPS-1
cd ~/irium
python3 scripts/poawx-stratum-long-soak-harness.py \
  --blocks 30 \
  --stratum 127.0.0.1:39512 \
  --rpc http://127.0.0.1:39511 \
  --receipt \
  --rpc-token <testnet-rpc-token>
```

Expected per block: `irx1=True strat=True blk=True`

## 9. Monitor Logs

VPS-1 iriumd: `tail -f /home/irium/irium-poawx-testnet/iriumd.log`

Key lines to watch:
```
[poawx] assignment height=N ...
[poawx] receipt stored height=N ... pending_count=1
[poawx] to_job: job=... mode=active pending=1 irx1_len=38
[poawx] block_extended accepted height=N cleared_receipts=1 remaining=0
[submit_block_extended] accepted height=N tip=...
```

## 10. Graceful Shutdown

```bash
kill $(cat /tmp/testnet-stratum.pid) 2>/dev/null || true
kill $(cat /tmp/testnet-iriumd.pid) 2>/dev/null || true

# On VPS-2:
ssh irium@157.173.116.134 "kill \$(cat /tmp/testnet-iriumd-vps2.pid) 2>/dev/null || true"
ssh irium@157.173.116.134 "kill \$(cat /tmp/vps2-tunnel.pid) 2>/dev/null || true"
```

## 11. Chain Reset Procedure

If chain reset is needed:
```bash
# Stop all testnet processes (see Section 10)
# Delete only the state dir, keep blocks dir for archive:
rm -rf /home/irium/irium-poawx-testnet/state
# Or full reset:
rm -rf /home/irium/irium-poawx-testnet
mkdir -p /home/irium/irium-poawx-testnet
cp ~/irium/bootstrap/anchors.json /home/irium/irium-poawx-testnet/
# Restart from Section 4
```

## 12. Safety Checklist Before Every Run

- [ ] Mainnet iriumd still alive: `fuser 38300/tcp`
- [ ] Mainnet stratum still alive: `fuser 3333/tcp`
- [ ] No POAWX in mainnet env: `cat /proc/<mainnet-pid>/environ | tr '\0' '\n' | grep POAWX`
- [ ] Testnet ports isolated from mainnet: testnet uses 39510-39512 only
- [ ] Testnet data dir is NOT `~/.irium/`
- [ ] RPC token set and not committed to git
- [ ] cloud firewall: mainnet ports unchanged

## 13. Incident Response

| Symptom | Action |
|---------|--------|
| Testnet iriumd crash | Check log for PANIC; restart from Section 4; miners re-submit receipts |
| Stratum crash | Restart from Section 5; miners auto-reconnect |
| VPS-2 loses sync | Kill tunnel + iriumd on VPS-2; restart from Section 6 |
| Mainnet service affected | STOP all testnet processes; investigate before resuming |
| Block height stuck | Check peers count in iriumd log; check tunnel alive; restart if needed |

---

*This runbook is draft only. Apply Phase 11-B fixes (firewall, DNS seed, direct P2P)
before using for external participants.*
