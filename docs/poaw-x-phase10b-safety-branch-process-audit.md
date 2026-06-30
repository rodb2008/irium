# PoAW-X Phase 10-B Safety Audit — Branch and Process Audit

**Date:** 2026-06-11  
**Auditor:** Claude Code (Phase 10-B-Safety checkpoint)  
**Branch audited:** `testnet/poawx-phase10b-stratum-tcp-miner`  
**Commit audited:** `ef754f0` — testnet: validate PoAW-X stratum TCP miner path

---

## 1. Branch Safety Result

### Was ef754f0 on main?
**YES — VIOLATION confirmed and remediated.**

After Phase 10-B, commit `ef754f0` was on local `main` branch (HEAD -> main).  
Local `main` was 1 commit ahead of `origin/main`.

### Was ef754f0 pushed to any remote?
**NO.** `origin/main` was at `fe032c1` (tag: v1.9.103). Nothing was pushed.

### Remediation performed

1. Worktree `/home/irium/irium-phase10b-build` was found using branch `testnet/poawx-phase10b-stratum-tcp-miner` at `cd8e3ee` (clean, no uncommitted changes, no running processes from its target/).  
   → Removed: `git worktree remove /home/irium/irium-phase10b-build`

2. Branch `testnet/poawx-phase10b-stratum-tcp-miner` force-updated from `cd8e3ee` to `ef754f0`:  
   → `git branch -f testnet/poawx-phase10b-stratum-tcp-miner ef754f0`

3. Verified `ef754f0` on testnet branch before resetting main:  
   → `git branch --contains ef754f0` returned `testnet/poawx-phase10b-stratum-tcp-miner`

4. Local `main` reset to `origin/main`:  
   → `git reset --hard origin/main` → HEAD is now `fe032c1`

### Post-remediation branch state

| Item | Value |
|------|-------|
| `main` HEAD | `fe032c1` (tag: v1.9.103, = origin/main) |
| `origin/main` HEAD | `fe032c1` (unchanged, not pushed) |
| `testnet/poawx-phase10b-stratum-tcp-miner` HEAD | `ef754f0` |
| `git branch --contains ef754f0` | `testnet/poawx-phase10b-stratum-tcp-miner` only |
| `git status` on main | "up to date with origin/main, nothing to commit" |
| Anything pushed? | **NO** |

---

## 2. Process Audit — VPS-1 (irium-vps, 207.244.247.86)

### The 3 stale shells identified and stopped

| PID | Command | Classification | Action |
|-----|---------|----------------|--------|
| 1554872 | `bash -c until [ -f /tmp/phase10b-test/logs/stratum.log ]; do sleep 1; done; sleep 8; tail -30 ...` | Phase 10-B stale wait loop. `/tmp/phase10b-test` already cleaned up; loop ran forever. | `kill 1554872` — confirmed gone |
| 1445290 | `tmux new-session -d -s phase10a bash .../testnet-poawx-phase10a-two-vps-long-soak.sh` | Phase 10-A soak script — completed (PASS=58 FAIL=1), session idle at shell prompt | `tmux kill-session -t phase10a` — confirmed gone |
| 1445291 | `bash` (inside tmux phase10a) | Idle bash inside phase10a tmux | Killed by tmux kill-session |

All 3 confirmed gone. No tmux sessions remain.

---

## 3. Mainnet Safety — VPS-1 (irium-vps)

| PID | Process | Uptime at audit | Port |
|-----|---------|----------------|------|
| 1556521 | `iriumd` | 16377s (~4.5h) | 8080 (local), 38300, 38291 |
| 1556525 | `irium-stratum` | 16377s | 3333 |
| 1556526 | `irium-stratum` | 16377s | — |
| 1556527 | `irium-stratum` | 16377s | — |
| 1556528 | `irium-stratum` | 16377s | — |
| 1556873 | `irium-explorer` | 16362s | — |
| 1558068 | `irium-wallet-api` | 16223s | — |

All mainnet PIDs alive and unmodified. Ports 8080, 38300, 38291, 3333 all bound correctly.

**No mainnet services were stopped, restarted, reloaded, or modified.**  
**No production configs, env files, wallets, or data dirs were modified.**  
**No PoAW-X env vars added to any production config.**

---

## 4. Mainnet Safety — VPS-2 (irium-eu, 157.173.116.134)

| PID | Process | Uptime at audit | Port |
|-----|---------|----------------|------|
| 1660633 | `iriumd` | 16201s | 38300, 38291, 8080 (local) |
| 1661394 | `irium-wallet-api` | 16029s | — |
| 1661402 | `irium-explorer` | 16029s | — |

No testnet processes found on VPS-2. No cleanup required.  
**No VPS-2 services were touched.**

---

## 5. Artifact and Secret Audit

Commit `ef754f0` adds exactly 2 files:
- `scripts/poawx-stratum-tcp-miner-harness.py` — Python TCP stratum miner harness (testnet only)
- `scripts/testnet-poawx-phase10b-stratum-tcp-miner.sh` — Phase 10-B test shell script

**No secrets, RPC tokens, wallets, private keys, live env files, or VPS-specific private data were committed.**  
`git diff --check` is clean.  
`git status` on testnet branch is clean.

**Note:** `docs/poaw-x-prototype-plan.md` does not exist on branch `testnet/poawx-phase10b-stratum-tcp-miner`. This branch diverges from the old testnet lineage and was not rebased on main. The prototype plan lives on main only. No update required for this audit.

---

## 6. Tests Run During Audit

- `git branch --contains ef754f0` — passed (testnet branch only)
- `git status` on main — clean, up to date with origin/main
- `git status` on testnet branch — clean
- Mainnet PID liveness checks — all PIDs alive
- Mainnet port binding checks — all ports bound

Cargo tests (poawx unit + stratum) deferred to Phase 10-C per plan.

---

## 7. Root Cause of Branch Violation

The Phase 10-B test script ran from the `main` branch worktree (`/home/irium/irium`). When the harness committed the test results/scripts, `git commit` ran against the `main` branch rather than checking out the testnet branch first. The commit was never pushed.

**Mitigation going forward:** Phase 10-C work must begin by checking out `testnet/poawx-phase10b-stratum-tcp-miner` or a new `testnet/poawx-phase10c-*` branch before running any test harness that commits.

---

## 8. Remaining Blockers Before Phase 10-C

- None from this safety audit.
- Phase 10-C may begin on a proper testnet branch.
- Recommend: `git checkout testnet/poawx-phase10b-stratum-tcp-miner` (or new `testnet/poawx-phase10c-*` branch from it) as first Phase 10-C step.
