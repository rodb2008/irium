# PoAW-X Phase 10-F: Remote Branch Cleanup

**Date:** 2026-06-11  
**Branch:** `testnet/poawx-phase10f-receipt-two-vps-soak`  
**Action:** Delete accidental remote testnet branch from GitHub  
**Preceded by:** `docs/poaw-x-phase10f-remote-push-pr-audit.md` (a9b489e)

---

## Summary

The testnet branch `testnet/poawx-phase10f-receipt-two-vps-soak` was present on
`origin` (GitHub). It was intentionally deleted from remote while preserving
all commits locally on VPS-1. `origin/main` was not affected. No PR existed.

---

## Step 1 — Local Preservation (pre-deletion)

| Item | Value |
|------|-------|
| Local branch | `testnet/poawx-phase10f-receipt-two-vps-soak` |
| Local HEAD | `a9b489e` (docs: audit PoAW-X remote push and PR safety) |
| `e15ce4e` in local branch | YES |
| `a9b489e` in local branch | YES |
| Working tree | clean |

Both target commits preserved locally before any remote operation.

---

## Step 2 — Remote State (pre-deletion)

| Item | Value |
|------|-------|
| Remote branch existed | YES — `origin/testnet/poawx-phase10f-receipt-two-vps-soak` |
| Remote tip | `3f3110e` (Revert ntime: re-apply tpl.time + 1) |
| `e15ce4e` on remote | YES (in testnet branch) |
| `a9b489e` on remote | NO (local-only commit) |
| Open PRs | NONE |
| `origin/main` tip | `f5252d2` (Merge testing-codes-before-merging into main v1.9.106) |
| PoAW-X on `origin/main` | NONE — confirmed clean |

---

## Step 3 — Deletion

```
git push origin --delete testnet/poawx-phase10f-receipt-two-vps-soak
# To https://github.com/iriumlabs/irium.git
#  - [deleted]         testnet/poawx-phase10f-receipt-two-vps-soak
```

---

## Step 4 — Post-Deletion Verification

```
git fetch --all --prune
git branch -r | grep -E 'poawx|phase10f|testnet'
# (no output — cleared)

git ls-remote --heads origin | grep 'testnet/poawx-phase10f-receipt-two-vps-soak'
# (no output — branch absent on remote)

git branch -r --contains e15ce4e
# (no output — local only)

git branch -r --contains a9b489e
# (no output — local only)
```

| Check | Result |
|-------|--------|
| Remote testnet branch deleted | CONFIRMED |
| `e15ce4e` remote refs | NONE — local only |
| `a9b489e` remote refs | NONE — local only |
| `origin/main` unchanged | CONFIRMED — tip still `f5252d2` |
| Local branch `testnet/poawx-phase10f-receipt-two-vps-soak` | INTACT |
| Local HEAD | `a9b489e` — unchanged |
| Working tree | clean |

---

## Step 5 — PAT Removal from Local Git Config

**Finding:** The `.git/config` remote URL on VPS-1 contained a GitHub Personal
Access Token in plaintext:
```
url = https://[REDACTED_TOKEN]@github.com/iriumlabs/irium.git
```

**Action taken:** PAT removed from remote URL.
```
git remote set-url origin https://github.com/iriumlabs/irium.git
```

**Result:**
```
origin  https://github.com/iriumlabs/irium.git (fetch)
origin  https://github.com/iriumlabs/irium.git (push)
```

No token remains in `.git/config`.

---

## Step 6 — PAT Rotation Recommendation

The PAT was stored in plaintext in `.git/config` on VPS-1. Although the file
was not tracked by git and was never committed to the repository, the token was
visible to anyone with filesystem access to VPS-1.

**Recommended actions:**

1. Go to GitHub → Settings → Developer settings → Personal access tokens
2. Revoke/rotate the PAT that was embedded in the remote URL
3. For future authenticated git operations on VPS-1, use one of:
   - SSH remote (`git@github.com:iriumlabs/irium.git`)
   - `gh auth login` credential store
   - Git credential helper (`git credential-store` or `git credential-cache`)
   - Never embed tokens in remote URLs directly

**This rotation was NOT performed automatically** — requires manual action in
GitHub settings.

---

## Step 7 — Mainnet Safety Confirmed

### VPS-1 (207.244.247.86)

| Process | PID | Port | Status |
|---------|-----|------|--------|
| iriumd (mainnet) | 1851777 | 38300 | running |
| irium-stratum (mainnet) | 1851781 | 3333 | running |
| Testnet ports 39510/39511/39512 | — | — | free |

### VPS-2 (157.173.116.134)

| Process | PID | Port | Status |
|---------|-----|------|--------|
| iriumd (mainnet) | 1660633 | 38300 | running |
| irium-explorer | 1661402 | 38310 | running |
| irium-wallet-api | 1661394 | 38320 | running |

### Stale SSH Tunnel (VPS-2)

**PID 1715693** — command confirmed: `ssh -f -N -L 127.0.0.2:39510:127.0.0.1:39510 irium@207.244.247.86`

This was a leftover from an early Phase 10-F soak run that used the `-f` flag
(the bug that caused wrong PID tracking in cleanup). The tunnel was bound to
`127.0.0.2` loopback only and had no interaction with any mainnet process.

**Action:** `kill 1715693` — confirmed killed.

After kill: VPS-2 mainnet services all running on expected ports. Port 39510 no
longer listening on VPS-2.

### Phase 10-F Testnet Artifacts

- VPS-1 testnet iriumd (`~/irium-poawx-phase10f/`): cleaned up by soak script
- VPS-2 testnet data dir (`~/irium-phase10f-testnet-vps2/`): absent
- VPS-2 testnet binary (`/tmp/iriumd-poawx-phase10f`): absent
- VPS-2 stale tunnel: killed

---

## Final State

| Item | State |
|------|-------|
| Local branch | EXISTS — `testnet/poawx-phase10f-receipt-two-vps-soak` |
| Local HEAD | `a9b489e` |
| `e15ce4e` preserved | YES (local only) |
| `a9b489e` preserved | YES (local only) |
| Remote testnet branch | DELETED |
| `origin/main` PoAW-X commits | NONE |
| Open PRs | NONE |
| PAT in `.git/config` | REMOVED |
| PAT rotation | PENDING — manual action required |
| VPS-1 mainnet | RUNNING (iriumd + stratum) |
| VPS-2 mainnet | RUNNING (iriumd + explorer + wallet-api) |
| Stale tunnel | KILLED |
