# PoAW-X Phase 26 — Commit Map

Every Phase 26 commit on `testnet/poawx-phase20-blueprint-completion-local`, with type, purpose, key
files, and the source ranges an auditor reviews. **NOT audited / production-ready / mainnet-ready.**
`origin/main` unchanged at `19c496dc5f2fa08981a109b10eeb257105c28c43` throughout.

> Note on SHAs: some docs phases were landed via the VPS-1 `git format-patch`/`git am` push fallback
> when the Windows credential-manager hung; `git am` regenerates committer metadata, so the landed
> remote SHA can differ from the original local SHA. The SHAs below are the **landed branch** SHAs.

## Commit table

| Phase | Commit | Type | Purpose | Important files |
|-------|--------|------|---------|-----------------|
| 26A | `30bce64` | docs | Seed-reconciliation design (Option C = epoch-seed/grandparent) | `docs/poaw-x-phase26a-seed-reconciliation-design.md` |
| 26B | `081a1bd` | code | Epoch-seed alignment: phase21d/21e expect `admission_epoch_seed`; phase22a unchanged | `src/chain.rs`, `src/poawx_committed_admission.rs`, `src/poawx_mining_harness.rs`, `src/bin/poawx-live-proof-harness.rs` |
| 26C | `bfe16fd` | docs | Live three-system 6-block soak result | `docs/poaw-x-phase26c-live-multiblock-soak.md` |
| 26D | `de13a83` | code | Persist + reload (re-validate) candidate admissions for cold replay | `src/poawx_admission.rs`, `src/storage.rs`, `src/bin/iriumd.rs`, `src/chain.rs` (test) |
| 26D | `abb2fd3` | docs | 26D live validation (restart cold-resync PASSED) | `docs/poaw-x-phase26d-admission-cache-persistence.md` |
| 26E | `9de939f` | code | Serve historical admissions during block-serve (receiver re-validates; bounded) | `src/p2p.rs`, `src/chain.rs` (test) |
| 26E | `0208368` | docs | 26E live validation (fresh-wipe sync PASSED) — **last source-bearing commit** | `docs/poaw-x-phase26e-historical-admission-sync.md` |
| 26F | `c15c436` | docs | Independent-audit package | `docs/audit/poawx-phase26-*` |
| 26G | `972bb9c` | docs | Public-testnet readiness package | `docs/poaw-x-phase26g-public-testnet-*` |
| 26H | `1217c85` | docs | Independent-audit kickoff package | `docs/audit/phase26h-kickoff/*` |
| 26I | `22dfde8` | docs | Internal self-review (748/0; phase22a byte-unchanged proof) | `docs/audit/phase26i-self-review/*` |
| 26J | `0e196ba` | docs | External auditor handoff package | `docs/audit/phase26j-external-handoff/*` |
| 26K | `6c7681a` | docs | Audit response / remediation workflow | `docs/audit/phase26k-remediation-workflow/*` |
| 26L | `208d5ff` | docs | Audit engagement tracker | `docs/audit/phase26l-engagement-tracker/*` |
| 26M | _this commit_ | docs | Final program summary + index + commit map + decision tracker | `docs/poaw-x-phase26-*` |

Only the four **code** commits (`081a1bd`, `de13a83`, `9de939f`, and the test-only portions in
`abb2fd3`/`0208368`) touch source; everything else is docs.

## Source ranges for audit

| Change | Range | Code commit | Surface |
|--------|-------|-------------|---------|
| Seed reconciliation (26B) | `30bce64..081a1bd` | `081a1bd` | `src/chain.rs` (phase21d/21e seed), `src/poawx_committed_admission.rs` (`admission_epoch_seed`), builders |
| Admission persistence (26D) | `bfe16fd..abb2fd3` | `de13a83` | `src/poawx_admission.rs` (persist/reload/re-validate), `src/storage.rs`, `src/bin/iriumd.rs` |
| Historical-admission serving (26E) | `abb2fd3..0208368` | `9de939f` | `src/p2p.rs` (`send_historical_admissions` + 4 call sites + receiver ingest) |
| **Full source range** | **`30bce64..0208368`** | — | **8 source files, +1006/−47** (rest = tests + docs) |
| Audit / readiness / process docs | `0208368..208d5ff` | — | docs-only (26F–26M); no source change |

## Verification one-liners (non-live)

```
git diff --stat 30bce64..0208368 -- 'src/*.rs'                 # 8 files, +1006/-47
git diff 30bce64..0208368 -- src/chain.rs | grep -nE "fn validate_block_committed_admission"   # phase22a sig untouched
git diff --name-only 30bce64..0208368 | grep -iE "pow\.rs|lwma|difficulty|target|reward|constants"   # (no output)
```

See `docs/audit/phase26h-kickoff/REPRO_COMMANDS.md` and `docs/audit/phase26i-self-review/REPRO_EVIDENCE.md`
for the full, already-run evidence (phase22a byte-identical; 748/0 tests; release build).
