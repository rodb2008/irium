# PoAW-X Phase 26D — candidate-admission cache persistence (cold-resync hardening, Option 1)

**Status: implemented + proven by repo-local tests.** A restarted node now reloads its durable
candidate-admission snapshot and re-validates persisted blocks through the **UNCHANGED phase21e
gate**, fixing the restart / keep-storage cold-resync that stalled in Phase 26C. The brand-new
fresh-wipe case (a node that never saw the admissions) is intentionally **out of scope** here and
remains for a future Option 2 (serve historical admissions during sync). NOT production-ready /
mainnet-ready / audited.

Branch `testnet/poawx-phase20-blueprint-completion-local`. Implemented at HEAD `bfe16fd`.

## Verified root cause

The candidate-admission cache `GLOBAL_ADMISSION_CACHE`
(`src/poawx_admission.rs`) is a process-global, **in-memory `OnceLock` that is never persisted and
never reloaded**. It is populated only by live ingest (RPC `POST /poawx/candidate-admission` and P2P
gossip of *fresh* admissions).

The **required phase21e gate** inside `connect_block` →
`validate_block_candidate_sets` (`src/chain.rs`) requires
`cs.serialize() == cache.admitted_candidate_set(net, height, seed).serialize()`.

So:
- **Incremental sync works** — each new block's admissions gossip live alongside the block, so the
  cache holds them when `connect_block` runs phase21e.
- **Cold restart / multi-block resync stalls** — on restart the cache is empty, so the persisted
  replay (`src/bin/iriumd.rs` `load_persisted_blocks`) calls `connect_block(h1)`, which fails
  phase21e (a non-quarantine error), defers the block ("missing ancestors"), and never reconnects;
  network-synced historical blocks fail the same gate. This is the Phase 26C cold-resync stall.

The blocker is **admission *availability*, not block-body download scheduling**. Verified by reading
the cache (ephemeral `OnceLock`, no disk I/O) and by the fact that the `phase26b_multiblock` test only
passes because it re-ingests admissions before every `connect_block`.

## Why phase21e is preserved (no validation weakened)

- `validate_block_candidate_sets` / phase21e logic is **byte-for-byte unchanged**; the
  `admitted_candidate_set` equality check still runs exactly as before.
- Persistence stores **only admissions that already passed `ingest_bytes` validation**, and reload
  re-runs the **same** `CandidateAdmissionV1` validation (network match + signature/digest/seed/
  true-VRF). Malformed / wrong-network / truncated / tampered records are rejected on reload — they
  cannot smuggle an unvalidated admission past phase21e.
- The reload deliberately skips only the live **gossip window** (an anti-spam freshness check), not
  any validity check — `admitted_candidate_set` has no window dependence, so this is sound.
- Mainnet PoAW-X stays hard-off independently of this path.

## What persistence stores

A length-prefixed snapshot of the **raw canonical wire bytes** of every cached
`CandidateAdmissionV1`, written atomically (temp file + rename) to
`<IRIUM_DATA_DIR>/candidate_admissions.dat` (the node's isolated data root — never `/tmp`, never a
default `.irium`). It lives in the **data root, not the state dir**, so it survives both a
same-storage restart and a "delete only state, keep blocks" resync. The file is bounded by the
(pruned) cache size and rewritten on each accepted admission.

## Files changed

- `src/storage.rs` — `candidate_admissions_file()` (path under the isolated data root).
- `src/poawx_admission.rs` — `NodeCandidateAdmissionCache` gains a `persist_path` field;
  `set_persist_path`, `persist_snapshot` (atomic, called on accept in `ingest_bytes`),
  `reload_persisted_bytes` (re-validating, no live-window), and `load_persisted` (startup loader).
- `src/bin/iriumd.rs` — `load_persisted_blocks` sets the persist path and reloads the snapshot
  **before** the persisted-block replay (and so makes every later ingest durable).
- `src/chain.rs` — **test only** (`phase26d_cold_replay_with_persisted_admissions`); no gate logic.

NOT changed: phase21d / phase21e / phase22a logic; `connect_block` validation; the puzzle / finality
/ dominance gates; `src/pow.rs`, LWMA, difficulty, target, block reward; mainnet behavior.

## Tests (repo-local, `cargo test --lib -- --test-threads=1`)

- `chain::phase26d_cold_replay_with_persisted_admissions` — **PASS**: builds a **6-block** all-gates
  chain (ingesting + persisting admissions); after clearing the in-memory cache, (a) `connect_block`
  is **rejected by phase21e** without the reload (gate intact), and (b) after `load_persisted()` a
  fresh chain **re-connects all 6 blocks** to the tip via the reloaded admitted set.
- `poawx_admission::phase26d_persist_reload_roundtrip` — **PASS**: ingest persists to disk; a fresh
  cache reloads the same admitted set.
- `poawx_admission::phase26d_reload_rejects_invalid_records` — **PASS**: wrong-network, corrupt,
  truncated, empty, and tampered records are rejected on reload.
- Regression: `phase26b_multiblock_epoch_seed_soak` and the full suite — **747 passed / 0 failed**
  (serialized; one pre-existing parallel-only flaky test runs clean serialized).
- Release build `--release --bin iriumd --bin poawx-live-proof-harness` — success.
- No default-storage usage in tests (snapshot path is a per-process file under `target/`).
- Mainnet PoAW-X remains hard-disabled (unchanged gate functions).

## Live validation (restart / keep-storage) — PASSED

Controlled Windows + VPS-1 + VPS-2 devnet run on this build (HEAD `de13a83`):

- Windows egress IP `122.162.151.91` (unchanged); existing source-restricted VPS-1 UFW rule for
  `41210` applied; **no firewall change**. RPC loopback-only on all three. Isolated Phase 26D dirs
  (`irium-p26d-vps1-node`, `irium-p26d-vps2-node`, `irium-poawx-phase26d\node`).
- **6 all-gates blocks mined + propagated to all three** (H1 Windows, H2 VPS-1, H3 Windows,
  H4 **VPS-2-originated**, H5 VPS-1, H6 Windows); all three matched height/tip/irx1 after each.
  Final: height 6, tip `1f7b10d9106047dabe909077855fa6cbcf71579e5b772a4b7f3ec0ac18991290`,
  irx1 `745c8a9d40043cc1b1d7d0d33ab05156be66de4b9ef7758ec7bb9612d5380526`.
- VPS-2's data root held `candidate_admissions.dat` (the durable snapshot, ~9.5 KB).
- **Restart with same isolated storage (the Phase 26C failure case):** stopped VPS-2 by exact PID,
  restarted (storage preserved). Startup logged **`reloaded 18 persisted candidate admissions for
  cold replay`** and **`Startup source-of-truth: … tip=6 hash=1f7b10d9…`** — the active chain rebuilt
  to **height 6 from disk** (the persisted-block replay now passes phase21e via the reloaded
  admitted set). In Phase 26C this exact restart left `local height=0` stuck. VPS-2's tip + irx1
  matched the hub and Windows exactly.
- **New block after resync:** mined H7 (Windows); VPS-2 received it incrementally — all three at
  height 7, tip `0d9076037e0fa6a84d896dda730a32af879a3d3a7bcad3a21c4962216f85bf14`,
  irx1 `20025b5b0dd6972c503eab81dc29b0d43372c1fe0830295f7872054668b22a05`.

## Cleanup proof

- All three Phase 26D nodes stopped by **exact pidfile PIDs** (no pkill/killall): Windows `40972`,
  VPS-1 `33729`, VPS-2 `2106268` — all STOPPED. All p26d ports closed
  (`41210/41411/41408`, `41420/41421/41418`, `41430/41431/41428`).
- Mainnet/prod alive and untouched: Windows `33752`, VPS-1 `219530`, VPS-2 `1851441`; VPS-1 prod
  pool alive.
- Default storage untouched (all predate this run): Windows `%USERPROFILE%\.irium` (2026-06-07),
  VPS-1 `~/.irium` (2026-06-21), VPS-2 `~/.irium` (2026-06-06). UFW unchanged. No real wallets
  touched. Artifacts preserved under `phase26d-artifacts-vps1/vps2` and the Windows
  `irium-poawx-phase26d\artifacts`.

## What remains unsolved

- **Fresh-wipe / brand-new node** that never received the admissions: still cannot validate
  historical blocks (it has no admissions to reload). Requires a future **Option 2** — serving /
  re-gossiping historical candidate admissions to a syncing peer (a larger P2P change), keeping
  phase21e unchanged.
- Independent audit; public testnet; governance / mainnet activation.
