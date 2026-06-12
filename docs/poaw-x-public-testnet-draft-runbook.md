# PoAW-X Public Testnet Draft Runbook

**Status:** DRAFT — Phase 11-E limited miner pilot COMPLETE. Ready for external participants.
**Date:** 2026-06-12
**Phase:** 11-E

---

## Prerequisites

- VPS-1 (207.244.247.86): Irium node + stratum + operator access
- VPS-2 (157.173.116.134): Second peer node operator access
- Cloud firewall: TCP 39510 and 39512 open on VPS-1 (PENDING APPROVAL)
- Build: `cargo build --release` on `testnet/poawx-phase11c-networking-readiness` or later

## 1. Build Testnet Binary

On VPS-1:
```bash
cd ~/irium
git checkout testnet/poawx-phase11c-networking-readiness
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
mkdir -p "$TESTNET_DIR/state" "$TESTNET_DIR/blocks" "$TESTNET_DIR/logs"
cp ~/irium/bootstrap/anchors.json "$TESTNET_DIR/"
cp -r ~/irium/bootstrap/trust "$TESTNET_DIR/"
```

On VPS-2:
```bash
TESTNET_DIR=/home/irium/irium-poawx-testnet-vps2
mkdir -p "$TESTNET_DIR/state" "$TESTNET_DIR/blocks"
scp irium@207.244.247.86:~/irium/bootstrap/anchors.json "$TESTNET_DIR/"
scp -r irium@207.244.247.86:~/irium/bootstrap/trust "$TESTNET_DIR/"
```

## 3. Open Cloud Firewall (PENDING APPROVAL — do not open without explicit instruction)

Required on VPS-1 at cloud provider:
- TCP 39510 inbound (testnet iriumd P2P)
- TCP 39512 inbound (testnet stratum)

Do NOT open:
- 39511 (testnet RPC — keep private)
- 38300/38291/3333 (mainnet — already managed, do not change)

See `docs/poaw-x-public-testnet-network-plan.md` for exact rule details.

## 4. Start VPS-1 Testnet iriumd

**IMPORTANT:** Must run from `cwd=~/irium` for anchor signer path resolution.

```bash
cd /home/irium/irium  # required cwd
IRIUM_NETWORK=devnet IRIUM_POAWX_MODE=active IRIUM_DATA_DIR=/home/irium/irium-poawx-testnet IRIUM_BOOTSTRAP_DIR=/home/irium/irium-poawx-testnet IRIUM_RPC_TOKEN=<your-testnet-rpc-token> IRIUM_P2P_BIND=0.0.0.0:39510 IRIUM_NODE_HOST=127.0.0.1 IRIUM_NODE_PORT=39511 IRIUM_DEV_EASY_BITS_TEMPLATE=1   target/release/iriumd   >> /home/irium/irium-poawx-testnet/iriumd.log 2>&1 &
echo $! > /tmp/testnet-iriumd.pid
```

Verify (wait ~5s):
```bash
curl -sf http://127.0.0.1:39511/status | python3 -c "import sys,json; d=json.load(sys.stdin); print('height=', d['height'], 'poawx=', d.get('poawx_mode','?'))"
curl -sf -H "Authorization: Bearer <token>" http://127.0.0.1:39511/rpc/getblocktemplate | python3 -c "import sys,json; d=json.load(sys.stdin); print('poawx_mode:', d.get('poawx_mode'))"
```

Expected: `poawx_mode: active`, `height: 1`.

## 5. Start VPS-1 Testnet Stratum

```bash
IRIUM_STRATUM_POAWX=1 IRIUM_RPC_BASE=http://127.0.0.1:39511 IRIUM_RPC_TOKEN=<same-token> IRIUM_STRATUM_PORT=39512 IRIUM_POW_LIMIT_HEX=7fffff0000000000000000000000000000000000000000000000000000000000   /opt/irium-pool/irium-stratum/target/release/irium-stratum   >> /home/irium/irium-poawx-testnet/stratum.log 2>&1 &
echo $! > /tmp/testnet-stratum.pid
```

Verify:
```bash
nc -z -w3 127.0.0.1 39512 && echo stratum_ok || echo stratum_not_ready
```

## 6. Start VPS-2 Testnet iriumd

### Option A: Direct P2P (requires cloud firewall rule on port 39510)

On VPS-2:
```bash
cd /home/irium/irium  # required cwd
IRIUM_NETWORK=devnet IRIUM_POAWX_MODE=active IRIUM_DATA_DIR=/home/irium/irium-poawx-testnet-vps2 IRIUM_BOOTSTRAP_DIR=/home/irium/irium-poawx-testnet-vps2 IRIUM_RPC_TOKEN=<same-token> IRIUM_P2P_BIND=0.0.0.0:39610 IRIUM_NODE_HOST=127.0.0.1 IRIUM_NODE_PORT=39611 IRIUM_ADDNODE=207.244.247.86:39510 IRIUM_P2P_SEED_PORT=39510 IRIUM_DEV_EASY_BITS_TEMPLATE=1   /tmp/iriumd-poawx-testnet   >> /home/irium/irium-poawx-testnet-vps2/iriumd.log 2>&1 &
echo $! > /tmp/testnet-iriumd-vps2.pid
```

### Option B: SSH Tunnel Workaround (current — firewall blocked)

On VPS-1, set up forward tunnel to VPS-2:
```bash
ssh -f -N -L 127.0.0.2:39510:127.0.0.1:39510   -o StrictHostKeyChecking=no -o BatchMode=yes   -o ServerAliveInterval=30 -o ServerAliveCountMax=60   irium@207.244.247.86
echo $! > /tmp/vps2-tunnel.pid
```

On VPS-2, use `IRIUM_ADDNODE=127.0.0.2:39510` instead of the public IP.

**Why `127.0.0.2`:** iriumd filters `127.0.0.1` as self-address. `127.0.0.2` routes
to loopback but is not in the filtered set.

## 7. Verify Peer Propagation

On VPS-1:
```bash
curl -sf http://127.0.0.1:39511/status | python3 -c "import sys,json; d=json.load(sys.stdin); print('h=', d['height'], 'peers=', d.get('peer_count',0))"
```

On VPS-2:
```bash
curl -sf http://127.0.0.1:39611/status | python3 -c "import sys,json; d=json.load(sys.stdin); print('h=', d['height'], 'peers=', d.get('peer_count',0))"
```

Both heights should converge within ~30 seconds and peer_count should be >= 1.

## 8. Run PoAW-X Soak Harness

```bash
cd ~/irium
python3 scripts/poawx-phase11b-canonical-receipts-validation.py
```

Or for a targeted stratum test:
```bash
python3 scripts/poawx-stratum-long-soak-harness.py   127.0.0.1 39512   http://127.0.0.1:39511 <testnet-rpc-token>   --blocks 10
```

Expected per PoAW-X block: `irx1=True strat=True blk=True`

## 9. Monitor Logs

VPS-1 iriumd: `tail -f /home/irium/irium-poawx-testnet/iriumd.log`

Key lines to watch:
```
[poawx] assignment height=N ...
[poawx] receipt stored height=N ... pending_count=1
[submit_block_extended] accepted height=N tip=...
[poawx] block_extended accepted height=N cleared_receipts=1 remaining=0
```

## 10. Query Block with irx1

After a PoAW-X block is mined, verify irx1 commitment:
```bash
HEIGHT=<block-height>
curl -sf http://127.0.0.1:39511/rpc/block?height=$HEIGHT | python3 -c "
import sys, json
d = json.load(sys.stdin)
print('height=', d['height'])
print('irx1_root=', d.get('irx1_root'))
"
```

`irx1_root` will be a 64-char hex string if the block was submitted via
`submit_block_extended`, or `null` if it was a legacy `submit_block`.

## 11. Graceful Shutdown

```bash
kill $(cat /tmp/testnet-stratum.pid 2>/dev/null) 2>/dev/null || true
kill $(cat /tmp/testnet-iriumd.pid 2>/dev/null) 2>/dev/null || true

# On VPS-2:
ssh irium@157.173.116.134 "kill \$(cat /tmp/testnet-iriumd-vps2.pid 2>/dev/null) 2>/dev/null || true"
ssh irium@157.173.116.134 "kill \$(cat /tmp/vps2-tunnel.pid 2>/dev/null) 2>/dev/null || true"
```

## 12. Chain Reset

See `docs/poaw-x-testnet-reset-rollback-policy.md`.

## 13. Safety Checklist Before Every Run

- [ ] Mainnet iriumd alive: `ss -lntp sport = :38300 | grep iriumd`
- [ ] Mainnet stratum alive: `ss -lntp sport = :3333 | grep irium-stratum`
- [ ] No PoAW-X env in mainnet process: `cat /proc/<mainnet-pid>/environ | tr '\0' '\n' | grep POAWX`
- [ ] Testnet ports 39510-39512 free: `ss -lntp | grep -E ':3951[0-2]'`
- [ ] Testnet DATA_DIR is NOT `~/.irium/`: `echo $IRIUM_DATA_DIR`
- [ ] cwd is `~/irium` when starting iriumd
- [ ] RPC token not committed to git: `git grep IRIUM_RPC_TOKEN -- '*.env' || true`

## 14. Incident Response

| Symptom | Action |
|---------|--------|
| Testnet iriumd crash | Check log for PANIC; restart from Section 4; miners re-post receipts |
| Stratum crash | Restart from Section 5; miners auto-reconnect |
| VPS-2 loses sync | Restart iriumd on VPS-2 from Section 6 |
| Mainnet service affected | Stop ALL testnet processes; investigate before resuming |
| Block height stuck | Check peer_count; check tunnel alive; restart stratum/iriumd if needed |
| irx1_root null on expected PoAW-X block | Check receipt was posted before mining; check stratum log for SBE errors |

---

*Runbook updated for Phase 11-C. Apply cloud firewall rules (Section 3) before use
with external participants.*
