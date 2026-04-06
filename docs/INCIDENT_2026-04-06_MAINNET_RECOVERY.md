# Incident Report: Mainnet LWMA Recovery — 2026-04-06

## Summary

Following the LWMA activation at block 16462, the EU node and EU miner were running
pre-LWMA or misaligned binaries, causing the EU miner to loop on a bits-mismatch error
at block 16462 and fail to contribute hash rate. The VPS miner was healthy but degraded
by a rogue competing process accidentally spawned during diagnostics.
Both issues were resolved on 2026-04-06. The chain continued advancing without interruption.

---

## Original Symptoms

- EU miner logged a recurring error: **"Block bits mismatch"** at height 16462.
  It would fetch a template, compute a hash, attempt submission, get rejected, and repeat.
- EU miner was stuck: it had synced past the LWMA activation height but its bits
  calculation disagreed with what the live chain expected at height 16462.
- VPS miner was hashing but hash rate was degraded (~8.7 MH/s instead of ~15 MH/s).
  A rogue process (PID 3316764) was consuming ~409% CPU alongside the real miner.

---

## Root Causes

### EU Node Binary (pre-LWMA)

The EU `iriumd` binary was built from commit `bb8d19b` in the original `irium/`
repository, which predates LWMA entirely.
- `src/activation.rs` in that binary had no LWMA constant at all.
- The node therefore served block templates with legacy difficulty bits above height 16462.
- The live chain had LWMA bits at 16462; the template had legacy bits — miner computed
  a hash against the wrong target.

### EU Miner Binary (activation height off by one)

The EU `irium-miner` binary had been built from the deleted `irium-discovery-clean`
directory. That build embedded `CARGO_MANIFEST_DIR=/home/irium/irium-discovery-clean`
(now gone) and used `MAINNET_LWMA_ACTIVATION_HEIGHT = Some(16_463)` — one block too late.
- At height 16462: `16462 < 16463` → miner used legacy bits.
- Chain's block 16462 has LWMA bits → submission rejected.
- Additionally, the stale binary's `repo_root()` fell back to a non-existent genesis
  path and would panic on startup if the node restarted.

### Rogue VPS Process

During an earlier diagnostic session, the command `irium-miner --version` was run as
a one-off SSH check. The binary at commit `5f0c732` does not implement `--version`
as a quick-exit flag — it silently ignored the flag and began mining.
This process (PID 3316764) ran for ~94 minutes undetected, competing with the real miner
(7 threads) on an 8-core VPS, halving effective hash rate.

---

## What Was Fixed on EU

1. **EU node binary replaced** with a fresh build from `irium-lwma-upgrade` (commit `5f0c732`).
   - Activation height: `MAINNET_LWMA_ACTIVATION_HEIGHT = Some(16_462)`.
   - `CARGO_MANIFEST_DIR` correctly embedded as `/home/irium/irium-lwma-upgrade`.
   - Binary replaced atomically: `cp new tmp && mv tmp target` to avoid "Text file busy".
   - Old iriumd process killed with `kill -9`; `Restart=on-failure` picked up new binary.

2. **EU miner binary replaced** with a fresh build from `irium-lwma-upgrade` (commit `5f0c732`).
   - Activation height aligned to `16_462` — same as node and live chain.
   - Same atomic replacement + kill pattern.
   - Miner synced through historical blocks 16462–18268 and entered active mining state.

3. **Verified** EU miner transitions cleanly through block 16462 with no bits-mismatch error.

### EU Service Override (effective config)
`/etc/systemd/system/irium-miner.service.d/zzzz-final.conf`:
```
ExecStart=/home/irium/irium-lwma-upgrade/target/release/irium-miner --threads 3
```

---

## What Was Fixed on VPS

1. **Rogue process killed**: `kill 3316764` (the accidental `irium-miner --version` process).
   - No service restarts, no config changes.
   - VPS miner hash rate recovered from ~8.7 MH/s → ~16.6 MH/s within 2 minutes.
   - VPS load average dropped from 14.12 → 7.38 (1-min).

---

## Current Healthy State (as of 17:49 UTC, 2026-04-06)

| Item | Status |
|------|--------|
| EU node (iriumd) | Running `irium-lwma-upgrade` build, activation 16462 |
| EU miner | Running `irium-lwma-upgrade` build, ~2.72 MH/s, 3 threads |
| VPS node (iriumd) | Running, height 18273+, 20 peers |
| VPS miner | Running `irium-lwma-upgrade` build, ~16.6 MH/s, 7 threads |
| Chain tip alignment | Both nodes on same tip, template updates arrive simultaneously |
| Block cadence | Blocks 18271–18273 arrived at normal intervals during monitoring |
| Errors | None on either miner or node |

---

## Non-Urgent Maintenance Item

**VPS node binary is stale.**

The running `iriumd` process on the VPS holds a file descriptor to an inode that has
been replaced on disk (shows as `(deleted)` in `/proc/.../exe`):
- In-memory binary SHA256: `6116ddd9...`
- On-disk binary SHA256:   `406ec224...`

The node is functioning correctly on the current binary (20 peers, synced, no errors).
This is not an emergency. A controlled restart is planned — see
`docs/MAINTENANCE_VPS_NODE_STALE_BINARY.md`.
