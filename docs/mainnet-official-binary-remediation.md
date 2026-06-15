# Mainnet Official Binary Remediation

**Date:** 2026-06-15
**Result: PASS** — mainnet restored to a clean official `origin/main` (v1.9.115) binary, served from a stable path isolated from dev/PoAW-X builds. One controlled restart performed.

---

## 1. Reason for Remediation

`iriumd.service` ran the binary at `/home/irium/irium/target/release/iriumd` — inside the dev repo. The Phase 14-E `cargo build --release` overwrote that path with the reconciled PoAW-X **branch** binary, and an external `systemctl` restart (Jun 14 23:30) put mainnet on that unofficial binary (PoAW-X gated off, MTP-compatible, but not a clean official build). See `docs/poaw-x-mainnet-binary-safety-triage.md`.

Remediation goal: run a clean official `origin/main` binary and permanently separate the production binary path from dev builds.

---

## 2. Binary Identity

| | Path | sha256 |
|---|---|---|
| Old running (branch) | `/home/irium/irium/target/release/iriumd` | `b121e5d12a33fc9e6eb456da3885f3ee7cdc07f3f5346d287f27a4e4368e7dda` |
| **New running (official)** | `/home/irium/mainnet/bin/iriumd-5d4604c` | `7c07ae2c30dd1c5ade6a23e99af4a132e4a4bbe8504c3b3ec4c342cfeb133cae` |

- Built from `origin/main` `5d4604c` (**v1.9.115**) in an isolated worktree `/home/irium/irium-main-release` with `CARGO_TARGET_DIR=/home/irium/irium-main-release-target` — no contact with the dev repo's `target/` or the live binary.
- Worktree confirmed official main: `src/poawx.rs` absent (clean, no PoAW-X code).
- Installed to versioned path + stable symlink: `iriumd-current` → `iriumd-5d4604c`.

---

## 3. systemd Change (drop-in, ExecStart only)

| | Value |
|---|---|
| Old ExecStart | `/home/irium/irium/target/release/iriumd` |
| New (effective) ExecStart | `/home/irium/mainnet/bin/iriumd-current` |
| Drop-in | `/etc/systemd/system/iriumd.service.d/override.conf` |

Drop-in content (overrides **only** ExecStart; all env, WorkingDirectory, User, data dirs, config preserved):
```
[Service]
ExecStart=
ExecStart=/home/irium/mainnet/bin/iriumd-current
```
`systemctl show -p ExecStart` resolved to a single entry: `path=/home/irium/mainnet/bin/iriumd-current`.

Privileged steps (install drop-in, `daemon-reload`, one `systemctl restart`) were executed by the operator via interactive `sudo` (password never entered into this session/transcript). Exactly one restart performed.

---

## 4. Restart & Verification

| Item | Before | After |
|---|---|---|
| PID | 3862880 | **4042499** (started Jun 15 07:19:01) |
| `/proc/<pid>/exe` | `…/irium/target/release/iriumd` | `/home/irium/mainnet/bin/iriumd-5d4604c` |
| Running hash | `b121e5d1…` | **`7c07ae2c…`** (== official) |
| Height | 31874 | **31875 and advancing** (best==local) |
| Peers | 9 | **9** (reconnected) |
| Service state | active | active |

**Clean startup:** blocks/state dirs intact; persist continuity tip=31874, `missing_in_window=0`, `contiguous_from_zero=31874`; header index rebuilt; no errors/panics.

**PoAW-X OFF:** no `IRIUM_POAWX_MODE` in service env; `configs/node.json` has no poawx/network key (defaults mainnet); 0 `poawx`/`irx1`/`assignment` log matches since restart.

**MTP intact:** official binary contains the consensus string "Block timestamp must be greater than median time past"; `src/chain.rs` in the official worktree has `MTP_ACTIVATION_HEIGHT`/`median_time_past` (4 refs). `MTP_ACTIVATION_HEIGHT=32_000` unchanged; mainnet height ~31875 (~125 blocks to activation). The official v1.9.115 binary enforces MTP identically to network expectation.

---

## 5. Build Isolation (collision fixed)

- Production service binary: `/home/irium/mainnet/bin/iriumd-current` → `iriumd-5d4604c` (a copy outside the dev repo).
- Dev/PoAW-X builds write to `/home/irium/irium/target/…` (and isolated worktree targets).
- The two paths are now **different** — future `cargo build` in `/home/irium/irium` can no longer overwrite the binary used by mainnet. Root cause resolved.

Recommended going forward: keep PoAW-X dev builds in the dev repo / separate worktree targets; deploy mainnet upgrades by building to a versioned file under `/home/irium/mainnet/bin/` and repointing the `iriumd-current` symlink, then one restart.

---

## 6. Rollback Path

If a regression appears: repoint `iriumd-current` to a prior versioned binary (or remove the drop-in `/etc/systemd/system/iriumd.service.d/override.conf` to revert ExecStart), `sudo systemctl daemon-reload`, `sudo systemctl restart iriumd.service`. The previous branch binary remains at `/home/irium/irium/target/release/iriumd` (`b121e5d1…`) for reference (not recommended for production).

---

## 7. Process Safety

- Exactly one controlled mainnet restart (authorized).
- No consensus parameters changed; PoAW-X not enabled; `IRIUM_POAWX_MODE` not set on mainnet.
- **Phase 14-F not started**; no devnet load.
- No push, no PR, no merge-to-main.
