# PoAW-X Testnet Chain Reset and Rollback Policy

**Status:** ADOPTED — applies to PoAW-X testnet only
**Date:** 2026-06-12
**Phase:** 11-C

---

## 1. Scope

This policy applies ONLY to the PoAW-X testnet chain (ports 39510-39512, devnet genesis).
It does NOT apply to mainnet.

---

## 2. Reset Criteria

A testnet chain reset may be triggered by:

| Trigger | Who decides |
|---------|-------------|
| Consensus parameter change (bits, epoch) | Operator |
| Persistent fork that cannot be reconciled | Operator |
| Block height corruption or panic | Operator |
| Phase boundary (e.g. Phase 11-C to 11-D) | Operator |
| Security issue in PoAW-X validation | Operator (immediate) |
| Tester request with justification | Operator at discretion |

The testnet chain carries no value. Resets are expected and documented.

---

## 3. Announcement Process

1. Operator posts reset notice to the testnet issue tracker.
2. Minimum 10-minute notice for planned resets (zero notice for security resets).
3. Notice includes: reset scope, ETA, reason, new seed address if changed.
4. Post-reset: confirm seed node is live before asking testers to reconnect.

---

## 4. Operator Reset Procedure

### Stop Testnet Services (VPS-1)

```bash
kill $(cat /tmp/testnet-stratum.pid 2>/dev/null) 2>/dev/null || true
kill $(cat /tmp/testnet-iriumd.pid 2>/dev/null) 2>/dev/null || true
fuser -k 39510/tcp 2>/dev/null || true
fuser -k 39511/tcp 2>/dev/null || true
fuser -k 39512/tcp 2>/dev/null || true
sleep 3
```

### Safety Check Before Wiping

```bash
# Confirm mainnet is alive and untouched
ss -lntp sport = :38300 | grep iriumd && echo mainnet-ok
ss -lntp sport = :3333  | grep irium-stratum && echo stratum-ok
# Confirm the dir to wipe is testnet-only
TESTNET_DIR=/home/irium/irium-poawx-testnet
echo "Wiping: $TESTNET_DIR"
# NEVER wipe ~/.irium or /home/irium/.irium
```

### Archive Logs

```bash
ARCHIVE=/home/irium/irium-poawx-testnet-archive-$(date +%Y%m%d-%H%M)
mkdir -p "$ARCHIVE"
cp /home/irium/irium-poawx-testnet/iriumd.log "$ARCHIVE/" 2>/dev/null || true
```

### Wipe Testnet Data

```bash
TESTNET_DIR=/home/irium/irium-poawx-testnet
rm -rf "$TESTNET_DIR/state" "$TESTNET_DIR/blocks"
mkdir -p "$TESTNET_DIR/state" "$TESTNET_DIR/blocks"
cp ~/irium/bootstrap/anchors.json "$TESTNET_DIR/"
cp -r ~/irium/bootstrap/trust "$TESTNET_DIR/"
```

### VPS-2 (if applicable)

```bash
ssh irium@157.173.116.134 "
  kill \$(cat /tmp/testnet-iriumd-vps2.pid 2>/dev/null) 2>/dev/null || true
  fuser -k 39610/tcp 2>/dev/null || true
  rm -rf /home/irium/irium-poawx-testnet-vps2/state
  rm -rf /home/irium/irium-poawx-testnet-vps2/blocks
"
```

Restart from the runbook (`docs/poaw-x-public-testnet-draft-runbook.md`).

---

## 5. Miner/Tester Reset Procedure

When the operator announces a reset:

1. Stop your miner or harness.
2. Wait for operator to confirm the seed node is live.
3. Restart your miner pointing at the testnet stratum (same address unless operator says otherwise).
4. Re-post a fresh receipt — receipts are not persisted across node restarts.
5. Do not reconnect to the old chain tip — it will be gone.

---

## 6. Logs to Preserve

Before wiping, copy:
- `iriumd.log` — chain operation log
- Stratum logs
- Any panic backtraces (`grep -i panic iriumd.log`)

Retention: 30 days minimum.

---

## 7. Rollback

Rollback (reverting to a previous block height without full chain wipe) is NOT supported
on the testnet. The testnet does not implement checkpoint-based rollback. If a fork must
be resolved, perform a full chain reset using Section 4.

---

## 8. Emergency Stop

Stop ALL testnet services immediately if:

| Condition | Action |
|-----------|--------|
| Testnet iriumd binding to mainnet port | Kill immediately, investigate |
| Testnet process using mainnet data dir | Kill immediately, investigate |
| Panic in testnet also appearing in mainnet log | Investigate mainnet first |
| Cloud firewall rule accidentally applied to mainnet port | Revert at cloud panel |

Emergency kill:
```bash
fuser -k 39510/tcp 2>/dev/null || true
fuser -k 39511/tcp 2>/dev/null || true
fuser -k 39512/tcp 2>/dev/null || true
```

---

## 9. Mainnet Isolation Guarantee

Testnet and mainnet are isolated by:

1. Separate data directories (`/home/irium/irium-poawx-testnet/` vs `~/.irium/`)
2. Separate ports (testnet: 395xx, mainnet: 382xx/3333)
3. Separate genesis (`IRIUM_NETWORK=devnet` on testnet, mainnet genesis elsewhere)
4. `IRIUM_NETWORK=devnet` — iriumd rejects mainnet blocks on devnet and vice versa
5. `IRIUM_POAWX_MODE=active` only on testnet — mainnet env has no POAWX vars

A testnet reset cannot affect the mainnet chain in any way.
