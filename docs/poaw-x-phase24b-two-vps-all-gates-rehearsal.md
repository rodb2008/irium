# PoAW-X Phase 24B — two-VPS all-gates rehearsal (PAUSED — storage isolation incident)

**Status: PAUSED before completion. No validation is claimed from Phase 24B.** Local-only;
not pushed; remote branch absent; `main` untouched. This document records a storage-isolation
incident that occurred during setup, its full recovery, the root cause, and the required fix.
It does **not** replace an external audit or a public testnet, and makes **no** mainnet-ready
claim.

Per operator decision, Phase 24B was reduced to a single-VPS rehearsal (VPS-1 only, loopback);
during the very first node bring-up the incident below occurred and the operator chose to pause
and document only — no further live node launches were performed.

## 1. Phase 24B was paused before completion

The rehearsal was stopped during the first devnet node launch. No blocks were produced, no
observer validation was run, no restart/reload test was run. There is **no Phase 24B
pass/validation result.**

## 2. The incident

- The devnet test node was launched intending to isolate its storage under `/tmp`
  (`IRIUM_DATA_DIR=/tmp/irium-p24b-nodeA`, etc.).
- `storage::configured_dir()` **rejects any configured path that is not under `$HOME`** (after
  normalization it requires `starts_with($HOME)`); a `/tmp` path therefore returns `None`.
- On `None`, the process **silently fell back** to the default runtime root `~/.irium`, so the
  blocks directory resolved to `~/.irium/blocks`.
- `~/.irium/blocks` is the **mainnet node (PID 219530) live persisted block store** (tip
  ~35505, "Growth Era").
- Running under **devnet PoW rules**, the node treated the mainnet blocks as failing the
  declared target and **quarantined them** — moving block files into `orphaned_*`
  subdirectories. Across two boots today (one accidental boot — `iriumd` has no exiting
  `--help` and starts a node — plus node A), **22,886** mainnet block files were displaced.

## 3. Impact

- A **mainnet hard-rule was violated** ("do not touch mainnet / PID 219530 must remain
  untouched").
- The mainnet **process PID 219530 did NOT crash.**
- Mainnet **continued from memory and advanced height** normally throughout (35505 → 35506).
- Only the **persisted** block store was temporarily disrupted (block files moved aside); the
  running chain state was unaffected.

## 4. Recovery

- All **22,886** displaced block files were restored with `mv -n` (**no clobber** of any blocks
  mainnet had re-synced in the interim).
- Block count restored to **35,876** (mainnet advanced during the window, so the count exceeds
  the pre-incident value).
- The **5 orphan dirs created today were removed**; only the **two pre-existing Jun-13 orphan
  dirs remain**.
- **Mainnet node 219530 alive** (height 35506, Growth Era).
- **Prod pool workers alive** (4 release workers).
- **No test node running** (only mainnet 219530).
- **Stray test dirs removed** (`/tmp/irium-p24b-node*`, `/home/irium/irium-p24b-state-node*`,
  pidfiles, logs).
- **Repo clean at `bded2ad`** (no repository changes — the incident affected only `~/.irium`,
  not the git tree).

## 5. Root cause

- A **non-`$HOME` storage path was rejected** by `configured_dir()`.
- The process then **silently fell back to the default `~/.irium`** instead of failing closed
  — and `~/.irium` is the mainnet node's live data root on this host.

## 6. Future required fix

- **No test/devnet launch may proceed** unless `IRIUM_BLOCKS_DIR`, `IRIUM_STATE_DIR`, and
  `IRIUM_DATA_DIR` are **all explicit, `$HOME`-rooted, dedicated** paths.
- The launcher **must print/verify the isolated storage paths before continuing**.
- **If any path resolves to `~/.irium`, abort immediately.**
- **Code hardening (recommended):** invalid/rejected configured storage paths should **fail
  closed** (refuse to start) rather than silently falling back to `~/.irium`. A node launched
  with an explicit but unusable `IRIUM_*_DIR` should error, not quietly use the default.
- **Host hardening (recommended):** run future PoAW-X devnet rehearsals on a host that does
  **not** also run mainnet, eliminating the shared-default-directory hazard entirely.

## 7. Result

- The **Phase 24B rehearsal was not completed.**
- **No validation claim** is made from Phase 24B (no all-gates block, no observer validation,
  no restart/reload result).
- The next live rehearsal should run on **isolated `$HOME`-rooted directories** or, preferably,
  on a **host without mainnet**.

(Earlier phases remain unaffected: the all-gates consensus behavior is exercised by the test
suite — e.g. `chain::phase22e_true_vrf_e2e_block` validates a full all-gates block in-process —
and the Phase 23A/24A internal hardening + malformed-wire corpus remain green. Those are
separate from this paused live rehearsal.)
