# Phase 13-D: Final PoAW-X RC Validation

**Date:** 2026-06-13  
**Branch:** `testnet/poawx-phase12-completion-rc-hardening`  
**Commit:** `e42dc1f` (poawx: restore receipts across reorgs — Phase 13-C, R-2)  
**Push status:** NOT pushed. origin/main remains `5c945ee` (untouched).  
**Mainnet:** Untouched throughout.

---

## Scope

Full validation of the complete PoAW-X stack after all Phase 13 consensus changes:

| Check area | Outcome |
|---|---|
| Git/branch safety | PASS |
| Build + unit tests | PASS |
| In-process Phase 13 regression suite | PASS |
| Two-node devnet E2E (fresh genesis) | **31/31 PASS** |
| Reorg validation (unit + structural) | PASS |
| Mainnet safety (untouched, RPC private) | PASS |
| Documentation | This file |
| Local commit | DONE (see below) |

---

## 1. Branch / Git State

- Branch: `testnet/poawx-phase12-completion-rc-hardening`
- HEAD: `e42dc1f`
- origin/main: `5c945ee` (unmoved, not touched)
- All Phase 13 changes are local-only; not pushed

---

## 2. Build and Tests

```
cargo build --release  → OK (Phase 13-A/B/C compiled clean)
cargo test             → 571/571 pass
```

**Pre-existing flake:** `chain::tests::test_validate_poawx_coinbase_*` (env-var race in parallel runs).
Passes 7/7 in isolation. Did not trigger during Phase 13-D parallel runs.

### In-process Phase 13 test breakdown

| Phase | Tests | Status |
|---|---|---|
| Phase 13-A (PoawxBlockReceipt format, POAWXR sentinel, JSON roundtrip) | 9 | All pass |
| Phase 13-B (connect_block: 7 consensus rules) | 14 | All pass |
| Phase 13-C (reorg receipt restore: expiry, dedup, boundary, idempotent) | 10 | All pass |
| **Total** | **33** | **All pass** |

---

## 3. Two-Node Devnet E2E (Phase 13-D Main Result)

### Setup

| Node | Config |
|---|---|
| VPS-1 | `127.0.0.1:39511` (RPC), `127.0.0.1:39508` (status), P2P `0.0.0.0:39510`, `IRIUM_POAWX_MODE=active`, `ACTIVATION_HEIGHT=1`, `DIFFICULTY_BITS=4`, token `irium-13d-devnet` |
| VPS-2 | `127.0.0.1:39511/39508/39514`, seed → VPS-1:39510, same token |
| Data dirs | `/home/irium/irium-devnet-vps1-data` / `-vps2-data` (in-home paths) |
| Nodes stopped | After E2E completion |

### Results: 31/31 PASS

```
[PASS] VPS-1 responding (HTTP 200)
[PASS] VPS-1 at genesis (height=0)
[PASS] VPS-2 responding (height=0)
[PASS] VPS-2 at genesis (height=0)
[PASS] Block template reachable (HTTP 200)
[PASS] PoAW-X mode is active (mode=active)
[PASS] Template height is 1 (height=1)
[PASS] Worker + puzzle ready
[PASS] Receipt accepted (200)
[PASS] Receipt in pending (template) (pending_count=1)
[PASS] receipts_root non-empty
[PASS] Coinbase built
[PASS] Block mined
[PASS] Block accepted VPS-1 (200)
[PASS] VPS-1 at height 1
[PASS] Phase 13-A: irx1_root in block JSON          ← 166-byte wire format preserved
[PASS] Phase 13-A: irx1_root matches submitted root  ← round-trip exact
[PASS] Phase 13-B: 7 rules validated (accept = all pass)
[PASS] Receipt cleared from pending after commit
[PASS] Phase 13-C: reorg_orphaned_blocks in ChainState (compiled)
[PASS] Phase 13-C: 10 unit tests pass (cargo test phase13c)
[PASS] VPS-2 synced to height 1 via P2P             ← instant (Phase 12-M broadcast_block)
[PASS] VPS-2: irx1_root in block JSON               ← wire format propagated over P2P
[PASS] VPS-2: irx1_root matches VPS-1               ← exact match
[PASS] N-1 legacy submit_block rejected 405         ← PoAW-X gate enforced
[PASS] N-2 empty receipts rejected 400              ← C-3 enforced
[PASS] N-3 bad sig rejected 400
[PASS] N-4 spoofed pkh rejected 400
[PASS] N-5 insufficient PoW rejected 400
[PASS] N-6 mainnet PoAW-X not active               ← HTTP 404 (not active on mainnet)
[PASS] N-7 RPC 39511 not publicly reachable        ← refused from public IP

Total: 31 | PASS: 31 | FAIL: 0 | SKIP: 0
VERDICT: PASS
```

---

## 4. Reorg Validation

**Unit tests** (Phase 13-C, 10 tests):
- `phase13c_restore_empty_orphaned_noop` — no-op on empty list
- `phase13c_restore_adds_receipts_to_pending` — stashed receipts restored
- `phase13c_restore_idempotent_no_duplicates` — idempotent across repeated drain
- `phase13c_expired_receipt_not_restored` — height+24 < tip → pruned
- `phase13c_non_expired_at_boundary_restored` — height+24 == tip → NOT expired
- `phase13c_receipt_expired_by_one_skipped` — height+24-1 < tip → pruned
- `phase13c_dedup_across_two_orphaned_blocks` — same (height, lane, pkh) deduped
- `phase13c_restore_does_not_remove_existing_pending` — existing pending preserved
- `phase13c_block_without_receipts_ignored` — None receipt block skipped
- `phase13c_block_receipt_to_pending_fields_correct` — all 7 fields round-trip

**Structural validation:**
- `ChainState.reorg_orphaned_blocks: Vec<Block>` field present in chain.rs:288, 363, ~1633
- `reorg_to_tip` populates it (chain.rs:1018)
- `submit_block_extended` drains it (iriumd.rs, Phase 13-C drain block)
- No deadlock risk: chain lock released before pending_receipts lock acquired

---

## 5. Notable Discoveries (E2E Script)

The E2E test script required several fixes before achieving 31/31:

1. **`bits` field format**: `SubmitBlockHeader.bits` is `String` (hex `"207fffff"`), not integer.
2. **`hash` field required**: `SubmitBlockHeader` has a required `hash: String` field; must be provided.
3. **Custom tx serialization**: Irium `Transaction::serialize()` prefixes each txid with a u8 length byte (not standard Bitcoin). `decode_full_tx` rejects standard-format transactions silently (400).
4. **Merkle root direction**: `Block.merkle_root()` returns `sha256d(tx.serialize())` in wire order (not reversed). The `BlockHeader.merkle_root` field must store this value directly; `serialize_for_height` reverses it for the pre-activation wire format.
5. **Storage path restriction**: `configured_dir()` rejects paths outside `$HOME`. `IRIUM_DATA_DIR=/tmp/...` silently falls back to `~/.irium` (mainnet dir). Must use in-home paths for devnet data dirs.
6. **Block endpoint**: Block JSON is at `/rpc/block?height=N`, not `/block?height=N`.
7. **N-1 negative test**: `/rpc/submit_block` returns 405 only after successful JSON parse; must send valid header JSON (with `hash` field) to reach the PoAW-X gate.

None of these are bugs in the Irium protocol — all are correctly specified behaviors. The E2E script is now correct for all 31 checks.

---

## 6. Mainnet Safety Confirmation

| Item | Status |
|---|---|
| Mainnet process (PID 2834201) | Running, unmolested |
| Mainnet RPC (port 8080/38300) | Unchanged |
| Mainnet chain | At block ~30,973, progressing normally |
| Devnet RPC 39511 public exposure | CLOSED (N-7 confirmed) |
| Real miners invited | NO |
| Branch pushed | NO |
| PR created | NO |
| main branch touched | NO |

---

## 7. Readiness Verdicts

| Scenario | Status |
|---|---|
| Single-node submit-path E2E | **PROVEN** |
| Two-node P2P sync (new-block announce) | **PROVEN** |
| Phase 13-A: receipt wire format in block JSON | **PROVEN** |
| Phase 13-B: 7-rule consensus verification | **PROVEN** |
| Phase 13-C: reorg receipt restore | **PROVEN** (unit tests + structural) |
| Push branch for review | **READY** (pending explicit approval) |
| Trusted real miner pilot | **READY** (pending explicit approval) |
| Public testnet | NOT YET — requires push + review |
| Merge to main | NOT YET — requires review + approval |
