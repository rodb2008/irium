# PoAW-X Devnet Cleanup — Mainnet Outage Incident & Permanent Safety Rule

**Severity:** High (production mainnet node stopped) — **Recovered, no data loss.**
**Host:** VPS-1 (seed node). irium-eu/VPS-2 unaffected throughout.
**Date:** 2026-06-15

---

## 1. What happened

During teardown of an *isolated PoAW-X devnet pilot* (devnet node + stratum on ports
39508/39510/39511/39512), a cleanup command used a **process-name substring match**
to kill pilot processes. The substring matched the **production mainnet binary**
`iriumd-current`, sending it SIGTERM. The mainnet node stopped. Because
`iriumd.service` is configured `Restart=on-failure` and the process exited
**cleanly (status 0)**, systemd treated it as a normal stop and did **not**
auto-restart it. The production pool services (which depend on this node via RPC)
were left without a backing node until manual restart.

## 2. Timeline (IST)

| Time | Event |
|------|-------|
| 22:09:54 | Mainnet node PID **4042499** received SIGTERM from the cleanup `pkill`; logged `persist queue drained on shutdown`; `iriumd.service: Deactivated successfully` (exit status 0). |
| 22:09:54–22:18 | Mainnet down. `Restart=on-failure` did not fire (clean exit). Pool services still running but node absent. |
| ~22:17 | Outage detected during post-cleanup mainnet verification (`systemctl is-active iriumd` → `inactive`, MainPID 0, PID 4042499 gone). |
| 22:18:36 | Operator ran recovery `sudo systemctl start iriumd`; new PID **219530** started. |
| 22:18:36–22:21 | Startup chain load + header-index relink (CPU-bound, ~3 min). |
| 22:21:57 | First post-restart heartbeat; node live, relinking from persisted tip. |
| ~22:22 | Node converged to tip **32294 → 32295**, `ahead=0`, peers 7–8. Fully recovered. |

## 3. Exact bad command

```bash
# RUN DURING DEVNET CLEANUP — DO NOT EVER REPEAT
pkill -f "iriumd" -u irium
```

(Issued as part of a broader cleanup line that also did `fuser -k` on devnet ports.
The `fuser -k <port>/tcp` part was safe; the `pkill -f "iriumd"` part was not.)

## 4. Why it hit production

- The production mainnet binary is `/home/irium/mainnet/bin/iriumd-current`
  (symlink → `iriumd-5d4604c`, official hash `7c07ae2c…`).
- `pkill -f "iriumd"` matches the **full command line** of every process containing
  the substring `iriumd` — including `iriumd-current` and `iriumd-5d4604c`.
- The earlier **binary-path isolation** remediation (separate prod binary path so dev
  *builds* cannot overwrite the prod binary) protects the **build/deploy** path only.
  It does **not** protect against a **runtime process-name match** — a `pkill`/`killall`
  on the substring still reaches the running production process.
- The devnet pilot node was a *different* binary at a *different* path, but shared the
  `iriumd` substring, so a name-based kill could not distinguish prod from pilot.

## 5. Recovery

```bash
# operator-run (sudo password-gated; agent cannot run sudo)
sudo systemctl start iriumd
```

## 6. Recovery result (verified)

- `systemctl is-active iriumd` → **active**; MainPID **219530**.
- Running exe `/proc/219530/exe` → `/home/irium/mainnet/bin/iriumd-5d4604c`,
  `sha256sum` first16 = **`7c07ae2c30dd1c5a`** (official, unchanged).
- Height resumed to persisted tip **32294**, advanced to **32295**;
  sync `local=32295 best_header=32295 ahead=0 peers=7`.
- Production pool services active: `irium-pool-api`, `irium-stratum`,
  `irium-stratum-443`, `irium-stratum-legacy`, `irium-stratum-solo`.
- irium-eu/VPS-2 mainnet untouched (MainPID **1851441**, hash `7c07ae2c…`).

## 7. Proof of no data loss

- Shutdown was clean: journal `[i] persist queue drained on shutdown` before exit
  (exit status 0 — not a crash/kill -9).
- Startup integrity: `persist continuity window: tip=32294 window_start=30295
  missing_in_window=0 contiguous_from_zero=32294 historical_missing_before_window=0`.
- No resync-from-genesis; node relinked persisted blocks and converged to the
  network tip. Brief startup display at linked tip 31902 (181 persisted-but-unlinked
  window headers) resolved within ~3 min to 32294/32295.

## 8. PERMANENT RULE — no broad process-name cleanup

**NEVER** use process-name / substring matching to stop pilot processes:

- ❌ `pkill -f "iriumd"` (or any `pkill -f` containing `iriumd`/`irium`)
- ❌ `killall iriumd*`
- ❌ `pkill -f irium`, `pkill iriumd`, `pgrep -f iriumd | xargs kill`
- ❌ any match on a bare/broad process name shared with production

## 9. ALLOWED cleanup methods only

1. **Exact pidfile** written at pilot startup:
   `kill "$(cat /tmp/pilot-node.pid)"` / `kill "$(cat /tmp/pilot-stratum.pid)"`
   (after confirming that PID is the pilot, not a recycled PID).
2. **Exact devnet port** (pilot ports are disjoint from mainnet):
   `fuser -k 39512/tcp 39511/tcp 39510/tcp 39508/tcp` — never a mainnet port.
3. **Exact isolated binary path** match if ever needed:
   `pkill -f "/home/irium/devnet-bin/iriumd-stdhdr"` (full unique path, never the
   bare name) — pidfile/port is still preferred.
4. **Process group** of the pilot launch (if started with `setsid`): kill the pilot
   PGID only.

## 10. Mandatory verification around every pilot test

Record and compare the **production MainPID + binary hash** immediately **before**
and **after** any pilot start/teardown:

```bash
systemctl show -p MainPID --value iriumd          # must be unchanged across the test
sha256sum /proc/$(systemctl show -p MainPID --value iriumd)/exe   # must stay 7c07ae2c…
```

If the production MainPID changes or the service is not `active` after teardown,
treat it as an incident and restore via `sudo systemctl start iriumd` before anything else.
