# PoAW-X Phase 26A — seed reconciliation design (phase21d ↔ phase22a multi-block blocker)

**Status: DESIGN ONLY. No consensus code was changed in Phase 26A. No live nodes were used.**
This document audits the contradiction found in Phase 25C, states the intended invariant,
evaluates three reconciliation options, and recommends a safe, minimal, gate-preserving fix that
makes a multi-block all-gates chain satisfiable. Implementation is deferred and requires explicit
approval (see §9).

Branch `testnet/poawx-phase20-blueprint-completion-local`. Audited at HEAD `bf00b25`.
Production-ready: **NO**. Mainnet-ready: **NO**. Audited: **NO**.

---

## 1. The exact contradiction

For a block at height `H`, `connect_block` runs the PoAW-X gates in this order
(`src/chain.rs:851` `connect_block`):

1. `validate_block_header`
2. `validate_poawx_coinbase`
3. `validate_poawx_block_receipts`
4. `validate_block_dominance_weights` (phase21c) — if anti-domination enforced
5. **`validate_block_candidate_sets` (phase21d/21e)** — `src/chain.rs:862`
6. `validate_block_puzzle_proofs` (phase21f)
7. `validate_block_finality` (phase21h)
8. **`validate_block_committed_admission` (phase22a)** — `src/chain.rs:870`
9. `validate_block_true_vrf` (phase22d)

Both gate 5 and gate 8 constrain the SAME object — the receipt's `candidate_set` — but disagree on
its `seed`:

- **phase21d** (`src/chain.rs:1363`):
  ```rust
  if cs.seed != block.header.prev_hash { return Err("phase21d: candidate set wrong seed"); }
  ```
  ⇒ `candidate_set.seed == block.header.prev_hash`. For height `H` that is `hash(H-1)`.

- **phase22a** (`src/chain.rs:1175` `validate_block_committed_admission`):
  - Outgoing self-consistency (`src/chain.rs:1199`): a block's own commitment must satisfy
    `ca.seed == block.header.prev_hash`, `ca.commit_height == H`, `ca.target_height == H+1`.
  - Incoming check for block `H`: the parent's commitment `pc` (made in block `H-1`, with
    `target_height == H`) must satisfy `pc.seed == prev.header.prev_hash` (`src/chain.rs:1224`),
    and `pc.matches_candidate_set(cs)` (`src/chain.rs:1238`).
  - `matches_candidate_set` (`src/poawx_committed_admission.rs`) requires, among other fields,
    `cs.seed == pc.seed`.
  - Since `pc` was frozen in block `H-1` with `pc.seed == (block H-1).prev_hash == hash(H-2)`,
    this forces `candidate_set.seed == hash(H-2)`.

So for any `H ≥ 2`:

| Gate | requires `candidate_set.seed ==` | value for H2 |
|------|----------------------------------|--------------|
| phase21d | `block.header.prev_hash` = `hash(H-1)` | `hash(H1)` |
| phase22a | `parent_commitment.seed` = `hash(H-2)` | `genesis` |

`hash(H-1) ≠ hash(H-2)` always ⇒ **no candidate set can satisfy both** for `H ≥ 2`.

`H1` is the only satisfiable block: its incoming committed-admission check is **graced** at the
activation height (`src/chain.rs` "activation-height grace": `parent_commit == None` and
`is_activation` ⇒ allowed), so phase22a imposes no seed constraint on `H1`, and phase21d's
`cs.seed == genesis` holds. This is why every prior phase (24K / 24L / 25B / 25C) produced exactly
one block.

Phase 25C observed this live: H2 **passed** phase21d (seed = `hash(H1)`) then **failed** phase22a
with `phase22a: candidate set does not match committed admission root` (parent commitment seed =
`genesis`).

### Why this is structural, not a harness bug

Committing the height-`H` candidate set **one block ahead** (inside block `H-1`) is only possible if
that set is determined by a seed **known when block `H-1` is produced**. The candidate set is a
function of `(network, height, seed)` (`CandidateSet::new`, `src/poawx_candidate.rs:408`). The latest
seed known at `H-1`'s production is `(block H-1).prev_hash == hash(H-2)` (the grandparent of `H`).
A set seeded by `hash(H-1)` (the immediate parent that phase21d demands) **cannot exist until block
`H-1` is finalized**, so block `H-1` cannot commit it — committing it would require `H-1` to know
its own hash (circular). The current code therefore overloads `candidate_set.seed` with two
incompatible meanings:

- phase21d treats it as **"the immediate parent / tip hash at this height"** (`hash(H-1)`).
- phase22a's commit-ahead treats it as **"the freeze seed knowable one block early"** (`hash(H-2)`).

---

## 2. Code-path references (audit)

| Concern | Location |
|---|---|
| candidate-set construction (devnet/test builder) | `src/poawx_mining_harness.rs:189` (`seed = prev_hash`), `:205–231` (candidates, admissions, AVR2), `:289–296` (outgoing `cs2` + commitment) |
| `CandidateSet` type / seed field / root | `src/poawx_candidate.rs:402` (`seed`), `:408` (`new(net, target_height, seed)`), `:488` (`root()` = `sha256(DOMAIN ‖ serialize())`) |
| phase21d/21e validation | `src/chain.rs:1337` `validate_block_candidate_sets`; seed check `:1363`; candidate self-check `cand.validate_self(net, height, &cs.seed)`; phase21e admitted-set lookup keyed on `block.header.prev_hash` (`cache.admitted_candidate_set(net, height, &block.header.prev_hash)`) and `cs.serialize() == admitted.serialize()` |
| committed-admission commitment construction | `src/poawx_committed_admission.rs:109` `from_candidate_set(cs, commit_height)` (captures `cs.seed`, `cs.root()`, `cs.target_height`, count, window_id) |
| phase22a validation | `src/chain.rs:1175` `validate_block_committed_admission`; outgoing `ca.seed == block.header.prev_hash` `:1199`; incoming `pc.seed == prev.header.prev_hash` `:1224`; `pc.matches_candidate_set(cs)` `:1238` |
| `matches_candidate_set` | `src/poawx_committed_admission.rs` — requires `cs.root()==root && count match && cs.seed==self.seed && net match && target_height match` |
| where `candidate_set.seed` is created | builder `src/poawx_mining_harness.rs:189` (`= prev_hash`); validated against `block.header.prev_hash` at `src/chain.rs:1363` |
| where `parent_commitment.seed` is created | builder `src/poawx_mining_harness.rs:289–296` (commits `cs2` for `H+1`); the node pins it to the committer's `prev_hash` at `src/chain.rs:1199` / `:1224` |
| true-VRF binding to the seed | `src/chain.rs` `validate_block_true_vrf`: `pr.seed != cs.seed` ⇒ AVR2 proof binds to `cs.seed` (follows whatever the candidate-set seed is) |
| admission wire binding | `src/poawx_admission.rs:100–128` — each admission bound to `(network, target_height, role, seed)` |

**Important:** the candidate-set seed feeds candidate self-validation, the AVR2/true-VRF check, the
admitted-set lookup (phase21e), and the puzzle challenge's candidate digests. Whatever value the seed
takes, all of these follow it automatically — EXCEPT two places that are hard-wired to
`block.header.prev_hash`: the phase21d equality check (`src/chain.rs:1363`) and the phase21e admitted
lookup key. The puzzle/finality gates use `block.header.prev_hash` for their own (separate) purposes
and are NOT part of this contradiction.

### How H1 and H2 currently differ

- **H1** (`prev = genesis(0)`): phase21d wants `cs.seed == genesis`. phase22a incoming is **graced**
  (activation height, no parent commitment). H1's OUTGOING commitment for H2 is built with
  `ca.seed == H1.prev_hash == genesis`, `target_height == 2`. → **Accepted.**
- **H2** (`prev = H1`): phase21d wants `cs.seed == hash(H1)`. phase22a wants
  `cs.seed == pc.seed == genesis` (from H1's commitment). → **Impossible.**

---

## 3. Intended invariant

What the seed SHOULD mean, to make both gates coherent and non-circular:

1. **Candidate admission is an "epoch" frozen one block ahead.** The candidate set for height `H` is
   the set of admitted/VRF-scored workers for `H`, and it must be *committable by block `H-1`*.
   Therefore it must be a deterministic function of data known at `H-1`'s production.
2. **The candidate-set seed for height `H` = the admission-epoch seed = the grandparent hash
   `hash(H-2)` = `(block H-1).prev_hash`** (for `H ≥ 2`). This is exactly the value the committer
   (block `H-1`) already freezes (`ca.seed == (H-1).prev_hash`), so the commitment and the presented
   set agree by construction.
3. **The candidate-set seed must NOT be conflated with the per-block `prev_hash`.** `prev_hash`
   (`hash(H-1)`) legitimately seeds the *intra-block* gates that depend on the immediate tip (puzzle
   challenge context, finality-parent linkage). Candidate admission is a *cross-block, commit-ahead*
   construct and must use the epoch seed.
4. **The commitment root binds the candidate set independently of seed equality.** `pc.candidate_
   admission_root == cs.root()` already binds the full canonical serialization (identities, scores,
   tickets, ordering). Seed equality is an *additional* anti-replay/anti-context-confusion check; it
   must compare the same epoch seed on both sides, not two different block hashes.
5. **Boundary:** at the activation height (`H1`) there is no grandparent; the epoch seed is the
   genesis hash (= `H1.prev_hash`), and incoming committed-admission stays graced. For `H2` the
   grandparent IS genesis, so `epoch_seed(H2) == genesis`, which matches H1's commitment.

Formally: `admission_epoch_seed(H) = (block H-1).prev_hash` for `H ≥ 2`; `= block.header.prev_hash`
(genesis) at the activation height `H1`. Equivalently `admission_epoch_seed(H) = hash(H-2)` with the
genesis hash standing in for `hash(-1)`/`hash(0)` at the boundary.

---

## 4. Fix options

### Option A — make the committed-admission seed follow the current candidate-set seed (`prev_hash`)

Redefine phase22a so the parent commitment must bind to block `H`'s `prev_hash` (`hash(H-1)`) rather
than the committer's `prev_hash`.

- **Consensus impact:** would require block `H-1` to commit a set seeded by `hash(H-1)` = block
  `H-1`'s own hash → **circular / infeasible** without a separate two-phase precommit of the next
  hash. Not realizable by a normal producer.
- **Security:** N/A (infeasible).
- **Replay/preimage:** N/A.
- **Hidden-assignment/precommit:** would need an entirely new precommit-of-own-hash mechanism.
- **Weakens a gate?** No, but it cannot be implemented coherently.
- **H1:** unaffected (graced). **H2+:** still impossible.
- **Migration/activation:** N/A.
- **Verdict: REJECTED — circular, not implementable.**

### Option B — relax phase22a seed equality; rely on the root only

Drop `cs.seed == pc.seed` from `matches_candidate_set`, keeping `prev_hash` seeding in phase21d, and
trust `cs.root() == pc.candidate_admission_root` to bind the set.

- **Consensus impact:** `cs.root() == sha256(DOMAIN ‖ cs.serialize())` and `serialize()` **includes
  the seed** (`src/poawx_candidate.rs`). A set seeded by `hash(H-1)` and a committed set seeded by
  `hash(H-2)` have **different roots**, so the roots still won't match — unless the root is
  recomputed to EXCLUDE the seed or the commitment binds only seed-independent identities. Either
  change redefines what is committed and lets a producer present a *differently VRF-assigned* set
  whose identities happen to match.
- **Security:** weakens the "freeze the exact assigned set for this epoch" property (the whole point
  of phase22a); opens a cherry-pick/seed-substitution surface.
- **Replay/preimage:** removing the seed from the binding loses cross-epoch replay protection on the
  committed root.
- **Hidden-assignment/precommit:** weakens binding of the committed assignment.
- **Weakens a gate?** **YES** (phase22a). Forbidden by the phase rules.
- **Verdict: REJECTED — weakens phase22a.**

### Option C — explicit epoch-seed alignment (recommended; minimal variant of the "seed split")

Define `candidate_set.seed` to be the **admission-epoch seed** = the grandparent hash
`(block H-1).prev_hash` (genesis at the activation boundary). Fix the two places that hard-wire the
expectation to the immediate `prev_hash`:

- phase21d (`src/chain.rs:1363`): expect `cs.seed == admission_epoch_seed(H)` (computed from
  `previous`), not `block.header.prev_hash`.
- phase21e admitted-set lookup: key on `admission_epoch_seed(H)` instead of `block.header.prev_hash`.

phase22a is **unchanged** — its commitment seed is already the committer's `prev_hash`, which now
equals the target's epoch seed, so the incoming check passes by construction. The candidate-set
root, canonical ordering, best-for-role, admitted-set equality, AVR2/true-VRF binding
(`pr.seed == cs.seed`), and the puzzle/finality gates (which keep using `block.header.prev_hash`) all
continue to be enforced.

- **Consensus impact:** candidate assignment for height `H` is frozen as of `hash(H-2)` instead of
  `hash(H-1)` — a one-block determinism lag. This is exactly what "commit one block ahead" means and
  what phase22a already assumes; it makes the two gates consistent.
- **Security:** **strengthens** anti-cherry-pick — a producer can no longer influence its own
  candidate set via its block's contents (the seed is the already-fixed grandparent hash). The
  grandparent hash is unpredictable before the grandparent exists, so VRF unpredictability that
  matters is preserved.
- **Replay/preimage:** commitments remain bound to `(target_height, commit_height, seed, root,
  window_id, digest)`; a stale/cross-height commitment still fails (`target_height`/`seed` differ).
- **Hidden-assignment/precommit:** ticket digests are registered independently of the per-block
  seed, so ticket validity is unaffected; the VRF assignment simply uses the epoch seed (one block
  earlier), consistent across the candidate set, admissions, and AVR2.
- **Weakens a gate?** **NO.** Both phase21d and phase22a remain fully required and fully enforced;
  only the *expected seed value* is corrected to the coherent epoch seed.
- **H1:** unchanged — `epoch_seed(H1) == genesis == H1.prev_hash`, incoming still graced.
  **H2+:** satisfiable — `epoch_seed(H) == (H-1).prev_hash == committer's commitment seed`.
- **Migration/activation:** PoAW-X is mainnet-hard-off and has never produced a chain past `H1`, so
  there is **no live state to migrate**. The fix lives behind the existing PoAW-X
  candidate-set/committed-admission activation; **no new activation height and no staggering**.
  Mainnet behavior is unchanged (gates remain hard-off for `network_id == 0`).
- **Test coverage needed:** see §5.
- **Minimal vs. explicit-field variant:** the minimal variant reuses the existing `seed` field with
  the corrected (epoch) meaning — **no wire-format change**. A fuller variant could add a distinct
  `admission_epoch_seed` field alongside a `prev_hash`-bound field; that is more invasive (wire
  change + serialization/deserialization + migration of the admission/commitment formats) for no
  additional safety here, so the minimal variant is preferred.

---

## 5. Recommended design

**Adopt Option C (minimal epoch-seed alignment).**

Define a pure helper, e.g. `admission_epoch_seed(previous: Option<&Block>, height) -> [u8;32]`:
- at the PoAW-X candidate-set activation height (`H1`): return `block.header.prev_hash` (genesis);
- otherwise: return `previous.header.prev_hash` (the grandparent hash `hash(H-2)`).

Then:
- **phase21d** (`validate_block_candidate_sets`): take `previous` and require
  `cs.seed == admission_epoch_seed(previous, height)`; key the phase21e admitted-set lookup on the
  same epoch seed; everything else (canonical, best-for-role, dominance weight, admitted-set
  equality, `validate_self`/scoring) unchanged.
- **`connect_block`** (`src/chain.rs:862`): pass `previous` into `validate_block_candidate_sets`
  (phase22a already receives `previous`).
- **phase22a**: unchanged.
- **builder** (`src/poawx_mining_harness.rs`, devnet/test only): build the candidate set, candidate
  admissions, and AVR2 for height `H` with `seed = admission_epoch_seed` (grandparent), while puzzle
  and finality continue to use `block.header.prev_hash`; the outgoing commitment for `H+1` continues
  to use `ca.seed = block.header.prev_hash` (which is the epoch seed of `H+1`). The harness binary
  must supply the grandparent hash (fetch `/rpc/block?height=H-2`, or the genesis hash for `H2`).

### Why it preserves BOTH gates
- phase21d still mandates an exact, node-recomputed candidate-set seed, canonical ordering,
  best-for-role selection, dominance-weight match, AVR2 binding, and admitted-set equality. Nothing
  is removed; only the expected seed value is corrected to the epoch seed.
- phase22a is untouched: it still requires a self-consistent outgoing commitment AND an incoming
  match of `(net, target_height, seed, root, count)` against the parent's commitment.

### Why it resolves H2+
For every `H ≥ 2`, `admission_epoch_seed(H) == (block H-1).prev_hash`, which is precisely the seed
block `H-1` froze in its commitment (`ca.seed == (H-1).prev_hash`). So `cs.seed == pc.seed` and the
roots match, while phase21d's expectation now equals that same epoch seed — the contradiction is
removed without weakening either gate or staggering activation.

---

## 6. Files likely to change (implementation — DEFERRED, requires approval)

- `src/chain.rs` — `validate_block_candidate_sets` (seed expectation + phase21e lookup key + accept
  `previous`); `connect_block` call site passes `previous`. (phase22a untouched.)
- `src/poawx_candidate.rs` **or** `src/poawx_committed_admission.rs` — add the pure
  `admission_epoch_seed(previous, height)` helper (no wire change in the minimal variant).
- `src/poawx_mining_harness.rs` — devnet/test builder: seed the candidate set / admissions / AVR2
  with the epoch (grandparent) seed; keep puzzle/finality on `prev_hash`; keep the outgoing
  commitment on `prev_hash`.
- `src/bin/poawx-live-proof-harness.rs` — fetch the grandparent hash for the builder input (live
  runs only; not exercised in this phase).
- `src/chain.rs` tests — the multi-block + invariant + negative tests in §5.
- **NOT changed:** phase22a logic; puzzle/finality/dominance gates; `src/pow.rs`, LWMA, difficulty,
  target, block reward; any mainnet behavior (PoAW-X stays hard-off for `network_id == 0`).

---

## 7. Test plan (must pass BEFORE any implementation is accepted)

In-process `connect_block` tests over the locked devnet genesis (repo-local; no live nodes):

1. **H1 accepted** — regression of the existing single-block path (`phase24l_lib_builder_connect_block`).
2. **H2 accepted** — `connect_block` advances `1 → 2` with `epoch_seed(H2) == genesis`.
3. **≥5 sequential all-gates blocks** — `connect_block` advances `genesis → 1 → 2 → 3 → 4 → 5` (and
   beyond), all gates enforced each step.
4. **candidate-set seed invariant per height** — assert `cs.seed == admission_epoch_seed(prev, H)`
   for each H (genesis at H1/H2; grandparent thereafter).
5. **committed-admission root invariant per height** — assert block `H`'s candidate set matches block
   `H-1`'s commitment (`matches_candidate_set` true) for each `H ≥ 2`.
6. **bad candidate-set seed rejected** — a set seeded with `block.header.prev_hash` (`hash(H-1)`)
   instead of the epoch seed ⇒ phase21d error.
7. **bad committed-admission root rejected** — tampered/mismatched commitment root ⇒ phase22a error.
8. **replayed/stale committed admission rejected** — a commitment from the wrong height/seed (e.g.
   reused from a different epoch) ⇒ phase22a error.
9. **mainnet PoAW-X hard-disabled** — `network_id == 0` keeps the harness/gates hard-off
   (`guard_network` rejects; gates do not engage on mainnet).
10. **no LWMA/difficulty/target changes** — diff confined to candidate/admission seed + tests; no
    edits to `src/pow.rs`, LWMA, difficulty, target, or reward modules.

Build/lint gates: `cargo build --release --bin iriumd --bin poawx-live-proof-harness`,
`cargo test` for the affected suites, `cargo fmt --check`. (These run in the implementation phase,
not in 26A.)

---

## 8. Risks

- **Seed-derivation correctness at the boundary** (`H1`/`H2`): the activation-height special case
  must return the genesis hash so `epoch_seed(H2) == genesis` matches H1's commitment. Covered by
  tests 2/4/5.
- **All seed consumers must move together**: candidate self-validation, AVR2 (`pr.seed == cs.seed`),
  phase21e admitted-set lookup, and the builder must all use the epoch seed; missing one re-creates a
  mismatch. The audit (§2) enumerates every consumer.
- **Admission cache keying**: admissions for height `H` must be ingested/looked-up under the epoch
  seed; the harness/admission path must POST with the epoch seed.
- **Scope discipline**: the change must not touch phase22a's logic, the puzzle/finality gates, or any
  PoW/LWMA/difficulty/target/reward code; enforced by test 10 and review.
- **Determinism horizon**: assignment is now frozen as of the grandparent. This is intended and
  strengthens anti-cherry-pick, but the design/threat model should be re-reviewed in audit to confirm
  no liveness/grinding regression on small networks.

---

## 9. Explicit statements

- **No live run was performed in Phase 26A.** No nodes, miners, pools, or network sockets were
  started; no firewall change; no sudo; no wallet/key access.
- **No consensus code was changed in Phase 26A.** This is a design document only. The recommended
  Option C will be implemented ONLY after explicit approval, following the repo's change rules
  (read-before-edit, TDD, the §7 tests green, mainnet hard-off preserved, docs/test-branch only).
- The Phase 25C devnet-only dominance-replay builder fix (a prerequisite, also deferred) remains
  preserved as `irium-poawx-phase25c/artifacts/dominance-replay-fix-UNUSED.patch` and is not part of
  this commit.
