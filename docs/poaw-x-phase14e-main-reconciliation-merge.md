# PoAW-X Phase 14-E — Main Reconciliation Merge

**Date:** 2026-06-14
**Type:** Merge `origin/main` → PoAW-X testnet branch (local only; **not pushed**)

---

## 1. Merge Identity

| Item | Value |
|---|---|
| Branch | `testnet/poawx-phase12-completion-rc-hardening` |
| Pre-merge HEAD | `d65d6e7` (Phase 14-D audit doc) |
| origin/main merged | `1cbf190` (tag v1.9.114) |
| Merge-base | `277f537` |
| Strategy | `git merge --no-ff origin/main` (no rebase, no cherry-pick) |
| Local safety backup ref | `backup/pre-14e-merge-d65d6e7` |
| Merge commit | `5315699` (parents `d65d6e7` + `1cbf190`) |
| Doc commit | this commit (`docs: record poawx main reconciliation merge`) |
| Push status | **NOT pushed** (remote still `7c02dd0`) |

---

## 2. Conflicts Encountered & Resolutions

4 conflicting files. `src/bin/iriumd.rs` and `src/constants.rs` **auto-merged** (MTP timing, carrier filter, and `MTP_ACTIVATION_HEIGHT` integrated without conflict).

| File | Conflict | Resolution |
|---|---|---|
| `src/chain.rs` | Duplicate `use crate::constants::{…}` + `genesis` import; main side added `MTP_ACTIVATION_HEIGHT` | Kept PoAW-X import block, **added `MTP_ACTIVATION_HEIGHT`** to it, removed the duplicate import. No logic lost. |
| `src/bin/irium-miner.rs` | `coinbase_tag()` truncation formatting (multi-line HEAD vs single-line main) — functionally identical | Kept HEAD (rustfmt) form. Net result == HEAD (no behavioural change; main's coinbase_tag already equivalent on PoAW-X). |
| `Cargo.toml` | version `1.9.110` (HEAD) vs `1.9.114` (main) | Took **`1.9.114`**. |
| `Cargo.lock` | `irium-node-rs` `1.9.110` vs `1.9.113` (main lockfile drift) | Set to **`1.9.114`** to match `Cargo.toml`; validated by `cargo build`. |

No conflict markers remain anywhere in the tree.

---

## 3. Files Changed (22, +1037 / −151)

**consensus / node (the reconciliation targets):**
- `src/constants.rs` — `+MTP_ACTIVATION_HEIGHT = 32_000`
- `src/chain.rs` — `median_time_past()`, MTP timestamp validation (height ≥ 32000)
- `src/bin/iriumd.rs` — MTP block-template timing + carrier-filter hardening (stale BTC/LTC carrier drop)

**version:** `Cargo.toml`, `Cargo.lock` → `1.9.114`

**explorer / indexer / frontend / deploy (Category C — naturally included by full merge):**
- `explorer/api/{db,models,routes/mod,routes/stats}.rs`, `explorer/api/Cargo.toml`
- `explorer/indexer/src/db/write.rs`, `decoder/script.rs`, `migrations/002_coinbase_tag.sql`
- `frontend/src/...` (Layout, StatusBar, StatCard, MiningCalculatorCard, BlockListPage, Home, index.css, fmt.ts)
- `docker-compose.yml`

**No net change:** `src/bin/irium-miner.rs` (PoAW-X already had equivalent `coinbase_tag`; resolved to HEAD form).

---

## 4. Post-Merge Verification

**MTP (now present):**
- `src/constants.rs:29` `pub const MTP_ACTIVATION_HEIGHT: u64 = 32_000;`
- `src/chain.rs:383` `pub fn median_time_past(&self) -> u32`
- `src/chain.rs:1202` timestamp validation: at height ≥ 32000 reject `time <= median_time_past()`, else fall back to previous-block check
- `src/bin/iriumd.rs:13389` block-template time uses MTP at height ≥ 32000

**Carrier-filter hardening (now present):**
- `src/bin/iriumd.rs:13439-13464` `is_btc_carrier`/`is_ltc_carrier` + relay-tip connection check; drops stale carriers

**PoAW-X preservation checks (all intact):**
- connect_block order (`chain.rs:846-849`): `validate_block_header` (incl. MTP) → `validate_poawx_coinbase` → `validate_poawx_block_receipts` → apply txs. MTP and PoAW-X are orthogonal, both run pre-mutation. **Order safe.**
- B-1 lane canonicalisation (`lane.bytes().next().unwrap_or(b'A')`) — present
- N-1 `POAWX_MAX_PENDING_RECEIPTS = 255` — present
- N-2 `saturating_add` expiry — present
- N-3 `crate::poawx::POAWX_WORKER_REWARD_PERMILLE` — present
- receipt wire format / `POAWX_RECEIPT_SECTION_MAGIC` (`block.rs`) — present
- irx1 root verification, worker signature, reward split, reorg restore, devnet seed isolation, P2P broadcast — present
- Mainnet PoAW-X disabled guards — intact (no mainnet activation path changed)

---

## 5. Checks & Test Results

| Check | Result |
|---|---|
| `cargo build --release` | **OK** — `irium-node-rs v1.9.114` (3 pre-existing `unused_variable` warnings) |
| `cargo fmt` | applied (only `iriumd.rs` carrier-filter region needed wrapping — cosmetic) |
| `cargo fmt --check` | **clean (EXIT 0)** |
| `cargo test` (multi) | **1588 passed, 0 failed** |
| Targeted: poawx | 99 passed |
| Targeted: irx1 root | 19 passed |
| Targeted: timestamp/consensus | 9 passed |
| Targeted: devnet seed isolation (`test_12l`) | 5 passed |
| Targeted: reorg restore (`phase13c`) | 10 passed |
| `cargo test -- --test-threads=1` | **1588 passed, 0 failed** (iriumd 248 in 823s; lib 415; +571/307/26/16/5) |

Note: main's chain-level MTP shipped without a dedicated unit test; it is covered by existing timestamp-validation tests, the clean build, and the required Phase 14-F two-VPS retest. The carrier filter likewise has no unit test (validated by build + integration).

---

## 6. Remaining Risks

- **MTP path is unit-covered only indirectly.** Live behaviour at the activation boundary must be confirmed in the two-VPS retest (devnet runs at height 1 « 32000, so MTP does not trigger there — boundary behaviour is logic-verified, not yet live-verified).
- **Explorer/indexer/frontend** changes (Category C) are now in the branch but were validated only by `cargo build`/`cargo test` of Rust crates; the indexer migration `002_coinbase_tag.sql` and frontend are not exercised by the node test suite.
- **Version 1.9.114** now matches main; any further main advance will re-open drift.

---

## 7. Verdict

- **Merge succeeded:** YES
- **MTP now present:** YES
- **Carrier filter now present:** YES
- **PoAW-X validation still passes:** YES (99 poawx + 10 reorg + 19 irx1 targeted, full suite green)
- **Version reconciled to v1.9.114:** YES
- **Mainnet untouched:** YES (running PID unchanged; no restart/reconfigure; building over the binary file is inode-safe on Linux)
- **Push status:** **BLOCKED — not pushed**
- **PR / merge-to-main / rebase:** none
- **FULL TWO-VPS RETEST (Phase 14-F) REQUIRED:** **YES** — consensus paths (`chain.rs`/`constants.rs`/`iriumd.rs`) changed; re-run the 37/37 E2E including the MTP path before any push or pilot.
