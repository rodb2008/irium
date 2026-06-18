# PoAW-X Phase 21 — Blueprint Gap Closure (21A foundation primitives; 21B gated ticket/penalty enforcement; 21C persistent reorg-safe anti-domination enforcement; 21D candidate-set + VRF-style assignment-proof foundation; 21E mandatory candidate admission/gossip)

**Status: Phase 21A implemented the FOUNDATION primitives (data-only); Phase 21B now wires ticket
+ penalty enforcement into the Phase 20 consensus/pool/wallet path behind explicit testnet/devnet
gates (mainnet hard-off; old behavior unchanged when gates off).** Public testnet with outside
miners, independent security audit, and community vote remain **out of scope**.

**Phase 21C (this pass)** makes anti-domination **persistent + reorg-safe + enforced**: a per-(miner,
window) reward-bucket state in `ChainState` (exact apply/revert, deterministic rebuild-from-chain),
updated in connect_block / reverted in disconnect_tip_block from Phase 20 reward events (fee +
delegate never credited; official fee-0 and third-party fee yield identical role amounts); an optional
trailing `DOM1` ext section (byte-identical when absent) carrying 4 per-role fairness weights that the
node re-validates against its persisted state when
`IRIUM_POAWX_ANTI_DOMINATION_ACTIVATION_HEIGHT`+`_REQUIRED=1` (mainnet hard-off, fail-closed); pool
mirror + fairness selection among collected candidates. Global-best-among-unseen-candidate selection
is **Phase 21D pending** (candidate-set/VRF), not faked. Details:
`poaw-x-phase21c-anti-domination-enforcement.md`.

**Phase 21D (this pass)** adds the candidate-set + assignment-proof foundation: a new
`src/poawx_candidate.rs` with `AssignmentProofV1` (a documented **VRF-style placeholder**
— no VRF lib in-repo — domain-separated, public-key-bound, hash-based, deterministic,
no private key), a canonical `CandidateSet` (sorted/dedup, stable root, deterministic
effective-score + tie-break), an optional trailing `CND1` ext section (byte-identical
when absent), node `validate_block_candidate_sets` (set present/canonical/bound, each
candidate self-consistent, dominance weights vs persisted state when active, and the
selected role solver = the BEST candidate within the included set) behind
`IRIUM_POAWX_CANDIDATE_SET_{ACTIVATION_HEIGHT,REQUIRED}` /
`IRIUM_POAWX_ASSIGNMENT_PROOF_{ACTIVATION_HEIGHT,REQUIRED}` (mainnet hard-off,
fail-closed), pool candidate-set production + byte-identical mirror, and a wallet
`poawx-assignment-proof` emitter (no key, testnet-only). **HONEST LIMITATION:** the node
validates best WITHIN the included candidate set; it cannot prove unseen miners did not
exist (no mandatory candidate admission/gossip yet), and the proof is a placeholder not
a true VRF. Details: `poaw-x-phase21d-candidate-set-assignment.md`.

**Phase 21E (this pass)** makes candidate admission/gossip mandatory when gated: a new
`src/poawx_admission.rs` (`CandidateAdmissionV1` + process-global node cache: validate →
window → dedupe → store, deterministic admitted-set root), a `PoawxCandidateAdmission`
P2P type + receive-loop ingest/rebroadcast, loopback RPC (`POST /poawx/candidate-admission`,
`GET /poawx/candidate-admissions`), node enforcement that a block's candidate set EQUALS
the node-admitted set for the height/seed (missing or extra candidate rejects; selected =
best among ADMITTED; fail-closed when none), pool build-from-admitted (fail-closed if the
admitted cache is unavailable) + a wallet `poawx-candidate-admission` emitter (no key,
testnet-only). Gates `IRIUM_POAWX_CANDIDATE_ADMISSION_{ACTIVATION_HEIGHT,REQUIRED,WINDOW}`,
mainnet hard-off. **HONEST LIMITATION:** best among candidates ADMITTED TO THIS NODE in the
window (propagation-sensitive, testnet/devnet) — still NOT proof that unseen offline miners
did not exist, and the assignment proof remains a VRF-style placeholder. Details:
`poaw-x-phase21e-candidate-admission-gossip.md`.

## Phase 21B — gated enforcement (this pass)
- **Ticket proof binding (`src/poawx_ticket.rs` + `src/poawx.rs`):** a compact, self-verifiable
  `TicketProof` (176-byte wire) binds network/height/role/miner-pkh, carries the sybil-work
  (nonce + digest, independently recomputable + threshold-checkable) and a deterministic
  `ticket_digest`. `Phase20ReceiptExt` gains an OPTIONAL trailing `role_ticket_proofs: [3]` section
  (magic-prefixed, present-only) — **byte-identical to pre-21B when absent**, so all existing
  Phase 20 exts/blocks/tests stay valid. The ticket proofs are bound into the ext digest →
  receipts-root → irx1 commitment automatically.
- **Node consensus enforcement (`src/chain.rs`):** `validate_phase20_production_block` calls
  `validate_phase20_ticket_proofs` only when `poawx_ticket::tickets_enforced(height)`
  (`IRIUM_POAWX_TICKETS_ACTIVATION_HEIGHT` + `IRIUM_POAWX_TICKETS_REQUIRED=1`, mainnet-off). It
  requires every rewarded role to carry a valid proof bound to the role solver pkh; rejects
  missing/malformed/wrong-network/expired/future/wrong-pkh/wrong-role/bad-sybil/bad-digest. When
  `poawx_penalty::penalty_state_enforced(height)` (`…PENALTY_STATE_ACTIVATION_HEIGHT` +
  `…PENALTY_STATE_REQUIRED=1`), suspended/slashed identities are rejected from **high-trust roles**
  (VERIFY/SUPPORT); expired penalties no longer reject. **Gate off ⇒ ticket proofs ignored
  (old Phase 20 behavior unchanged).**
- **Pool production (`pool/irium-stratum/src/delegation.rs`):** a byte-identical `TicketProofMirror`
  + `Phase20ReceiptExtMirror.role_ticket_proofs`; `build_collected_phase20_ext` /
  `build_synthetic_phase20_ext` attach per-role proofs when `pool_tickets_enforced(height)`; a
  parity test proves the pool ext deserializes via the node lib and the node validator accepts each
  proof. Official fee-0 and third-party fee paths both carry tickets when enforced. If a required
  input is missing the builder returns None ⇒ the node fails closed.
- **Wallet (`src/bin/irium-wallet.rs`):** `poawx-ticket-proof` emits a ticket-proof JSON bound to
  role/height (testnet/devnet only, mainnet rejected, **no private key**, emit-only).
- **Dominance: still data-only (deterministic helper + Phase 20 reward-event feed, tested). Full
  persistent + reorg-safe dominance enforcement is deferred to Phase 21C** (not faked): it requires
  persistent chain state + reorg handling beyond this step.
- **Tests:** node `ticket_proof_roundtrip_and_validate`, `phase20_ext_ticket_section_roundtrip_backward_compatible`,
  chain `phase21b_ticket_penalty_enforcement` (gate off accepts; on rejects missing; valid accepts;
  expired rejects; penalty suspended high-trust rejects; penalty-off accepts); pool
  `phase21b_pool_ticket_mirror_and_ext_parity` + `phase21b_pool_tickets_enforced_gate`; wallet
  ticket emit. All Phase 20 / hidden-precommit / role-gossip tests still green.

---

**Phase 21A (foundation) — original status:** primitives + tests, data-only and gated. Mainnet
remains **hard-off** for all PoAW-X gates.

> **PoAW-X is consensus / network-level, not pool-owned.** The pool/stratum is only **one miner
> interface** to PoAW-X; it is not the owner or authority of the protocol. These Phase 21
> primitives live in the **node library** (`src/poawx_*`), are network/consensus-oriented, and
> are gated independently of any pool. A pool is just one way a miner participates.

## Phase 21A modules (new, in `src/`)
- `src/poawx_ticket.rs` — **Miner Work Ticket** + lightweight **Sybil-work** primitive
- `src/poawx_dominance.rs` — **anti-domination** recent-reward tracker + fairness weight
- `src/poawx_adaptive.rs` — **adaptive mining/security mode** state machine
- `src/poawx_penalty.rs` — **penalty / fraud state** primitive
- `src/lib.rs` — module declarations

All are `#![allow(dead_code)]` foundation, deterministic, fixed-point only (no floats in any
consensus-relevant math), saturating arithmetic, and **mainnet hard-off** via
`crate::activation::network_id_byte() == 0`.

## What each primitive provides

### Miner Work Ticket (`poawx_ticket`)
Per-epoch, network-bound identity/eligibility token: `version, network_id, miner_pkh, epoch,
assignment_public_key` (VRF/assignment placeholder), `sybil_work_nonce, sybil_work_digest,
recent_reward_score, valid_work_count, invalid_work_count, penalty_status, bond_reference
(optional), issued_height, expiry_height`. Canonical byte serialization + stable `digest()`.
`validate()` rejects: wrong network (mainnet hard-off), expired, future-issued, malformed,
bad penalty status, sybil-digest-mismatch, and (when the threshold is enabled) insufficient
sybil work. Penalized tickets are not eligible for high-trust roles.

### Sybil-resistance (`poawx_ticket`)
A cheap identity cost: the ticket's `sybil_work_digest` must meet a configurable leading-zero-bit
target (`IRIUM_POAWX_TICKET_SYBIL_BITS`, default **0 = off**, mainnet always 0). This is an
identity/eligibility cost only — it is **NOT chain PoW and does NOT touch LWMA-144**. A test
helper grinds a nonce for tiny targets.

### Anti-domination recent reward (`poawx_dominance`)
Bounded per-miner reward history (primary/compute/verify/support buckets + total, window id,
last height) with window/epoch reset. Deterministic fixed-point fairness weight:
`fairness_weight = valid_work_score * 1000 / (1000 + recent_reward_share_permille)`. A Phase 20
multi-role reward event (RoleReward pkhs + the 55/22/13/10 split) can feed the tracker (tested).

### Adaptive modes (`poawx_adaptive`)
Deterministic `Normal / Caution / Defense / Recovery` state machine from observed signals
(active miner count, valid role count, recent invalid work, reorg signal, reward concentration,
finality availability) → policy (confirmation multiplier, stricter verification, require-ticket,
require-finality placeholder, role fallback). **No hardware-class assumptions** (no CPU/GPU/ASIC
anywhere). The chain continues with ≥1 valid miner; low participation → Caution (not halt);
instability → Defense; clearing → Recovery → Normal. Zero miners → `can_produce_block() == false`.

### Penalty / fraud state (`poawx_penalty`)
`Clean / Warned / TemporarilyReduced / SuspendedForEpoch / SlashedPlaceholder` with eligibility +
fixed-point weight multiplier; per-miner record escalates on invalid work and expires
suspensions. **`SlashedPlaceholder` is a placeholder only** — no economic slashing.

## Integration with Phase 20 (minimal, safe)
- Primitives are exposed from the node lib; helper functions are available for Phase 20 to call
  later. A test proves Phase 20 role-reward events feed the dominance tracker.
- **Data-only this phase:** no change to live block acceptance, no ticket required by existing
  tests, no change to Phase 20 E2E behavior. Optional enforcement (validating tickets in the
  role-claim/receipt path, or applying dominance/penalty weights to assignment) is **Phase 21B**,
  to be added under its own activation gate.

## Mainnet safety / gates (all default off, mainnet hard-off)
- `IRIUM_POAWX_TICKETS_ACTIVATION_HEIGHT`, `IRIUM_POAWX_TICKETS_REQUIRED=1`,
  `IRIUM_POAWX_TICKET_SYBIL_BITS`
- `IRIUM_POAWX_ANTI_DOMINATION_ACTIVATION_HEIGHT`
- `IRIUM_POAWX_ADAPTIVE_MODE_ACTIVATION_HEIGHT`
- `IRIUM_POAWX_PENALTY_STATE_ACTIVATION_HEIGHT`

Each `*_active(height)` returns false on mainnet regardless of env; existing Phase 20 tests are
unaffected. Chain difficulty remains **LWMA-144 automatic**.

## Blueprint gaps — status after 21A
| Blueprint item | Status |
|---|---|
| Miner Work Tickets | **PARTIAL** — primitive + sybil-work + validation + tests (data-only; enforcement = 21B) |
| Stronger Sybil resistance | **PARTIAL** — ticket sybil-work cost primitive (gated, off by default) |
| Anti-domination / recent reward history | **21C** — persistent + reorg-safe per-(miner,window) state, connect/disconnect/reorg/restart, gated ext weight binding + node validation, pool selection weighting (global-best candidate optimality = 21D) |
| Adaptive mining/security modes | **PARTIAL** — deterministic mode/policy state machine (not yet consumed by node) |
| Penalty / fraud state | **PARTIAL** — status enum + record/escalation/expiry (slashing placeholder only) |
| Fuller private assignment / VRF eligibility | **21D** — candidate-set commitment + AssignmentProofV1 (VRF-STYLE PLACEHOLDER, not true VRF) + node best-within-included-set validation; true VRF = future; mandatory candidate admission/gossip = **21E** |
| Puzzle work-mode primitives beyond simplified role path | **PENDING** |
| Stronger finality-committee integration with the 10% role | **PENDING** — `require_finality` is a placeholder flag |

## Remaining next technical steps (Phase 21B+)
1. VRF / private-assignment integration (replace the `assignment_public_key` placeholder with a
   real VRF eligibility proof).
2. Puzzle work-mode integration (richer puzzle lanes beyond the current simplified role path).
3. Full finality-committee reward + signature integration with the SUPPORT/finality 10% role.
4. Gated enforcement (21B): ticket validation in the role-claim/receipt path; apply dominance +
   penalty weights to role assignment.

**Excluded (not in this track):** public testnet with outside miners, independent security
audit, community vote, mainnet activation.
