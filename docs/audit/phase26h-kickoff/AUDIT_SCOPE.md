# PoAW-X Phase 26 — Audit Scope

Baseline HEAD `972bb9c` (docs); last source change `0208368`. `origin/main` unchanged
(`19c496dc5f2fa08981a109b10eeb257105c28c43`). **NOT audited / production-ready / mainnet-ready.**

## Exact source diff ranges

| Change | Range | Code commit | Primary files |
|--------|-------|-------------|---------------|
| **26B** epoch-seed reconciliation | `30bce64..081a1bd` | `081a1bd` | `src/chain.rs`, `src/poawx_committed_admission.rs`, `src/poawx_mining_harness.rs`, `src/bin/poawx-live-proof-harness.rs` |
| **26D** admission persistence | `bfe16fd..abb2fd3` | `de13a83` | `src/poawx_admission.rs`, `src/storage.rs`, `src/bin/iriumd.rs`, `src/chain.rs` (test) |
| **26E** historical-admission serving | `abb2fd3..0208368` | `9de939f` | `src/p2p.rs`, `src/chain.rs` (test) |
| **Full source range** | `30bce64..0208368` | — | 8 source files, +1006/−47 (rest = tests + docs) |

Reproduce: `git diff <range> -- 'src/*.rs'`. See `REPRO_COMMANDS.md`.

## Files to prioritize (consensus/validation path)

1. **`src/chain.rs`** — `validate_block_candidate_sets` (phase21d/21e): the expected candidate-set
   seed is now `admission_epoch_seed(...)`, and the phase21e admitted-set lookup is keyed on it. The
   equality check `cs == admitted_candidate_set` is unchanged. (The large line count here is added
   tests: `phase26b_*`, `phase26d_cold_replay_with_persisted_admissions`,
   `phase26e_fresh_sync_via_served_admissions`.)
2. **`src/poawx_committed_admission.rs`** — pure helper `admission_epoch_seed(parent_prev, block_prev)`
   (grandparent hash; genesis at the activation boundary). **Confirm `validate_block_committed_admission`
   (phase22a) is UNCHANGED** vs `30bce64` (see below).
3. **`src/poawx_admission.rs`** — `CandidateAdmissionV1::validate`; `NodeCandidateAdmissionCache`
   methods `ingest_bytes` (live), `reload_persisted_bytes`, `load_persisted`, `persist_snapshot`,
   `admissions_for_height`, `admitted_candidate_set`, keys `(target_height, role_id, solver_pkh)`,
   window/prune logic.
4. **`src/p2p.rs`** — `send_historical_admissions(writer, peer, start_height, block_count)` and its
   four call sites (two `GetBlocks` serve handlers + two "no getblocks after headers, pushing N
   blocks" handshake-push paths); the receiver `PoawxCandidateAdmission` handler (ingest via the
   normal path). The change is purely additive (getblocks gating/locator untouched).
5. **`src/storage.rs`** — `candidate_admissions_file()` path (under the isolated data root; never
   `/tmp`/`.irium`).
6. **`src/bin/iriumd.rs`** — startup hook in `load_persisted_blocks` (set persist path + reload before
   the persisted-block replay).
7. **`src/poawx_mining_harness.rs`** and **`src/bin/poawx-live-proof-harness.rs`** — the devnet/test
   block **builder** (mainnet-hard-off). **NOT validators.** In scope only to confirm: they do not run
   on mainnet, they produce blocks the node still independently validates, and they print no secrets.

## phase22a must be confirmed UNCHANGED

A core premise is that **`validate_block_committed_admission` (phase22a) was not modified**. The fix
corrected only phase21d/21e's *expected seed value*, not phase22a. The auditor should verify directly:

```
git diff 30bce64..0208368 -- src/chain.rs | grep -nE "fn validate_block_committed_admission|matches_candidate_set"
# expect: no changes to the function body (only incidental comment mentions of "phase22a")
git diff 30bce64..0208368 -- src/poawx_committed_admission.rs
# expect: only the new admission_epoch_seed helper added; AdmissionCommitmentV1/matches_candidate_set unchanged
```

## Explicitly NOT changed (confirm)

- phase21d's other checks (canonical, best-for-role, dominance-weight match, AVR2 binding,
  admitted-set equality) and phase21e equality logic.
- phase22a (`validate_block_committed_admission`) and `matches_candidate_set`.
- phase22d (true-VRF), phase21f (puzzle), phase21h (finality), phase21c (dominance) validators.
- `src/pow.rs`, LWMA, difficulty, target, block reward (`src/constants.rs`) — verify none changed in
  the range.
- Mainnet behavior (PoAW-X hard-off for `network_id == 0`).
