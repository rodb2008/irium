# PoAW-X Phase 21D — Candidate-set + VRF/private-assignment foundation

**Status:** Implemented (gated, testnet/devnet only, **mainnet hard-off**, default
off; old behavior byte-identical when off). Local-only; not pushed. Builds on Phase
21A–21C. PoAW-X is **consensus/network-level**; the pool is one miner interface, not
the owner — the node re-validates everything.

## AssignmentProofV1 — VRF-style placeholder (NOT a final cryptographic VRF)

The repo ships **no VRF library**, so `AssignmentProofV1` (`src/poawx_candidate.rs`)
is an explicit, documented **VRF-style placeholder**: a domain-separated
(`IRIUM_POAWX_ASSIGNMENT_PROOF_V1`), **public-key-bound**, hash-based digest over
`network_id ‖ target_height ‖ role_id ‖ solver_pkh ‖ assignment_public_key ‖
ticket_digest ‖ seed (prev_hash)`. The `assignment_score` is the first 8 digest bytes
(LE). It is **deterministic and independently recomputable** by every node — it is
**NOT unpredictable-before-reveal** the way a true VRF output is, and requires **no
miner private key**. Replacing it with a real cryptographic VRF is future work.
`validate()` rejects wrong network/height/role/ticket/seed and any digest mutation.

## Candidate set (`RoleCandidate` / `CandidateSet`)

Each `RoleCandidate` carries: role_id, solver_pkh, assignment_public_key,
ticket_digest, penalty_status, assignment_proof_digest, dominance_weight,
penalty_weight, effective_score, role_claim_digest. A `CandidateSet` has a
`(network_id, target_height, seed)` header + a **canonical, deduplicated, sorted**
candidate list (sort key: role_id, solver_pkh, ticket_digest,
assignment_proof_digest). Canonical rules: deterministic ordering, fixed-point math
only (no floats), stable `root()` over the canonical serialization (any mutation or
reorder changes the root), bound to network/height/role/miner/ticket/dominance.

## Effective score (deterministic, HIGHER WINS)

`effective_score = assignment_score × dominance_weight × penalty_weight /
1_000_000` (fixed-point u128, saturating). Inputs: the assignment/proof score, the
Phase 21C dominance fairness weight, and the penalty weight (permille from
`PenaltyStatus::weight_multiplier_permille`). Suspended/slashed ⇒ penalty_weight 0 ⇒
**score 0** (a suspended candidate has zero weight and is never selected unless all
are zero). Deterministic tie-break: **(1) effective_score, (2) assignment_proof_digest,
(3) solver_pkh, (4) ticket_digest** (higher score wins; ties resolved by the smaller
of each subsequent field). No hardware-class or pool-ownership assumptions.

## Candidate-set commitment in the ext

`Phase20ReceiptExt` gains an OPTIONAL trailing **CND1** section (magic + u32 length +
canonical candidate-set bytes) carrying the full set so the node can self-validate
selection. **Byte-identical to pre-21D when absent.** The deserialize magic-dispatch
loop now handles `TPK1` (ticket) / `DOM1` (dominance) / `CND1` (candidate) in any
order, strict on unknown/truncated trailing data. When present it is bound into the
ext digest → receipts root → irx1 commitment automatically.

## Node validation (gated)

When `candidate_set_enforced(height)` (`IRIUM_POAWX_CANDIDATE_SET_ACTIVATION_HEIGHT` +
`IRIUM_POAWX_CANDIDATE_SET_REQUIRED=1`, mainnet hard-off), `connect_block` calls
`validate_block_candidate_sets`:

- the candidate set must be **present** (else reject), bound to `(network, height,
  parent prev_hash seed)`, and **canonical** (sorted, deduped);
- every candidate must be **self-consistent** — recomputed assignment-proof digest +
  penalty weight + effective score must match the stored values;
- each candidate's `dominance_weight` must equal the **node's persisted-state weight**
  (Phase 21C) when dominance is active;
- each **selected** role solver (`role_reward.{compute,verify,support}`) must be the
  **BEST candidate for that role** under the deterministic effective-score ordering.
- Mismatch / missing / malformed ⇒ **reject (fail closed)**.

Penalty/ticket enforcement compose with the existing Phase 21B gates running in the
same `connect_block`: candidate penalty weight is validated in-set (suspended ⇒ 0),
and the selected role tickets are validated by the 21B ticket path.

### HONEST LIMITATION (still open)

The node can only validate the **included** candidate set. It cannot prove that
**unseen** miners did not exist, because there is **no mandatory candidate admission /
gossip rule** yet. So this enforces **best-within-the-included-set**, NOT
**global-network-best**. Making candidate admission mandatory (so the included set
provably covers all eligible miners) — and replacing the placeholder with a real VRF —
remains future work.

## Pool production (one interface, not the owner)

`pool/irium-stratum` mirrors `poawx_candidate` byte-for-byte (assignment digest/score,
candidate wire, set root, best-for-role, effective score, penalty weight).
`build_pool_candidate_set` builds one candidate per selected fairness role from the
role solvers + per-role ticket digests + the process-global dominance view (penalty
Clean), seed = block prev_hash, sorted canonical. `build_synthetic`/`build_collected`
attach it when `pool_candidate_set_enforced(height)` (else None ⇒ the node fails
closed); `build_collected` threads the block `prev_hash` from the stratum job. Official
fee-0 and third-party fee paths both carry the candidate set when enforced; synthetic
fallback only when explicitly enabled. The node re-validates the entire set, so a
diverging pool is simply rejected.

## Wallet helper

`irium-wallet poawx-assignment-proof --network-id <id> --target-height <H> --role
<compute|verify|support> --solver <addr|40hex> --ticket-digest <64hex> --seed <64hex>
[--assignment-pubkey <66hex>]` emits an `AssignmentProofV1` JSON (assignment_proof
fields + assignment_score + assignment_proof_digest). **No private key / no seed
phrase**; testnet/devnet only; mainnet hard-off (network_id 0 rejected). The output
digest matches the node-lib recomputation (placeholder is deterministic).

## Gates (all default off, mainnet hard-off)

- `IRIUM_POAWX_CANDIDATE_SET_ACTIVATION_HEIGHT`, `IRIUM_POAWX_CANDIDATE_SET_REQUIRED=1`
- `IRIUM_POAWX_ASSIGNMENT_PROOF_ACTIVATION_HEIGHT`, `IRIUM_POAWX_ASSIGNMENT_PROOF_REQUIRED=1`

Each gate returns false on mainnet (`network_id == 0`) regardless of env; old Phase 20
behavior is unchanged when off. Chain difficulty remains **LWMA-144 automatic** —
candidate-set/assignment never touches PoW.

## Tests

- `poawx_candidate`: assignment-proof digest determinism/sensitivity, validate
  accept/reject (net/height/role/ticket/seed/mutation), effective-score rules,
  candidate wire round-trip, set root + mutation, best-for-role + tie-break, gate
  logic.
- `chain`: `phase21d_candidate_set_enforcement` (accept best, reject non-best/missing/
  mutated/wrong-seed, mainnet hard-off).
- `poawx`: `phase21d_ext_candidate_section_roundtrip` (absent byte-identical, present,
  mutation changes digest, candidate+dominance both).
- pool: `phase21d_pool_candidate_set_parity` (wire/root/best-for-role parity with the
  node lib, assignment-digest parity, full-ext CND1 round-trip, byte-identical off,
  mainnet hard-off).
- wallet: `phase21d_assignment_proof_emit_json_no_secret_mainnet_off`.

## Remaining technical steps (excluded here)

- **True cryptographic VRF** to replace `AssignmentProofV1` (placeholder).
- **Mandatory candidate admission / gossip** so the included set provably covers all
  eligible miners (global-network-best, not just best-within-included-set).
- Puzzle work-mode primitives beyond the simplified role path.
- Finality-committee integration with the SUPPORT/finality role.
- **Excluded (not in this track):** public testnet with outside miners, independent
  security audit, community vote, mainnet activation.
