# PoAW-X threat model

For each attacker: expected protection, relevant files/functions, covering tests, and open
limitations. All protections are **mainnet hard-off** (inactive when `network_id == 0`) and
default-off; they apply on testnet/devnet when gated on.

## Malicious block producer
- **Protection:** the node re-validates every gated ext section in `connect_block`
  (`validate_block_candidate_sets`, `…_dominance_weights`, `…_puzzle_proofs`, `…_finality`,
  `…_committed_admission`, `…_true_vrf`); fail-closed. The producer cannot weaken thresholds or
  substitute a non-best candidate.
- **Files:** `src/chain.rs`, `src/poawx.rs`.
- **Tests:** `chain::phase21*`, `phase22a_*`, `phase22d_true_vrf_enforcement`,
  `phase22e_true_vrf_e2e_block`, `phase22e_wrong_candidate_score_rejects`.
- **Limitation:** "best among candidates ADMITTED to this node in the window" (see admission).

## Malicious pool
- **Protection:** the pool holds NO VRF secret and never proves; it only bundles miner-supplied
  proofs and fails closed if a selected role lacks one. The node re-verifies everything; the
  pool cannot bypass validation.
- **Files:** `pool/irium-stratum/src/delegation.rs` (`build_pool_true_vrf_section`,
  `build_synthetic_phase20_ext`, `build_collected_phase20_ext`), `src/chain.rs`.
- **Tests:** `delegation::phase22e_pool_e2e_bundle_and_failclosed`,
  `phase22d_pool_true_vrf_parity_and_failclosed`.

## Malicious miner / Sybil identities
- **Protection:** tickets + Sybil-work binding; penalty status; per-(miner,window) dominance
  weighting; candidate admission requires self-consistent, bound candidates.
- **Files:** `src/poawx_ticket.rs`, `src/poawx_penalty.rs`, `src/poawx_dominance.rs`,
  `src/poawx_admission.rs`.
- **Tests:** `poawx_ticket`, `poawx_penalty`, `poawx_dominance`, `poawx_admission`.
- **Limitation:** Sybil resistance is work/ticket-based, not identity-based; public-network
  tuning pending.

## Reward domination attempt
- **Protection:** persistent, reorg-safe per-(miner,window) dominance state; fairness weight
  reduces a dominating miner's effective score; validated vs PERSISTED parent state.
- **Files:** `src/poawx_dominance.rs`, `src/chain.rs` (apply/revert on connect/disconnect).
- **Tests:** `poawx_dominance` (12), `chain::phase21c_*` (reorg + enforcement).

## Replay / substitution of a VRF proof
- **Protection:** the VRF message binds network_id, target_height, role_id, solver_pkh,
  ticket_digest, seed, assignment_public_key; verification fails for any other context.
- **Files:** `src/poawx_candidate.rs` (`vrf_message`, `AssignmentProofV2::validate`).
- **Tests:** `assignment_v2_prove_verify_and_rejects`, `phase22e_admission_v2_accept_and_reject`.

## Candidate omission / addition
- **Protection:** under admission enforcement the block's candidate set must EQUAL the node's
  admitted set for (height, seed); chain-committed admission commits the next height's admitted
  root in block H-1.
- **Files:** `src/poawx_admission.rs`, `src/poawx_committed_admission.rs`, `src/chain.rs`.
- **Tests:** `chain::phase21e_admission_enforcement`, `phase22a_committed_admission_enforcement`.
- **Limitation:** cannot prove unseen/offline/never-gossiped miners were absent (documented).

## Malformed gossip / wire payloads (DoS / parser abuse)
- **Protection:** bounded deserialization — fixed `*_WIRE` sizes, `*_MAX_BYTES`/`*_CAP` caps,
  exact-length checks, magic-dispatch with explicit errors; dedupe + window in caches.
- **Files:** all `*_admission.rs`/`*_finality.rs`/`poawx.rs` deserialize paths.
- **Tests:** `phase23a_*` malformed-input tests; `cache_ingest_dedupe_window_and_root`.

## Finality vote forgery
- **Protection:** member-signed secp256k1 votes; the node re-verifies every vote + committee
  membership + threshold; the pool only bundles collected votes.
- **Files:** `src/poawx_finality.rs`, `src/chain.rs` (`validate_block_finality`).
- **Tests:** `poawx_finality`, `chain::phase21h_finality_enforcement`.

## Fee manipulation
- **Protection:** official path is fee-0; third-party fee only in explicit pool mode, capped at
  `THIRD_PARTY_FEE_CAP_BPS` (200 = 2.00%); invalid fee terms fail closed to 0%.
- **Files:** `src/poawx.rs` (`apply_fee`), `pool/irium-stratum/src/delegation.rs`.
- **Tests:** `reward`, `phase20` (pool), `native_rewardable`.

## Private key leakage
- **Protection:** wallet emit helpers take secrets as input only; never echoed/logged/in JSON
  or error strings; submit posts only public wire.
- **Files:** `src/bin/irium-wallet.rs`.
- **Tests:** `phase22d_assignment_proof_v2_emit_no_secret_mainnet_off`,
  `phase22e_candidate_admission_v2_emit_and_submit`, `phase21*_emit_*_no_secret_*`.

## Invalid puzzle proof
- **Protection:** deterministic assigned-work challenge recomputed from the selected candidate
  + fast bounded verification; does NOT affect chain difficulty/LWMA.
- **Files:** `src/poawx_puzzle.rs`, `src/chain.rs` (`validate_block_puzzle_proofs`).
- **Tests:** `poawx_puzzle`, `chain::phase21f_puzzle_enforcement`.

## Reorg / admission-root mismatch
- **Protection:** dominance state apply/revert on connect/disconnect; committed-admission seed
  uses the grandparent hash (no circularity); block-data-only validation is reorg-safe.
- **Files:** `src/chain.rs`, `src/poawx_committed_admission.rs`.
- **Tests:** `chain::phase21c_*` (reorg), `phase22a_*`.

## Accidental mainnet activation
- **Protection:** `network_id == 0` hard-off in every gate (44 guards); activation needs a
  non-zero network + an activation height + a `*_REQUIRED=1` flag (three independent
  conditions); all default off.
- **Files:** every `*_gate`/`*_enforced` function.
- **Tests:** `*_gate(0, …) == false` / `mainnet hard-off` assertions in every module.
