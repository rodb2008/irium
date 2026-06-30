# PoAW-X consensus review targets

All in `src/chain.rs`, `src/poawx*.rs`, `src/block.rs`. Validation is node-authoritative and
fail-closed.

## Ext serialization / deserialization — `src/poawx.rs` (`Phase20ReceiptExt`)

- Magic-dispatch trailing sections (present-only; byte-identical + same digest when absent):
  `TPK1`, `DOM1`, `CND1`, `PZL1`, `FIN1`, `CAC1`, `AVR2`.
- Review: exact/bounded lengths; `need()` guards; rejection of unknown magic, duplicate
  sections, truncated/oversized payloads; that "absent" is byte-identical to the pre-section
  wire (backward compatibility); that mutation changes the ext digest.
- Tests: `phase20_receipt_ext_wire_roundtrip`, `phase2*_ext_*_roundtrip_backward_compatible`,
  `phase23a_ext_rejects_malformed_avr2_section`.

## Receipts root / irx1 commitment

- Review: the ext digest feeds the receipts-root and the irx1 block commitment; any section
  change must change the committed root; gate-off blocks are byte-identical to legacy.

## connect_block enforcement — `src/chain.rs`

Hooks (each gated, fail-closed):
`validate_block_dominance_weights`, `validate_block_candidate_sets`,
`validate_block_puzzle_proofs`, `validate_block_finality`,
`validate_block_committed_admission`, `validate_block_true_vrf`.
- Review: ordering, that each is only enforced under its gate, that the selected role solver is
  the best candidate, and that no path can be satisfied by a weaker/forged section.

## Candidate set enforcement — `src/poawx_candidate.rs` + `src/chain.rs`

- `validate_self` (pure V1 placeholder digest recompute) vs `validate_scoring` (penalty +
  effective-score only) — under the true-VRF gate the consensus caller uses `validate_scoring`
  because the candidate digest is the VRF output (verified via the AVR2 proof). Review this
  reconciliation carefully (it is the key Phase 22E design point).
- Under admission enforcement, the block candidate set must EQUAL the node's admitted set.

## Committed admission root — `src/poawx_committed_admission.rs`

- `AdmissionCommitmentV1::from_candidate_set` commits the next height's admitted-set root in
  block H-1; freeze seed = grandparent hash (no circularity). Review reorg-safety and that the
  root binds the candidate digests (and thus the VRF outputs under V2).

## Dominance state / reorg — `src/poawx_dominance.rs` + `src/chain.rs`

- Persistent per-(miner,window) buckets; apply on connect, revert on disconnect; validated vs
  PERSISTED parent state. Review exact apply/revert symmetry and restart-replay correctness.

## Penalty / finality / puzzle gates

- Penalty: `src/poawx_penalty.rs`. Finality: `src/poawx_finality.rs` (member-signed votes,
  threshold, finalize parent). Puzzle: `src/poawx_puzzle.rs` (assigned-work, NOT chain
  difficulty/LWMA — confirm no LWMA/difficulty/target interaction).

## Mainnet hard-off gates

- Every `*_gate`/`*_enforced` returns false when `network_id == 0`; default-off; activation
  needs network≠0 + activation height + `*_REQUIRED=1`. Confirm no path bypasses the hard-off.

## Backward compatibility & malformed handling

- Gate-off ⇒ legacy/V1 behavior, byte-identical wire. Malformed/oversized payloads are rejected
  with bounded deserialization (no panics; `Result` errors). Confirm no `unwrap`/`expect` on
  attacker-controlled input in non-test consensus paths.

## Determinism

- Integer/fixed-point only (no floats in consensus; the lone `f64` is a `#[cfg(test)]`
  distribution test). Canonical ordering via `BTreeMap`/`BTreeSet` + explicit sorts. No
  wall-clock in consensus.
