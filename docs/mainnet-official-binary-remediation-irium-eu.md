# Mainnet Official Binary Remediation — irium-eu (VPS-2)

**Date:** 2026-06-15
**Host:** irium-eu (157.173.116.134, `vmi2995746`)
**Result: PASS** — irium-eu mainnet moved to a clean official `origin/main` (v1.9.115) binary on a stable production path isolated from the dev repo. One controlled restart performed.

---

## 1. Reason

`iriumd.service` ran `/home/irium/irium/target/release/iriumd` — inside the dev repo. Any `cargo build` in that repo could overwrite the live mainnet binary on the next restart (the same class of vulnerability remediated on VPS-1). irium-eu was already running an official main build, but from the unsafe repo path. Remediation moves it to `/home/irium/mainnet/bin/iriumd-current` (outside the repo).

---

## 2. Binary Identity

| | Path | sha256 |
|---|---|---|
| Old running | `/home/irium/irium/target/release/iriumd` | `e6cbe44cc0eb23bfabef8e2b2da03351d15cc34678128433fa87569d87de9981` |
| **New running (official)** | `/home/irium/mainnet/bin/iriumd-5d4604c` | `7c07ae2c30dd1c5ade6a23e99af4a132e4a4bbe8504c3b3ec4c342cfeb133cae` |

- Built from `origin/main` `5d4604c` (**v1.9.115**) in an isolated worktree `/home/irium/irium-main-release` with `CARGO_TARGET_DIR=/home/irium/irium-main-release-target` — no contact with the live repo `target/` or the running binary.
- Worktree confirmed official main: `src/poawx.rs` absent.
- **Reproducible build:** the resulting hash `7c07ae2c…` is byte-identical to the official binary independently built on VPS-1 — confirming a genuine clean official `5d4604c` build.
- Installed to versioned path + stable symlink `iriumd-current` → `iriumd-5d4604c`.

---

## 3. systemd Change (drop-in, ExecStart only)

| | Value |
|---|---|
| Old ExecStart | `/home/irium/irium/target/release/iriumd` (no args) |
| New (effective) ExecStart | `/home/irium/mainnet/bin/iriumd-current` |
| Drop-in | `/etc/systemd/system/iriumd.service.d/override.conf` |

```
[Service]
ExecStart=
ExecStart=/home/irium/mainnet/bin/iriumd-current
```
`systemctl show -p ExecStart` resolved to a single entry: `path=/home/irium/mainnet/bin/iriumd-current`. All other unit settings (env, WorkingDirectory, User, data dirs) preserved.

Privileged steps (drop-in install, `daemon-reload`, one `systemctl restart`) run by the operator via interactive `sudo` over `ssh -t` (sudo requires a password on this host; password never entered into the session/transcript). Exactly one restart performed.

---

## 4. Restart & Verification

| Item | Before | After |
|---|---|---|
| PID | 1836431 | **1851441** (started Jun 15 06:17:30) |
| `/proc/<pid>/exe` | `…/irium/target/release/iriumd` | `/home/irium/mainnet/bin/iriumd-5d4604c` |
| Running hash | `e6cbe44c…` | **`7c07ae2c…`** (== official) |
| Height | 31938 (preflight) / 31948 (at restart) | **31948 → 31949 advancing** (best==local) |
| Peers | 12 | **11** (reconnected) |
| Service state | active | active |

**Clean startup:** blocks/state dirs intact; persist continuity tip=31948, `missing_in_window=0`, `contiguous_from_zero=31948`. A transient `unlinked_in_window=23` header-index warning appeared at startup; it resolved as the node synced with peers — confirmed by height advancing (31948→31949) and peers reconnecting (5→11). No missing-block or resync errors.

**PoAW-X OFF:** no `IRIUM_POAWX_MODE` in service env; 0 `poawx`/`irx1`/`assignment` log matches since restart.

**MTP intact:** new binary contains the consensus string "Block timestamp must be greater than median time past" (official v1.9.115). `MTP_ACTIVATION_HEIGHT=32_000` unchanged; mainnet ~31949 (≈50 blocks to activation), enforced identically to the network.

---

## 5. Build Isolation (collision fixed)

- Service binary: `/home/irium/mainnet/bin/iriumd-current` → `iriumd-5d4604c` (outside the dev repo).
- Dev/repo builds write to `/home/irium/irium/target/…` (and isolated worktree targets).
- Paths are now **different** — future `cargo build` in `/home/irium/irium` can no longer overwrite the irium-eu mainnet binary. Root cause resolved (matches VPS-1).

---

## 6. Rollback Path

Repoint `iriumd-current` to a prior versioned binary, or remove the drop-in `/etc/systemd/system/iriumd.service.d/override.conf` to revert ExecStart; then `sudo systemctl daemon-reload` + `sudo systemctl restart iriumd.service`. The previous binary remains at `/home/irium/irium/target/release/iriumd` (`e6cbe44c…`) for reference.

---

## 7. Process Safety

- Exactly one controlled irium-eu mainnet restart (authorized).
- VPS-1 mainnet not touched.
- No PoAW-X built on irium-eu (only official main); **no PoAW-X/devnet load** started on irium-eu.
- No consensus parameters changed; PoAW-X not enabled; `IRIUM_POAWX_MODE` not set.
- No push, no PR, no merge-to-main.

Both production seed-node hosts (VPS-1 and irium-eu) now run clean official v1.9.115 binaries from stable, dev-isolated paths.
