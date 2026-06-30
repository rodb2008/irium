# PoAW-X Phase 26 — Reproduction Commands (non-live)

Local, non-live commands an auditor can run. **No live node / VPS / firewall / sudo commands and no
secrets are included.** Baseline source `0208368` (HEAD `972bb9c`). `origin/main` unchanged
(`19c496dc5f2fa08981a109b10eeb257105c28c43`).

Requires a Rust toolchain (stable) and `git`. No network nodes are started by any command below.

## 1. Check out the baseline

```
git clone https://github.com/iriumlabs/irium.git
cd irium
git fetch origin
git checkout testnet/poawx-phase20-blueprint-completion-local
git pull --ff-only origin testnet/poawx-phase20-blueprint-completion-local
git rev-parse HEAD                 # docs HEAD: 972bb9c...
git ls-remote origin main          # expect 19c496dc5f2fa08981a109b10eeb257105c28c43 (unchanged)
git status --short                 # expect clean
```

## 2. Diff ranges (the actual changes to review)

```
# Full source change (the audit surface):
git diff 30bce64..0208368 -- 'src/*.rs'

# Per phase:
git diff 30bce64..081a1bd          # 26B epoch-seed reconciliation
git diff bfe16fd..abb2fd3          # 26D admission persistence
git diff abb2fd3..0208368          # 26E historical-admission serving

# Confirm phase22a and consensus modules are UNCHANGED:
git diff 30bce64..0208368 -- src/chain.rs | grep -nE "fn validate_block_committed_admission|matches_candidate_set"
git diff --name-only 30bce64..0208368 | grep -iE "pow\.rs|lwma|difficulty|target|reward|constants"   # expect: no output
```

## 3. Focused tests (serialized)

```
cargo test phase26b_multiblock_epoch_seed_soak --lib -- --test-threads=1
cargo test phase26d --lib -- --test-threads=1
cargo test phase26e --lib -- --test-threads=1
cargo test admission --lib -- --test-threads=1
cargo test p2p --lib -- --test-threads=1
cargo test --lib phase26 -- --test-threads=1
```

Key tests:
- `chain::phase26b_multiblock_epoch_seed_soak` — 6-block `connect_block` chain + seed invariant.
- `chain::phase26b_stale_immediate_parent_seed_rejected`,
  `chain::phase26b_committed_admission_root_and_replay_rejected` — negatives.
- `chain::phase26d_cold_replay_with_persisted_admissions` — phase21e rejects empty cache; reconnects
  after reload.
- `poawx_admission::phase26d_persist_reload_roundtrip`,
  `poawx_admission::phase26d_reload_rejects_invalid_records`.
- `chain::phase26e_fresh_sync_via_served_admissions` — fresh node syncs via served admissions; rejects
  tampered.

## 4. Full lib suite (serialized — required)

```
cargo test --lib -- --test-threads=1
```

Run **serialized**: PoAW-X tests mutate process-global env and the global admission cache. One
pre-existing test (`phase24k_native_pow_all_gates_validators`) does not take the shared env lock and is
flaky **only under parallel execution**; it passes in isolation and serialized. Expected totals on this
baseline: ~748 passed / 0 failed. (Do not interpret a parallel-run flake as a regression.)

## 5. Release build

```
cargo build --release --bin iriumd --bin poawx-live-proof-harness
```

## 6. Where to read (no commands needed)

- `docs/audit/phase26h-kickoff/` — this kickoff package.
- `docs/audit/poawx-phase26-independent-audit-package.md`, `...-technical-appendix.md`,
  `...-auditor-checklist.md`.
- `docs/poaw-x-phase26{b,c,d,e}-*.md` — per-phase implementation + live-validation results (logs
  summarized; no secrets).

## Notes
- These commands do not start any node, open any port, change any firewall, or use sudo.
- Do not run live multi-host or public-testnet commands as part of the audit; the live results are
  summarized in the per-phase docs and the review guide. A separately-approved live exercise would
  follow the `docs/poaw-x-phase26g-*` runbook.
