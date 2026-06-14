# PoAW-X Phase 14-D — Main Compatibility Gap Audit

**Date:** 2026-06-14
**Type:** Audit only (no push, no PR, no merge, no rebase, no cherry-pick, no mainnet touch)

---

## 1. Preflight State

| Item | Value |
|---|---|
| Current branch | `testnet/poawx-phase12-completion-rc-hardening` |
| Local HEAD | `3452ede47ec5e7a1abf9d86a2e17236fbf0ef47b` (`3452ede`) |
| Working tree | Clean |
| `git fetch origin` | Done (only `gh-pages` advanced; `main` already current locally) |
| **Fetched origin/main** | **`1cbf190e5dca1c77faf58537b561cb2839bb08b6` (`1cbf190`, tag v1.9.114)** |
| Merge-base (HEAD ∩ origin/main) | `277f537945ef085a8b3dc3ac8bad21a97f0c3967` (`277f537` — "fix(api): use colon path syntax for axum 0.7 route parameters") |
| Remote testnet branch | `7c02dd0` (Phase 14-B) — local is **1 commit ahead** (unpushed Phase 14-C `3452ede`) |
| PR for branch | None (open or closed) |
| In-progress merge/rebase/cherry-pick | None |
| Push performed | No |

Note: `origin/main` is at `1cbf190` (v1.9.114), well beyond the `cec0070` recorded in memory and the `a6267dce` referenced in the task. Main **has advanced** during PoAW-X development.

Counts since merge-base: **25 commits on main** (9 non-merge feature commits), **51 commits on PoAW-X**.

---

## 2. Commits on main MISSING from PoAW-X (deduplicated, non-merge)

| Commit | Tag | Area | Summary |
|---|---|---|---|
| `0b01660` | v1.9.114 | **consensus** | Change MTP activation height to 32000 |
| `2c494e1` | v1.9.113 | **consensus** | Activate MTP (median-time-past) timestamp rule |
| `74e320b` | — (a6267dc) | explorer API | Add `/stats` endpoint (chain stats from DB + iriumd RPC) |
| `73a03c4` | v1.9.112 | **node/miner** | Fix block-template carrier filter: drop stale BTC/LTC carriers |
| `efbc152` | v1.9.111 | indexer | Fix indexer null-byte crash in `coinbase_tag`/`milestone_id` |
| `4c595b8` | — | frontend | Restyle explorer to irium-core theme |
| `b81563a` | — | frontend | Fix block age display for future timestamps |
| `0bba4a2` | v1.9.110 | miner | `IRIUM_COINBASE_TAG` env var — **(already on PoAW-X, equivalent)** |
| `a4fb465` | v1.9.110 | explorer | Index + expose `coinbase_tag` from scriptSig |

## 2b. Commits on PoAW-X NOT on main

51 commits — the full PoAW-X stack (Phases 10-B → 14-C) plus a parallel/cherry-picked `IRIUM_COINBASE_TAG` (`bfad3c3`, `0a60ad8`, `063841f` = version bump 1.9.110). No main feature is silently dropped by the branch; all PoAW-X-unique commits are additive.

---

## 3. Changed Files (main since merge-base), grouped by area

**consensus / chain / block:**
- `src/constants.rs` — `+MTP_ACTIVATION_HEIGHT = 32_000`
- `src/chain.rs` — `+median_time_past()`, MTP timestamp validation at height ≥ 32000

**node / miner / block template:**
- `src/bin/iriumd.rs` — MTP-based block-template time; carrier-filter hardening (drop stale BTC/LTC carriers not connecting to relay tip)
- `src/bin/irium-miner.rs` — `coinbase_tag()` + scriptSig tagging (**already present on PoAW-X**)

**explorer / indexer / API:**
- `explorer/api/src/routes/stats.rs` (new), `routes/mod.rs`, `models.rs` (`ChainStats`, `coinbase_tag` on BlockSummary/BlockDetail), `db.rs`
- `explorer/indexer/src/db/write.rs` (`coinbase_tag` indexing + null-byte `clean()`), `decoder/script.rs` (`extract_coinbase_tag`)
- `explorer/indexer/migrations/002_coinbase_tag.sql` (new — `ALTER TABLE blocks ADD COLUMN coinbase_tag TEXT`)
- `explorer/api/Cargo.toml`

**frontend / UI:**
- `frontend/src/components/{Layout,MiningCalculatorCard,StatCard,StatusBar}.tsx`, `pages/{Home,BlockListPage}.tsx`, `index.css`, `lib/fmt.ts`

**deployment / version:**
- `docker-compose.yml`
- `Cargo.toml` / `Cargo.lock` — version `1.9.114` (PoAW-X at `1.9.110`)

**Overlapping files (changed on BOTH sides — conflict-risk):** `Cargo.lock`, `Cargo.toml`, `src/bin/irium-miner.rs`, `src/bin/iriumd.rs`, `src/chain.rs`, `src/constants.rs`

---

## 4. Risk Classification

| # | Change | Class | Rationale |
|---|---|---|---|
| 1 | **MTP consensus rule** (`MTP_ACTIVATION_HEIGHT=32000`, `median_time_past`, timestamp validation, block-template time) | **A + D** | Consensus-critical; touches `constants.rs`/`chain.rs`/`iriumd.rs` (overlap). PoAW-X **lacks it entirely** (still "greater than previous block"). Must be brought in before merge or it would **revert a live consensus rule on main**. Does not trigger on the devnet E2E (height 1 « 32000), but required for consensus alignment. |
| 2 | **Block-template carrier filter hardening** (`73a03c4`) | **A + D** | Affects node block production; touches `iriumd.rs` (overlap). PoAW-X lacks `is_btc_carrier` stale-drop logic. Low impact on devnet (no BTC/LTC carriers) but needed for production/main consistency. |
| 3 | `IRIUM_COINBASE_TAG` miner feature | **E** | Already on PoAW-X (`bfad3c3`/`0a60ad8`); scriptSig format byte-identical (`Block {h}/{tag}`, `Block {h} solo {tag} `). No action. |
| 4 | axum 0.7 colon path syntax (`277f537`) | **E** | In the merge-base; present on PoAW-X. No action. |
| 5 | Explorer `coinbase_tag` indexing + null-byte fix (`a4fb465`,`efbc152`) | **C** | Separate indexer process; does not affect node/consensus/PoAW-X branch correctness. Bring before explorer redeploy. |
| 6 | `/stats` endpoint (`74e320b`) | **C** | Explorer API only. |
| 7 | Frontend restyle + block-age fix (`4c595b8`,`b81563a`) | **C** | UI only. |
| 8 | `docker-compose.yml` deployment | **C** | Deployment only; does not affect branch correctness. |
| 9 | Version bump `1.9.110 → 1.9.114` + `Cargo.lock` | **B + D** | `Cargo.toml`/`Cargo.lock` overlap; reconcile version before merge. |

No Category items were found that are missing AND break the current branch build/test (the branch is internally self-consistent).

---

## 5. Build / Test Gate (branch unchanged at 3452ede)

| Check | Result |
|---|---|
| `cargo fmt --check` | **EXIT=0 (clean)** |
| `cargo build --release` | **OK** (finished; 3 pre-existing `unused_variable` warnings) |
| `cargo test -- --test-threads=1` | **PASS — 0 failed** (iriumd 248, lib 415, +571/307/26/16/5 across binaries = 1588 tests; longest binary 814s) |

No build/test failure is attributable to missing main changes — the branch compiles and tests green on its own. Missing main work is **additive**, not a break.

---

## 6. Integration Recommendation

**Primary: merge `origin/main` into the branch** (`git merge origin/main`).
- The branch was already pushed (`7c02dd0`), so **rebasing 51 commits is discouraged** — it rewrites published history and replays conflicts across `chain.rs`/`constants.rs`/`iriumd.rs` at many commits.
- A single merge resolves conflicts once across the 6 overlap files, brings ALL main work (consensus + explorer + frontend), and leaves the branch a clean superset for eventual fast-forward merge to main.

**Minimal alternative: cherry-pick only consensus/node commits** — `2c494e1`, `0b01660` (MTP) and `73a03c4` (carrier filter) — and defer explorer/frontend (Category C). Use this if the testnet branch should stay focused and not absorb UI churn.

Either path must reconcile the version (`Cargo.toml` → ≥ `1.9.114` or next) and `Cargo.lock`.

---

## 7. Verdict

- **READY TO PUSH WITHOUT MAIN RECONCILIATION:** **NO** — the branch lacks a live consensus rule (MTP) now on main; pushing/advertising it as pilot-ready would misrepresent its consensus state. (The branch *ref* can be pushed without touching main, but it should not be treated as final.)
- **NEED MAIN RECONCILIATION BEFORE PUSH:** **YES**
- **SAFE TO REBASE NOW:** **NO** — branch is published (7c02dd0) and 51 commits long with consensus-file overlap; prefer merge or targeted cherry-pick.
- **SAFE TO CHERRY-PICK ONLY SELECTED COMMITS:** **YES** — MTP (`2c494e1`,`0b01660`) + carrier filter (`73a03c4`) are the minimal consensus/node set.
- **FULL TWO-VPS RETEST REQUIRED AFTER RECONCILIATION:** **YES** — any change to `chain.rs`/`iriumd.rs`/`constants.rs` consensus/template paths requires re-running the 37/37 two-VPS E2E.

**Push status: REMAINS BLOCKED.** No push, no PR, no merge, no rebase, no cherry-pick performed in this phase.

---

## 8. Recommended Action Sequence (for approval — not executed)

1. Create a working integration on the branch: `git merge origin/main` (or cherry-pick `2c494e1 0b01660 73a03c4`).
2. Resolve conflicts in the 6 overlap files — preserve PoAW-X logic AND add MTP (`median_time_past`, `MTP_ACTIVATION_HEIGHT`, timestamp validation, template timing) + carrier-filter hardening.
3. Reconcile `Cargo.toml` version (≥ 1.9.114) and regenerate `Cargo.lock`.
4. `cargo fmt --check` + `cargo build --release` + `cargo test -- --test-threads=1`.
5. Re-run full two-VPS E2E (target 37/37) — MTP path included.
6. Only then consider push / pilot / review.
