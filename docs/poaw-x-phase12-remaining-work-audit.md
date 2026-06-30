# PoAW-X Phase 12-A: Remaining Work Audit

**Branch:** `testnet/poawx-phase12-completion-rc-hardening`
**Based on:** `testnet/poawx-phase11f-real-external-miner-validation` @ ac31fde
**Date:** 2026-06-12
**Author:** Phase 12-A automated audit
**Status:** NOT READY FOR REAL MINERS

---

## 1. Branch / Commit

| Field | Value |
|-------|-------|
| Audit branch | `testnet/poawx-phase12-completion-rc-hardening` |
| HEAD at audit | `ac31fde` |
| `origin/main` | `5c945ee` (untouched) |
| Push status | NOT pushed |

---

## 2. Mainnet Untouched Confirmation

- VPS-1 mainnet iriumd PID 2065028 — not modified, not restarted.
- VPS-2 mainnet iriumd PID 1744330 — not modified, not restarted.
- `origin/main` remains at `5c945ee`.
- No merge to main was performed.
- No mainnet port (38300, 38291, 8080) was touched.
- Consensus files (`chain.rs`, `consensus.rs`, `pow.rs`, `settlement.rs`, `activation.rs`) were not modified.
- This audit branch is local-only.

---

## 3. Audit Scope

Files inspected:

- `src/bin/iriumd.rs` (27,498 lines) — full PoAW-X HTTP layer
- `src/bin/irium-miner.rs` — standard miner binary
- `src/bin/irium-miner-gpu.rs` — GPU miner binary
- `src/protocol.rs` — P2P protocol message types
- `src/chain.rs`, `src/consensus.rs`, `src/block.rs`, `src/lib.rs` — chain/consensus layer
- `pool/irium-stratum/src/stratum.rs` — Stratum PoAW-X wiring
- `pool/irium-stratum/src/block.rs` — receipts_root / irx1 builder
- `pool/irium-stratum/src/template.rs` — PoAW-X template fields
- `docs/poaw-x-prototype-plan.md` — planned vs implemented tracker

Grep patterns used: `TODO`, `FIXME`, `HACK`, `unimplemented!`, `panic!`, `poaw`, `irx1`,
`PoAW`, `PUZZLE_DIFFICULTY`, `reward_split`, `operator_share`, `miner_share`, `activation`,
`cfg(test)`, `bypass`, `skip_validation`, `disabled`, `persist`, `reorg`, `IBD`,
`worker_pkh`, `auth`, `P2P`, `gossip`.

---

## 4. Completed PoAW-X Areas

| Area | Status | Evidence |
|------|--------|----------|
| `/poawx/assignment` GET endpoint | COMPLETE | Phase 10-D, 11-B |
| `/poawx/receipt` POST endpoint | COMPLETE | Phase 10-D, 11-B |
| `commitment_nonce` derivation (SHA256 of parent_hash) | COMPLETE | Phase 11-B |
| Solution PoW check at POST receipt (SHA256d, PUZZLE_DIFFICULTY=1) | COMPLETE | Phase 11-B |
| `compute_poawx_receipts_root` canonical sort (height+lane+worker_pkh+nonce) | COMPLETE | Phase 11-B |
| `irx1` OP_RETURN format (0x6a 0x24 "irx1" 32-byte root = 38 bytes) | FINAL | Phase 11-B |
| `irx1` coinbase validation in `submit_block_extended` | COMPLETE | Phase 11-B |
| Receipt root mismatch rejection | COMPLETE | Phase 11-B |
| `commitment_nonce` mismatch rejection per-receipt in SBE | COMPLETE | Phase 11-B |
| `IRIUM_POAWX_MODE` env-var gate on assignment/receipt endpoints | COMPLETE | Phase 11-A |
| `is_non_mainnet` guard on assignment/receipt endpoints | COMPLETE | Phase 11-A |
| Stratum irx1 injection (`build_irx1_commitment_script`) | COMPLETE | Phase 10-B |
| Stratum `submit_block_extended` dispatch when receipts present | COMPLETE | Phase 10-D |
| `compute_receipts_root_from_pending` in stratum matches iriumd | COMPLETE | Phase 11-B |
| `getblocktemplate` PoAW-X fields (`poawx_mode`, `poawx_pending_receipts`, `receipts_root`) | COMPLETE | Phase 10-D |
| `/rpc/block` returns `irx1_root` field | COMPLETE | Phase 11-C |
| Pending receipts cleared on block acceptance | COMPLETE | Phase 10-D |
| Rate limiting on `/poawx/assignment` and `/poawx/receipt` (IP-based) | COMPLETE | Existing infra |
| Bogus share rejection | VALIDATED | Phase 11-D |
| Mainnet hard-disable on assignment/receipt endpoints | SOLID | Phase 11-A, 11-D |
| P2P sync between VPS-1 and VPS-2 testnet | VALIDATED | Phase 11-D, 11-E |
| Direct TCP stratum reachability | VALIDATED | Phase 11-D |

---

## 5. Incomplete PoAW-X Areas

### 5.1 CRITICAL — Consensus / Architecture

**[C-1] PoAW-X is NOT a consensus rule — chain layer has zero irx1 validation**

- Files: `src/chain.rs`, `src/consensus.rs`, `src/block.rs`
- Finding: `grep -rn 'irx1|poawx' src/*.rs` returns **zero hits** outside `iriumd.rs`.
- Impact: All P2P-synced blocks bypass irx1 validation entirely. A node doing IBD
  (initial block download) will accept any block regardless of irx1 presence or validity.
  PoAW-X enforcement exists only at the local operator HTTP API layer.
- Required fix: Move irx1 validation (when `IRIUM_POAWX_MODE=active`) into
  `chain.rs::connect_block()` or a new consensus hook so all code paths (P2P, API, sync)
  enforce the rule uniformly.

**[C-2] `submit_block` (pool-compatible) endpoint completely bypasses PoAW-X**

- File: `src/bin/iriumd.rs:14211` — `async fn submit_block`
- Route: `/rpc/submit_block`
- Finding: Zero PoAW-X checks in this function. Any pool miner using the standard
  `/rpc/submit_block` path can mine blocks with no `irx1` commitment.
- Required fix: Either reject calls to `/rpc/submit_block` when `IRIUM_POAWX_MODE=active`,
  or add the same `irx1` validation logic from `submit_block_extended`.

**[C-3] PoAW-X is fully opt-in — irx1 check skipped when receipts are empty**

- File: `src/bin/iriumd.rs:13718,13848`
- Finding: All `irx1` and receipt validation in `submit_block_extended` is gated on
  `if !req.poawx_receipts.is_empty()`. A miner can submit an empty receipt list and the
  block is accepted without any PoAW-X validation.
- Impact: PoAW-X participation is voluntary. No miner is required to solve puzzles.
- Required fix: When `IRIUM_POAWX_MODE=active`, reject `submit_block_extended` calls
  with no receipts (or enforce a minimum participation threshold per window).

### 5.2 HIGH — Puzzle Difficulty

**[D-1] `PUZZLE_DIFFICULTY` hardcoded to 1 (requires only 1 leading zero bit)**

- File: `src/bin/iriumd.rs:13602,13673,13785`
- Evidence:
  - `"puzzle_difficulty": 1u64` returned in assignment response
  - `const PUZZLE_DIFFICULTY: u32 = 1` in `poawx_post_receipt`
  - `if leading < 1` (raw literal) in `submit_block_extended`
- Impact: 1 leading zero bit is trivially solvable in microseconds. This is a prototype
  test value with no real proof-of-work separation.
- Required fix: Implement adaptive puzzle difficulty targeting ~1–10 seconds per solution
  at a reference hash rate, configurable via env var for testnet, returned dynamically
  in the assignment response.

**[D-2] Inconsistent difficulty constant representation**

- `poawx_post_receipt` uses named constant `const PUZZLE_DIFFICULTY: u32 = 1`.
- `submit_block_extended` uses raw magic literal `if leading < 1`.
- Required fix: Extract to a single shared constant so both enforcement points track
  the same value.

### 5.3 HIGH — Receipt Integrity

**[R-1] No operator reward split enforcement**

- Finding: `grep -n 'reward.*split|operator.*share|miner.*share'` in `iriumd.rs` returns
  zero PoAW-X hits.
- Impact: The `irx1` commitment in coinbase is validated, but there is no check that the
  miner has paid the operator their share of the block reward. An operator could receive
  0 IRM while still satisfying the irx1 check.
- Required fix: Define reward split rules in the PoAW-X spec and enforce coinbase output
  value distribution in `submit_block_extended`.

**[R-2] No receipt persistence — in-memory only**

- File: `src/bin/iriumd.rs:203` — `poawx_pending_receipts: Arc<Mutex<Vec<...>>>`
- Confirmed in `docs/poaw-x-prototype-plan.md`: "Receipt persistence: IN-MEMORY ONLY (deferred)"
- Impact: Any `iriumd` restart (crash, update, OOM kill) clears all pending receipts.
  Miners who solved puzzles lose their work; the operator's next `getblocktemplate`
  will have an empty receipt pool.
- Required fix: Persist pending receipts to disk (JSON file in `state_dir()`) on each
  write; reload on startup.

**[R-3] No reorg handling for PoAW-X receipts**

- File: `src/bin/iriumd.rs:13905`
- Finding: When a block is accepted, receipts for that height are removed from the
  pending pool. If the block is later orphaned, the cleared receipts are NOT restored.
- Impact: After a reorg, the pending pool for the orphaned height is empty, so the
  next block at that height will have no `irx1` commitment.
- Required fix: On reorg (chain disconnect), re-add cleared receipts for the orphaned
  height back to the pending pool.

**[R-4] No worker authentication — `worker_pkh` is untrusted**

- File: `src/bin/iriumd.rs:13608` — `poawx_post_receipt`
- Finding: `worker_pkh` is accepted as a plain string with no signature or proof of
  ownership of the corresponding private key.
- Impact: An attacker can submit receipts claiming arbitrary worker addresses, polluting
  the `irx1` root with illegitimate entries.
- Required fix: Require a signature over the receipt payload (height + lane + solution +
  commitment_nonce) by the private key corresponding to `worker_pkh`.

**[R-5] No pending receipt pool size cap**

- File: `src/bin/iriumd.rs:13692`
- Finding: The pending receipts `Vec` has no upper bound. Per-worker dedup exists
  (`retain` removes old entry for same height+lane+worker_pkh), but high-volume attacks
  with varied `worker_pkh` values can grow the list unboundedly.
- Required fix: Cap the pending pool at a configurable maximum (e.g. 1000 entries);
  evict oldest entries on overflow.

### 5.4 HIGH — Mining Integration

**[M-1] Standard miner (`irium-miner.rs`) has zero PoAW-X integration**

- File: `src/bin/irium-miner.rs`
- Finding: `grep -rn 'poaw|irx1|PoAW' src/bin/irium-miner.rs` returns zero hits.
- Impact: External miners using the built-in `irium-miner` binary cannot participate
  in PoAW-X at all. They must use the stratum path exclusively.

**[M-2] GPU miner has post-fork height TODO blocking correct PoAW-X block submission**

- File: `src/bin/irium-miner-gpu.rs:1448`
- Line: `// TODO(fix-2a): derive height from coinbase BIP34 for post-fork mining.`
- Impact: GPU miner uses `hash_for_height(0)` for all blocks. Post-fork block submissions
  will be rejected with a hash mismatch error.

### 5.5 MEDIUM — Test Coverage

**[T-1] Zero PoAW-X unit or integration tests**

- File: `src/bin/iriumd.rs` — 152 `#[tokio::test]` functions, **none** cover PoAW-X paths.
- Missing test coverage:
  - `/poawx/assignment` seed/nonce derivation correctness
  - `/poawx/receipt` accept valid, reject bad nonce, reject insufficient PoW
  - `submit_block_extended` accept with valid irx1, reject missing irx1, reject root mismatch,
    reject bad `commitment_nonce` per-receipt
  - `submit_block_extended` reject empty receipts when mode=active
  - Mainnet guard on assignment, receipt, and SBE endpoints
  - `compute_poawx_receipts_root` parity with stratum `compute_receipts_root_from_pending`
  - Receipt persistence round-trip (write + restart + reload)
  - Reorg receipt restoration
  - Pool size cap / eviction

**[T-2] No automated end-to-end PoAW-X regression test in `cargo test`**

- Existing Python harnesses (`scripts/poawx-phase11b-canonical-receipts-validation.py`)
  are manual CLI scripts, not part of `cargo test`. No CI coverage of PoAW-X paths.

### 5.6 MEDIUM — RPC / Operator View

**[O-1] No operator endpoint to inspect the pending PoAW-X receipt pool**

- Finding: No `/rpc/poawx/pending`, `/rpc/poawx/status`, or equivalent route registered.
- Impact: Operators cannot introspect the pending receipt pool via the private RPC.
  They must rely on `getblocktemplate` responses (stratum-oriented) or daemon logs.
- Required fix: Add a private (RPC-auth-required) `GET /rpc/poawx/pending` endpoint
  returning pending count, current heights, worker addresses, and current root.

**[O-2] `submit_block_extended` missing mainnet guard for PoAW-X receipts**

- File: `src/bin/iriumd.rs:13710`
- Finding: The `IRIUM_POAWX_MODE` / `is_non_mainnet` guard exists in `poawx_get_assignment`
  (line 13562) and `poawx_post_receipt` (line 13615), but NOT in `submit_block_extended`.
- Impact: A crafted submission with non-empty receipts can reach the PoAW-X validation
  branch on a mainnet node. Because the mainnet node's pending pool is always empty,
  the `expected_root` is all-zeros; if the submitted root is also all-zeros (e.g. the
  client sends `poawx_receipts_root: ""`), the check at line 13734 passes silently.
- Required fix: Add `if is_mainnet && !req.poawx_receipts.is_empty() { return Err(SERVICE_UNAVAILABLE); }`
  near the top of `submit_block_extended`.

### 5.7 MEDIUM — Protocol / P2P

**[P-1] No P2P gossip for PoAW-X puzzle receipts**

- File: `src/protocol.rs`
- Finding: `MessageType` enum (variants 1–25) has no PoAW-X message type.
- Impact: Pending receipts are node-local. In a multi-operator setup where different
  miners connect to different iriumd nodes, receipts are not shared. Only the specific
  node a miner POSTed to will include that receipt in `getblocktemplate`.
- Required fix (for multi-operator RC): Add a `PuzzleReceipt = 26` P2P gossip message,
  or formally document that single-operator topology is the only supported mode.

**[P-2] Unsigned P2P offer notifications (protocol.rs security TODOs)**

- File: `src/protocol.rs:52` (`OfferTakeNotification`), `src/protocol.rs:78` (`OfferBroadcast`)
- Both carry: `// TODO(security follow-up): no cryptographic signature is required`
- Impact: Third parties can spoof offer take/broadcast messages, enabling offer griefing.
- Note: Not PoAW-X specific but must be resolved before a formal security audit.
- Required fix: Add ed25519 signature over payload by the sender's wallet key.

### 5.8 LOW — Deployment / Docs

**[L-1] systemd service files for testnet not installed on VPS-1**
- Documented as PENDING in `docs/poaw-x-prototype-plan.md`.
- Templates described but no `.service` files installed.

**[L-2] DNS seed for testnet not registered**
- Listed as PENDING in `docs/poaw-x-public-testnet-network-plan.md`.

**[L-3] Public testnet block explorer not deployed**
- Planned in Phase 11-F; not implemented.

**[L-4] Testnet faucet not deployed**
- Planned in Phase 11-F; not implemented.

**[L-5] Stratum `poawx_enabled: false` by default**
- File: `pool/irium-stratum/src/stratum.rs:3852`
- Requires `IRIUM_STRATUM_POAWX=1` env var; silent if omitted.
- Recommend logging a warning when iriumd is in `active` mode but stratum PoAW-X is disabled.

---

## 6. Blockers Before Real Miner Testing

The following must be resolved before any external miner connects:

| ID | Item | Severity |
|----|------|----------|
| C-1 | irx1 not enforced in chain layer (P2P sync bypasses all PoAW-X) | CRITICAL |
| C-2 | `submit_block` pool path bypasses PoAW-X entirely | CRITICAL |
| C-3 | PoAW-X is opt-in (empty receipt list accepted without irx1) | CRITICAL |
| D-1 | Puzzle difficulty hardcoded to 1 bit (no meaningful proof of work) | HIGH |
| R-1 | No operator reward split enforcement in coinbase | HIGH |
| R-2 | Receipt persistence is in-memory only (restart destroys receipts) | HIGH |
| R-3 | No reorg handling for cleared PoAW-X receipts | HIGH |
| R-4 | `worker_pkh` is untrusted (no signature proof of ownership) | HIGH |
| R-5 | No pending pool size cap (OOM risk under receipt flood) | HIGH |
| T-1 | Zero automated PoAW-X tests (no regression safety net) | HIGH |
| O-2 | `submit_block_extended` missing mainnet guard for PoAW-X path | MEDIUM |

Acceptable for a closed internal testnet, but must close before public RC:

| ID | Item | Severity |
|----|------|----------|
| M-1 | Standard `irium-miner` binary has zero PoAW-X integration | HIGH |
| M-2 | GPU miner post-fork height TODO (`irium-miner-gpu.rs:1448`) | MEDIUM |
| P-1 | No P2P receipt gossip (multi-operator topology not supported) | MEDIUM |
| O-1 | No `/rpc/poawx/pending` operator introspection endpoint | MEDIUM |
| D-2 | Inconsistent difficulty constant (named vs raw literal) | LOW |
| P-2 | Unsigned P2P offer notifications (`protocol.rs:52,78`) | MEDIUM |
| L-1–L-5 | systemd service files, DNS seed, explorer, faucet, stratum warning | LOW |

---

## 7. Required Code Fixes

| File | Change Required |
|------|----------------|
| `src/chain.rs` (or new `src/poawx_consensus.rs`) | Add `validate_poawx_coinbase()` called from `connect_block()` when mode=active |
| `src/bin/iriumd.rs:14211` (`submit_block`) | Reject or add irx1 check when `IRIUM_POAWX_MODE=active` |
| `src/bin/iriumd.rs:13718` (`submit_block_extended`) | Reject zero-receipt submissions when mode=active |
| `src/bin/iriumd.rs:13602,13673,13785` | Extract `PUZZLE_DIFFICULTY` to shared constant; implement adaptive difficulty |
| `src/bin/iriumd.rs:13710` | Add mainnet guard for non-empty `poawx_receipts` in SBE |
| `src/bin/iriumd.rs` | Add `GET /rpc/poawx/pending` private operator endpoint |
| `src/bin/iriumd.rs` | Add receipt persistence (`state_dir()/poawx_receipts.json`) |
| `src/bin/iriumd.rs` | Add reorg receipt restoration on chain disconnect path |
| `src/bin/iriumd.rs` | Add pending pool size cap (max 1000, evict oldest) |
| `src/bin/iriumd.rs` | Add `worker_pkh` signature validation in `poawx_post_receipt` |
| `src/bin/irium-miner.rs` | Add PoAW-X puzzle solve + receipt submission loop |
| `src/bin/irium-miner-gpu.rs:1448` | Fix BIP34 height derivation (TODO fix-2a) |
| `src/protocol.rs:52,78` | Add ed25519 signatures to offer gossip messages |

---

## 8. Required Tests

| Test Function | Coverage |
|---------------|----------|
| `test_poawx_assignment_seed_derivation` | Correct seed and nonce from parent hash |
| `test_poawx_receipt_accept_valid` | Valid solution accepted, stored in pending pool |
| `test_poawx_receipt_reject_bad_nonce` | Bad `commitment_nonce` rejected 422 |
| `test_poawx_receipt_reject_insufficient_pow` | Solution with zero leading bits rejected |
| `test_sbe_accept_with_valid_irx1` | `submit_block_extended` accepts block with correct irx1 |
| `test_sbe_reject_missing_irx1` | SBE rejects block missing irx1 when receipts present |
| `test_sbe_reject_root_mismatch` | SBE rejects submitted root that does not match computed |
| `test_sbe_reject_bad_nonce_per_receipt` | SBE rejects receipt with wrong `commitment_nonce` |
| `test_sbe_reject_empty_receipts_when_active` | Empty receipt list rejected when mode=active |
| `test_poawx_mainnet_guard_assignment` | Assignment endpoint returns 503 on mainnet |
| `test_poawx_mainnet_guard_receipt` | Receipt POST returns 503 on mainnet |
| `test_poawx_mainnet_guard_sbe` | SBE with receipts returns 503 on mainnet |
| `test_receipts_root_iriumd_stratum_parity` | `compute_poawx_receipts_root` == `compute_receipts_root_from_pending` |
| `test_poawx_receipts_persist_reload` | Receipts written to disk survive simulated restart |
| `test_poawx_reorg_receipt_restore` | Cleared receipts restored on chain disconnect |
| `test_poawx_pool_size_cap` | Pool evicts oldest entries when cap exceeded |

---

## 9. Required Docs

| Document | Action |
|----------|--------|
| `docs/poaw-x-prototype-plan.md` | Update with Phase 12 findings and new blockers table |
| `docs/poaw-x-consensus-spec.md` | NEW — formal irx1 consensus rule spec (activation, validation, difficulty schedule) |
| `docs/poaw-x-reward-split-spec.md` | NEW — coinbase output split rules (miner vs operator shares) |
| `docs/poaw-x-public-testnet-runbook.md` | Update with Phase 12 deployment steps and systemd service files |
| `docs/poaw-x-public-tester-miner-guide.md` | Finalize with difficulty, `worker_pkh`, stratum env var instructions |
| `docs/poaw-x-phase12-remaining-work-audit.md` | This document |

---

## 10. Security / Privacy Risks

| Risk | Severity | Notes |
|------|----------|-------|
| irx1 not enforced at chain/consensus layer | CRITICAL | P2P-synced nodes accept any block |
| `submit_block` bypasses PoAW-X entirely | CRITICAL | Pool miners can mine without participating |
| `worker_pkh` spoofing / receipt pool flooding | HIGH | No identity proof on submitted receipts |
| In-memory receipt loss on iriumd restart | HIGH | Miner work destroyed; template corrupted |
| Pending pool unbounded (OOM risk) | HIGH | Denial of service via receipt flood |
| Puzzle difficulty trivially low (1 leading bit) | HIGH | No real work separation from random guessing |
| Operator reward share not enforced | HIGH | Financial contract unverified on-chain |
| `submit_block_extended` mainnet guard missing | MEDIUM | Crafted submissions reach PoAW-X path on mainnet |
| Offer gossip spoofing (`protocol.rs` TODOs) | MEDIUM | Off-path griefing of offers/marketplace |

No private keys, wallet secrets, RPC tokens, miner IPs, or personal information are
exposed in this report. All sensitive values have been omitted or redacted.

---

## 11. Recommended Phase 12-B through 12-G Sequence

### Phase 12-B: Critical Consensus Fixes
- Promote `irx1` validation into `chain.rs::connect_block()` (consensus layer)
- Add mainnet guard to `submit_block_extended` for non-empty receipt path
- Block or add irx1 check to `submit_block` when `IRIUM_POAWX_MODE=active`
- Enforce non-empty receipts in `submit_block_extended` when mode=active
- Add pending pool size cap (max 1000, evict oldest)
- **Gate:** All four critical blockers (C-1, C-2, C-3, O-2) closed.

### Phase 12-C: Puzzle Difficulty and Adaptation
- Extract `PUZZLE_DIFFICULTY` to a single shared constant
- Implement dynamic difficulty targeting ~1–10 seconds per solution at a reference rate
- Make difficulty configurable via env var for testnet flexibility
- Return live difficulty value in the `/poawx/assignment` response
- **Gate:** D-1 and D-2 closed.

### Phase 12-D: Receipt Integrity and Persistence
- Persist pending receipts to `state_dir()/poawx_receipts.json` on each write
- Load receipts from disk on iriumd startup
- Restore cleared receipts on chain disconnect (reorg handling)
- Add `worker_pkh` signature requirement in `poawx_post_receipt`
- Enforce operator reward split in `submit_block_extended` coinbase validation
- Add `GET /rpc/poawx/pending` private operator endpoint
- **Gate:** R-1 through R-5 and O-1 closed.

### Phase 12-E: Test Coverage
- Implement all 16 test functions listed in Section 8
- Ensure `cargo test` passes with full PoAW-X path coverage
- Add `receipts_root` parity test between iriumd and stratum implementations
- **Gate:** T-1 and T-2 closed; zero PoAW-X regressions.

### Phase 12-F: Mining Integration and Deployment
- Add PoAW-X puzzle solve + receipt submission loop to `irium-miner.rs`
- Fix GPU miner BIP34 height derivation (`irium-miner-gpu.rs:1448`)
- Install systemd service files on VPS-1 (`iriumd-testnet.service`, `irium-stratum-testnet.service`)
- Register DNS seed or publish static seed list in docs
- Deploy read-only testnet block explorer
- **Gate:** M-1, M-2, L-1, L-2, L-3 closed.

### Phase 12-G: RC Hardening and Security Audit
- Add P2P `PuzzleReceipt` gossip message type (or formally document single-operator topology)
- Resolve `protocol.rs` offer signature TODOs (P-2)
- Write `docs/poaw-x-consensus-spec.md` and `docs/poaw-x-reward-split-spec.md`
- Add stratum warning log when `IRIUM_POAWX_MODE=active` but `IRIUM_STRATUM_POAWX` is unset
- Run extended public testnet soak (≥100 blocks, ≥2 external miners)
- Conduct or commission a formal security review of all PoAW-X validation paths
- **Gate:** All blockers closed; public RC declared.

---

## 12. Final Verdict

**NOT READY FOR REAL MINERS.**

The PoAW-X protocol is architecturally incomplete at the consensus layer. The three
CRITICAL blockers alone make the current implementation incorrect:

1. **P2P-synced nodes never validate `irx1`** — `chain.rs::connect_block()` has zero
   PoAW-X awareness. Any block received over P2P is accepted unconditionally.
2. **The standard pool path (`/rpc/submit_block`) bypasses PoAW-X entirely** — a miner
   does not need to touch the `/poawx/` endpoints to successfully mine on testnet.
3. **Receipt participation is voluntary** — submitting an empty receipt list to
   `submit_block_extended` passes all PoAW-X checks and is accepted.

Additionally, the 8 HIGH severity items (puzzle difficulty trivially low, no reward
split enforcement, in-memory-only receipts, no reorg handling, no worker authentication,
no pool size cap, no PoAW-X integration in the standard miner binary, zero automated tests)
must all be resolved before a public testnet release candidate is declared.

Phases 9–11 produced a solid prototype: P2P sync works, stratum `irx1` injection is
correct, canonical `receipts_root` sorting is solid, and all negative checks pass in
isolated testing. That foundation is intact on this branch. Phase 12-B through 12-G
must close the blockers above before a real external miner connects to the testnet.
