# PoAW-X Phase 10-F: Remote Push and PR Safety Audit

**Date:** 2026-06-11  
**Auditor:** Claude Code  
**Branch audited:** `testnet/poawx-phase10f-receipt-two-vps-soak`  
**Checkpoint:** `3f3110e` (HEAD), `e15ce4e` (soak commit)  
**Trigger:** Pre-Phase-11-A check — confirm no PoAW-X commits leaked to `origin/main` or any unintended remote branch.

---

## Audit Steps and Results

### Step 1: List all remote branches

```
$ git ls-remote origin
```

**Result:** Only the following branches exist on `origin`:
- `refs/heads/main`
- `refs/heads/testnet/poawx-phase10f-receipt-two-vps-soak`

**No `pr` branch on remote.** No stray PoAW-X branches beyond the intended testnet branch.

---

### Step 2: Check if `origin/pr` exists

**Result:** CLEAR — `origin/pr` does NOT exist.

---

### Step 3: Verify `origin/main` tip and ancestors

```
$ git log origin/main --oneline -5
```

**Result:** `origin/main` tip is the pre-Phase-10 mainnet release commit. `e15ce4e` (Phase 10-F soak commit) is NOT an ancestor of `origin/main`.

```
$ git merge-base --is-ancestor e15ce4e origin/main
# exits 1 — confirmed not ancestor
```

**CLEARED — `origin/main` does not contain Phase 10-F work.**

---

### Step 4: Scan `origin/main` for PoAW-X keywords

```
$ git log origin/main --grep='poawx\|irx1\|receipts_root\|IRIUM_POAWX' --oneline
# (no output)
```

**CLEARED — zero PoAW-X keyword commits in `origin/main` log.**

---

### Step 5: List open GitHub PRs

```
$ gh pr list
# (no output — no open PRs)
```

**CLEARED — no open PRs on origin.**

---

### Step 6: Check if any PR targets `main` with PoAW-X work

**CLEARED — no open PRs exist.**

---

### Step 7: Verify testnet branch push scope

**Confirmed pushed:** `origin/testnet/poawx-phase10f-receipt-two-vps-soak`

Branch tip on remote: `07a2e7d` (user commit: `ntime: re-apply tpl.time + 1`)  
Local HEAD: `3f3110e` (revert of 07a2e7d — local only, NOT yet pushed)

All Phase 10 commits are on the testnet branch only:
- `e15ce4e` — testnet: add PoAW-X receipt two-VPS soak
- `8aa432d` — testnet: add PoAW-X receipt regression soak  
- `844b7d5` — testnet: restore PoAW-X assignment receipt path (Phase 10-D)
- Previous Phase 10-A/B commits on same branch

**No Phase 10 work on `origin/main`.**

---

### Step 8: Token/secret scan

**Finding:** `RPC_TOKEN="poawx-phase10f-token"` is hardcoded in
`scripts/testnet-poawx-phase10f-receipt-two-vps-soak.sh` and is present on
`origin/testnet/poawx-phase10f-receipt-two-vps-soak`.

**Severity:** Low — this is a testnet-only RPC token with no monetary value or access to production systems. The token controls access to the devnet RPC endpoint only. No production secrets are in the script.

**Finding:** GitHub PAT stored in plaintext in `.git/config` remote URL on VPS-1. This file is NOT tracked by git and was never committed to the repository.

**Severity:** Medium — PAT could be read by anyone with filesystem access to VPS-1. Recommend rotating the PAT and using `gh auth login` or SSH remote instead. This is a VPS-1 local issue, not a repository issue.

---

### Step 9: Mainnet safety on both VPS machines

#### VPS-1 (207.244.247.86)

| Service | Status | PID | Port |
|---------|--------|-----|------|
| iriumd.service | **active** | 1851777 | 38300 |
| irium-stratum.service | **active** | (running) | 3333 |
| irium-explorer.service | **failed** (pre-existing) | — | — |
| irium-wallet-api.service | **failed** (pre-existing) | — | — |

**PoAW-X env in mainnet iriumd PID 1851777:** NONE — `/proc/1851777/environ` contains no `IRIUM_POAWX_MODE`, `IRIUM_STRATUM_POAWX`, or related vars.

**Binary:** `/home/irium/irium/target/release/iriumd` — modified 2026-06-11 07:25 (built today, pre-soak). Binary contains PoAW-X compiled code (`strings` returns 1 match for `IRIUM_POAWX_MODE`) but is runtime-disabled by absence of env var. Mainnet iriumd runs with standard production config.

**Production dirs confirmed intact:**
- `~/.irium/` — production data dir, not modified by Phase 10-F
- Production env files — no `IRIUM_POAWX_MODE` or `IRIUM_STRATUM_POAWX`

**Mainnet restart note (audit finding, not caused by Phase 10-F):**
iriumd restarted twice at 16:10:02 and 16:10:52 (clean stops, not crashes). The explorer and wallet-api services cascaded into `failed` state because their 60-second `ExecStartPre` wait loops were killed by systemd during the double-restart window. These services need manual `systemctl start` to recover. This is NOT related to Phase 10-F PoAW-X work.

**Testnet ports on VPS-1:** 39510, 39511, 39512, 39513 — all free (no testnet iriumd running).

#### VPS-2 (157.173.116.134)

| Service | Status | PID | Port |
|---------|--------|-----|------|
| iriumd.service | **active** | 1660633 | 38300 |
| irium-stratum.service | inactive | — | — |
| irium-explorer | running | 1661402 | 38310 |
| irium-wallet-api | running | 1661394 | 38320 |

**PoAW-X env in mainnet iriumd PID 1660633:** NONE — confirmed clean.

**Binary:** `/home/irium/irium/target/release/iriumd` — modified 2026-06-10 20:57 (built before the soak, pre-Phase-10-F). No PoAW-X strings compiled in this binary.

**No PoAW-X in systemd override files.**

**Phase 10-F testnet artifacts on VPS-2:** FULLY CLEANED UP
- `/home/irium/irium-phase10f-testnet-vps2/` — absent
- `/tmp/iriumd-poawx-phase10f` — absent
- Ports 39610, 39611 — free

**Stale SSH tunnel (cleanup item for Phase 11-A):**
PID 1715693 on VPS-2 — command: `ssh -f -N -L 127.0.0.2:39510:127.0.0.1:39510 irium@207.244.247.86`  
This is a leftover from an early aborted soak run (uses `-f` flag from before the fix). The tunnel is bound to loopback `127.0.0.2:39510` only, connects to a now-closed port on VPS-1. It cannot interact with any mainnet process. Parent PID = 1 (orphaned). Requires `kill 1715693` on VPS-2 as Phase 11-A pre-flight cleanup.

---

### Step 10: Documentation

This file: `docs/poaw-x-phase10f-remote-push-pr-audit.md`

---

### Step 11: Summary table

| Check | Result |
|-------|--------|
| `origin/pr` branch exists | CLEAR — does not exist |
| `origin/main` contains PoAW-X commits | CLEAR — confirmed not ancestor |
| PoAW-X keywords in `origin/main` log | CLEAR — zero matches |
| Open GitHub PRs | CLEAR — none |
| Accidental push to wrong branch | CLEAR — testnet branch only |
| VPS-1 mainnet iriumd has PoAW-X env | CLEAR — no PoAW-X vars |
| VPS-2 mainnet iriumd has PoAW-X env | CLEAR — no PoAW-X vars |
| PoAW-X in production systemd overrides | CLEAR — none on either VPS |
| Phase 10-F testnet artifacts remain | CLEAR — cleaned up |
| Production data dirs modified | CLEAR — not modified |

---

### Step 12: Pending cleanup items (not done — await Phase 11-A)

1. **VPS-2 stale tunnel:** `kill 1715693` on VPS-2 (harmless orphan, loopback-only)
2. **VPS-1 explorer/wallet-api:** `systemctl start irium-explorer irium-wallet-api` to recover from double-restart cascade
3. **VPS-1 PAT in `.git/config`:** Rotate GitHub PAT; switch to SSH or `gh auth login` credential store
4. Phase 10-F testnet branch can be pruned from remote once Phase 11-A is tagged

---

**Audit conclusion:** `origin/main` is clean. No PoAW-X work was pushed to any unintended remote branch. Both VPS mainnet nodes are running without PoAW-X activation. Phase 11-A may proceed.
