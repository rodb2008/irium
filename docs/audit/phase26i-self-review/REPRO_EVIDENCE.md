# PoAW-X Phase 26 — Self-Review Reproduction Evidence (Phase 26I)

Summarized outputs of the non-live commands run during the internal self-review. **Not an independent
audit.** No live nodes, firewall, sudo, mainnet, or wallet access were involved. No secrets included.

Baseline: branch `testnet/poawx-phase20-blueprint-completion-local`, HEAD `1217c85`, source `0208368`,
`origin/main` `19c496dc5f2fa08981a109b10eeb257105c28c43` (unchanged).

## 1. Repo state

```
git fetch origin
git checkout testnet/poawx-phase20-blueprint-completion-local   # Already on ...
git pull --ff-only origin ...                                   # Already up to date
git rev-parse HEAD            -> 1217c85cf53334434cd093e870fea99fa4d2092f
git ls-remote origin main    -> 19c496dc5f2fa08981a109b10eeb257105c28c43
git status --short           -> (empty: clean working tree)
```

## 2. Source diff surface

```
git diff --stat 30bce64..0208368 -- 'src/*.rs'
```
```
 src/bin/iriumd.rs                   |  14 +
 src/bin/poawx-live-proof-harness.rs |  29 +-
 src/chain.rs                        | 537 +++++++++++++++++++++++++++++++++++-
 src/p2p.rs                          |  77 ++++++
 src/poawx_admission.rs              | 228 +++++++++++++++
 src/poawx_committed_admission.rs    |  29 ++
 src/poawx_mining_harness.rs         | 130 ++++++---
 src/storage.rs                      |   9 +
 8 files changed, 1006 insertions(+), 47 deletions(-)
```
All seven referenced commits (`30bce64 081a1bd bfe16fd abb2fd3 de13a83 9de939f 0208368`) resolve in
the local repo.

## 3. phase22a unchanged — proof

`validate_block_committed_admission` lives at `src/chain.rs:1175` (spans lines 1175–1264, 90 lines).
Extracted the function from both revisions and diffed:

```
git show 30bce64:src/chain.rs | sed -n '/^    fn validate_block_committed_admission/,/^    fn [a-z]/p' > old
git show 0208368:src/chain.rs | sed -n '/^    fn validate_block_committed_admission/,/^    fn [a-z]/p' > new
diff old new   -> (empty)   ==> IDENTICAL: phase22a body unchanged
```
Also: `git diff 30bce64..0208368 -- src/chain.rs | grep -E "fn validate_block_committed_admission|fn matches_candidate_set"`
returned no diff lines (function signatures untouched).

`src/poawx_committed_admission.rs` diff in range = **only** the new `admission_epoch_seed(...)` helper
added after line 222; `AdmissionCommitmentV1` / `matches_candidate_set` unchanged.

## 4. Consensus params unchanged — proof

```
git diff --name-only 30bce64..0208368 | grep -iE "pow\.rs|lwma|difficulty|target|reward|constants"
-> (no matches)   ==> PoW/LWMA/difficulty/target/reward/constants untouched
```

## 5. phase21d/21e change — what actually changed

In `validate_block_candidate_sets` (`src/chain.rs`), two lines changed value (not logic):
- `if cs.seed != block.header.prev_hash` → `if cs.seed != epoch_seed`
- `admitted_candidate_set(net, height, &block.header.prev_hash)` → `... &epoch_seed`

where `epoch_seed = admission_epoch_seed(self.chain.last().map(|p| p.header.prev_hash), block.header.prev_hash)`.
The phase21e equality check is unchanged:
```
if cs.serialize() != admitted.serialize() {
    return Err("phase21e: candidate set does not match admitted candidates".to_string())
}
```
Error strings `phase21d: candidate set wrong seed` and `phase21e: ...` are preserved.

## 6. Serving bound — proof

`async fn send_historical_admissions(writer_weak, peer, start_height, block_count)` (`src/p2p.rs:6693`):
```
if block_count == 0 { return; }
let cap = block_count.saturating_mul(16);
let mut sent = 0;
for h in start_height..(start_height + block_count) {
    for adm in cache.admissions_for_height(h) {
        if sent >= cap { return; }      // hard upper bound
        ... send PoawxCandidateAdmissionPayload ...
        sent += 1;
    }
}
```
Called at 4 block-serve sites: `src/p2p.rs:5113, 5932, 7626, 8481` (definition at 6693). Purely
additive to the existing block-serve paths.

## 7. Receiver / reload re-validation — proof

- Receiver handler `MessageType::PoawxCandidateAdmission` (`src/p2p.rs:6506`) calls
  `global_admission_cache().ingest_bytes(&p.admission_bytes)` — full validation on the normal path.
- `reload_persisted_bytes` (`src/poawx_admission.rs:518`): rejects wrong `network_id` (`:526`), calls
  `adm.validate(adm.network_id, adm.target_height)` and rejects on error (`:529`), rejects a
  conflicting digest for an existing key (`:539`). Returns false (skip) rather than panicking on bad
  input.

## 8. Mainnet hard-off — proof

`candidate_admission_gate(network_id, activation, height)` (`src/poawx_admission.rs:66`) returns
`false` when `network_id == 0`; the committed-admission gate in `poawx_committed_admission.rs:277`
does the same. Multiple `"mainnet hard-off"` guards present in both modules.

## 9. Tests

| Command | Result |
|---------|--------|
| `cargo test phase26b_multiblock_epoch_seed_soak --lib -- --test-threads=1` | 1 passed / 0 failed |
| `cargo test --lib phase26b -- --test-threads=1` | 3 passed / 0 failed (incl. `phase26b_stale_immediate_parent_seed_rejected`, `phase26b_committed_admission_root_and_replay_rejected`) |
| `cargo test phase26d --lib -- --test-threads=1` | 3 passed / 0 failed (`phase26d_cold_replay_with_persisted_admissions`, `phase26d_persist_reload_roundtrip`, `phase26d_reload_rejects_invalid_records`) |
| `cargo test phase26e --lib -- --test-threads=1` | 1 passed / 0 failed (`phase26e_fresh_sync_via_served_admissions`) |
| `cargo test --lib -- --test-threads=1` | **748 passed / 0 failed / 0 ignored** (25.70s) |

Note: the full suite is run **serialized** (`--test-threads=1`) because PoAW-X tests mutate
process-global env and the global admission cache; one pre-existing test
(`phase24k_native_pow_all_gates_validators`) is parallel-only flaky and passes serialized. Build
emitted 4 pre-existing cosmetic warnings (unused `committee` at `chain.rs:9237`; self-assign at
`poawx.rs:2346`) — see `INTERNAL_FINDINGS.md`.

## 10. Release build

```
cargo build --release --bin iriumd --bin poawx-live-proof-harness   -> Finished (exit 0)
target/release/iriumd.exe                      ~19.1 MB
target/release/poawx-live-proof-harness.exe    ~6.2 MB
```
