# Mainnet Binary Safety Triage

**Date:** 2026-06-15
**Trigger:** Phase 14-F preflight found the mainnet `iriumd` PID had changed (3800352 → 3862880). Investigation revealed mainnet is running the reconciled PoAW-X **branch** binary, not a clean official `main` build. Phase 14-F was paused.

**Safety status: PASS (mainnet healthy, consensus-compatible, PoAW-X OFF) — but running an unofficial binary; controlled remediation recommended.**

---

## 1. Timeline

| Time (server) | Event |
|---|---|
| (pre-14E) | Mainnet running as PID **3800352** on the pre-merge binary |
| Phase 14-E build | I ran `cargo build --release`, overwriting `/home/irium/irium/target/release/iriumd` with the reconciled **v1.9.114 branch** binary (HEAD 47ee653) |
| Jun 14 23:28:09 | Operator `bisu40` committed `c934d6d` to `main` ("Add coinbase_tag field to rpc/blocks response") |
| Jun 14 23:30:24 | **External** graceful `systemctl` stop of `iriumd.service` (log: "Stopping… / persist queue drained on shutdown / Deactivated successfully"). **Not initiated by me; not an OOM/crash.** |
| Jun 14 23:30:25 | systemd started **PID 3862880** from `ExecStart=/home/irium/irium/target/release/iriumd` — i.e. **my branch binary**, because that path had been overwritten and no fresh `main` build was produced |
| Jun 14 23:30:50 | Clean resync: tip=31649, missing_in_window=0, contiguous_from_zero=31649 — healthy |

**Who/what restarted:** systemd journal shows an external, deliberate `systemctl` stop+start (graceful, "persist queue drained"). The author of the immediately-preceding `main` commit is `bisu40`; the restart appears to have been an operator action intended to deploy that work — but the on-disk service binary was the PoAW-X branch build, not a fresh `main` build. I issued no `systemctl`/restart/build that caused this.

---

## 2. Current State (low-load inspection)

| Item | Value |
|---|---|
| Mainnet PID (before → after) | 3800352 → **3862880** (started Jun 14 23:30:25) |
| Mainnet height | **31798**, advancing (local==best) |
| Peers / seedlist / mempool | 8 / 16 / 1 |
| Sync status | In sync, 0 missing in persist window |
| `iriumd.service` ExecStart | `/home/irium/irium/target/release/iriumd` |
| `/proc/3862880/exe` | → `/home/irium/irium/target/release/iriumd` (**not** `(deleted)`) |
| **Running binary sha256** | `b121e5d12a33fc9e6eb456da3885f3ee7cdc07f3f5346d287f27a4e4368e7dda` |
| **On-disk binary sha256** | `b121e5d12a33fc9e6eb456da3885f3ee7cdc07f3f5346d287f27a4e4368e7dda` (**IDENTICAL**) |
| Repo branch / HEAD | `testnet/poawx-phase12-completion-rc-hardening` / `47ee653` |
| origin/main | `5d4604c` (advanced from `1cbf190` since the 14-E merge) |

Running binary == on-disk branch binary (identical hash). Mainnet is running exactly the reconciled branch build at HEAD 47ee653; no post-restart divergence.

---

## 3. What mainnet is missing vs official main

`origin/main` `5d4604c` = `1cbf190` (merged into branch) **+ `c934d6d`** "Add coinbase_tag field to rpc/blocks response".

- `c934d6d` touches `Cargo.toml` (version) and `src/bin/iriumd.rs`: adds `extract_coinbase_tag()` helper and a `coinbase_tag` field to the **rpc/blocks JSON response**.
- **Non-consensus**: no change to validation, `connect_block`, PoW, MTP, or any consensus path. Pure RPC response enrichment for the explorer.
- Impact of mainnet missing it: the rpc/blocks response lacks the `coinbase_tag` field. Cosmetic; no consensus or stability effect.

---

## 4. PoAW-X OFF Confirmation

Three independent signals confirm PoAW-X is disabled on mainnet:
1. `iriumd.service` env contains **no `IRIUM_POAWX_MODE`** (PoAW-X code requires `IRIUM_POAWX_MODE=active`).
2. `configs/node.json` has **no** `poawx`/`network` key → defaults to **mainnet** network kind (mainnet activation guards reject PoAW-X regardless).
3. **No** `poawx`/`irx1`/`assignment` activity in the mainnet journal.

The branch binary compiles PoAW-X in, but it is dormant on mainnet. The five mainnet activation guards (network-kind + mode + activation height) remain in force.

---

## 5. MTP Compatibility

- `MTP_ACTIVATION_HEIGHT = 32_000`. Mainnet height **31798** → **~202 blocks** to activation (~6–7h at 120s blocks).
- The branch binary's MTP rule (`median_time_past()` + timestamp validation) came directly from merging `origin/main` `1cbf190` (v1.9.114). It is **byte-for-byte the same consensus logic** as official v1.9.114.
- Therefore the running branch binary will enforce MTP at height 32000 **identically** to the official binary. **Consensus-compatible.**

---

## 6. Consensus Compatibility Verdict

The running branch binary is **consensus-compatible with official v1.9.114**:
- It fully contains `1cbf190` (v1.9.114) consensus code (MTP included).
- The only main commit it lacks (`c934d6d`) is a non-consensus RPC field.
- PoAW-X is gated off, so its presence has no mainnet consensus effect.

---

## 7. Risk Assessment

**Overall risk: LOW–MODERATE.**

- ✅ Mainnet healthy, in sync, consensus-compatible, PoAW-X off.
- ⚠️ Mainnet is running an **unofficial** binary (a testnet branch build that carries dormant PoAW-X code), not a clean release of official `main`.
- ⚠️ **Binary-path collision:** `iriumd.service` runs the binary at `target/release/iriumd` inside the dev repo. Any future `cargo build` in this repo overwrites the live production binary, and the next restart will pick up whatever is there. This is the root cause and will recur.
- ⚠️ MTP activation (~6–7h away) is handled identically by this binary, so no added consensus risk — but production should be on the intended official build before/▸around such an event.

---

## 8. Recommended Remediation (NOT performed — report only)

- **A. Short term (now):** Leave the current binary running. It is healthy, consensus-compatible, and PoAW-X is gated off. **Do not restart, do not run `cargo build`/tests in this repo, and do not start any devnet load** until the official binary is restored — any of those could overwrite the service binary or trigger another pickup.
- **B. Controlled maintenance window:** Build/deploy a clean official `origin/main` (`5d4604c`) binary and restart mainnet **once**, deliberately, with verification of the deployed hash.
- **C. Permanently separate the production binary path** from dev builds — copy the release binary to a stable location (e.g. `/home/irium/bin/iriumd` or `/usr/local/bin/iriumd`) and point `iriumd.service` `ExecStart` there, so `cargo build` in the repo can never overwrite the live binary.
- **D. Move PoAW-X testing builds to a separate git worktree/path** so the repo's `target/` is never the source of the production binary.

Recommended sequence: **A now → B+C together in a maintenance window → D going forward.**

---

## 9. Process Safety Confirmation

- No restart, stop, or reconfigure of mainnet performed by me.
- No deploy performed.
- No `cargo build`/tests run during this triage (low-load inspection only).
- No devnet/testnet process running; **Phase 14-F not started**.
- No push, no PR, no merge-to-main.
- Repo tree clean; HEAD `47ee653`.
