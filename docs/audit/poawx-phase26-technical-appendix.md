# PoAW-X Phase 26 — Technical Appendix

Companion to `poawx-phase26-independent-audit-package.md`. Detailed per-change analysis. No secrets,
keys, wallet data, or raw machine-private logs. **NOT production-ready / mainnet-ready / audited.**
Audited HEAD `0208368`.

---

## A. Epoch-seed reconciliation (Phase 26A design / 26B code — `30bce64..081a1bd`)

### The old contradiction (phase21d vs phase22a)

For a block at height `H`, `connect_block` (`src/chain.rs`) runs, in order, phase21d/21e
(`validate_block_candidate_sets`) then phase22a (`validate_block_committed_admission`). Both constrain
the SAME receipt `candidate_set`, but disagreed on its `seed`:

- **phase21d** required `cs.seed == block.header.prev_hash` ⇒ for `H` that is `hash(H-1)`.
- **phase22a** committed the candidate set one block ahead. A block's own outgoing commitment must
  satisfy `ca.seed == block.header.prev_hash`; the consuming block `H` then requires its candidate
  set to match the parent's commitment, whose seed is the parent's `prev_hash == hash(H-2)`.

So for every `H ≥ 2`, the candidate set would need `seed == hash(H-1)` AND `seed == hash(H-2)` —
impossible. Only `H1` worked (its incoming committed-admission check is graced at the activation
height). This is why every earlier phase produced exactly one block.

**Root structural reason:** committing the height-`H` candidate set one block ahead (inside `H-1`) is
only non-circular if that set is seeded by a value known when `H-1` is produced — i.e. the
grandparent hash `hash(H-2) = (block H-1).prev_hash`. A set seeded by `hash(H-1)` cannot be committed
ahead (that is `H-1`'s own, not-yet-final hash). phase21d wrongly demanded the immediate parent.

### The new invariant: `admission_epoch_seed`

`src/poawx_committed_admission.rs`:
```
admission_epoch_seed(parent_prev_hash, block_prev_hash) =
    parent_prev_hash         if parent_prev_hash is Some and != all-zero  (grandparent hash(H-2))
    block_prev_hash          otherwise (activation boundary: parent is genesis; = genesis hash)
```

The candidate-admission EPOCH seed for height `H` is the seed the parent froze in its outgoing
committed admission = the grandparent hash. `validate_block_candidate_sets` now computes this from the
current tip (`self.chain.last()`, which is block `H`'s parent at validation time, before the new
block is pushed) and requires `cs.seed == epoch_seed`; the phase21e admitted-set lookup is keyed on
the same epoch seed. For `H1`, `epoch_seed == genesis` (graced); for `H2`, `epoch_seed == genesis`
(the parent `H1`'s prev_hash); for `H ≥ 3`, `epoch_seed == hash(H-2)`.

### Why phase22a remains unchanged

phase22a's commitment seed is already the committer's `prev_hash`, which now equals the target's
epoch seed by construction. So the incoming match (`cs.seed == parent_commitment.seed` and root
equality) passes with NO change to `validate_block_committed_admission`. The fix corrects only the
*expected seed value* in phase21d/21e — not phase22a, and not any equality logic.

### Why H2+ works (and is not a weakening)

`epoch_seed(H) == (block H-1).prev_hash` is exactly what the parent committed; phase21d expects that
same value; phase22a matches it. The candidate set is now frozen as of the grandparent, a one-block
determinism lag that is intended by "commit one block ahead" and **strengthens** anti-cherry-pick (a
producer cannot influence its own candidate set via its block's contents). The grandparent hash is
unpredictable before the grandparent exists, so VRF unpredictability that matters is preserved.

The devnet/test block builder (`src/poawx_mining_harness.rs`, mainnet-hard-off) was updated to seed
the candidate set / admissions / AVR2 with the epoch seed, keep puzzle/finality/claims on the block's
`prev_hash`, build the outgoing commitment for `H+1` over `prev_hash`, and replay prior reward events
for height-`≥2` dominance weights. The live-proof binary (`src/bin/poawx-live-proof-harness.rs`)
fetches the parent `prev_hash` from the node. **These are builders, not validators** — the node
validates every block independently.

### Tests (repo-local)

- `phase26b_multiblock_epoch_seed_soak` — builds and `connect_block`-accepts **6 sequential**
  all-gates blocks (genesis→6); asserts `cs.seed == admission_epoch_seed(prev, H)` at every height
  and that dominance weights evolve (1000 at H1, < 1000 after).
- `phase26b_stale_immediate_parent_seed_rejected` (negative) — an H2 block seeded with the immediate
  parent (pre-26B seeding) is rejected with `phase21d ... seed`.
- `phase26b_committed_admission_root_and_replay_rejected` (negative) — tampered root, different-epoch
  (replay) seed, and wrong target-height commitments all fail `matches_candidate_set` (phase22a
  binding preserved, replay-safe).

---

## B. Admission-cache persistence (Phase 26D — `bfe16fd..abb2fd3`)

### Root cause

`GLOBAL_ADMISSION_CACHE` (`src/poawx_admission.rs`) is a process-global, **in-memory `OnceLock`**,
populated only by live ingest (RPC `POST /poawx/candidate-admission` and P2P gossip). phase21e
requires `cs == cache.admitted_candidate_set(...)`. On a restart the cache is empty, so the
persisted-block replay (`src/bin/iriumd.rs load_persisted_blocks`) calls `connect_block(h1)`, which
fails phase21e (a non-quarantine error), defers the block ("missing ancestors"), and never
reconnects. (Incremental sync works only because admissions gossip live with each new block.)

### The new `candidate_admissions.dat`

- `persist_snapshot` (called on each `AcceptedNew` in `ingest_bytes`) writes a length-prefixed
  snapshot of the **raw canonical wire bytes** of every cached admission, atomically (temp file +
  rename), to `<IRIUM_DATA_DIR>/candidate_admissions.dat` (`src/storage.rs candidate_admissions_file`).
  It lives in the **data root, not the state dir**, so it survives a "delete only state, keep blocks"
  resync and a same-storage restart. Bounded by the (pruned) cache size.
- `load_persisted` (startup, before the block replay) reads the file and calls
  `reload_persisted_bytes` per record.

### Revalidation on reload

`reload_persisted_bytes` re-runs the SAME validation as `ingest_bytes`: deserialize →
`network_id` match → `CandidateAdmissionV1::validate` (signature/digest/seed/true-VRF) → conflict
check → store. It deliberately skips only the live **gossip window** (`in_window`, an anti-spam
freshness check), because `admitted_candidate_set` has no window dependence and we are reconstructing
historical admitted state, not accepting new gossip. It cannot store an unvalidated admission.

### Corruption / wrong-network handling

- Missing file → 0 reloaded. Truncated/garbage tail → scanning stops (no panic).
- Per-record: empty/oversize → rejected; deserialize failure → rejected; `network_id` mismatch →
  rejected (before any state change); `validate` failure (tampered digest, bad seed/VRF) → rejected;
  conflicting digest for the same `(height, role, solver)` key → rejected.

### Why phase21e remains unchanged

phase21e's equality check is byte-for-byte unchanged; persistence only makes the already-admitted set
durable. Tests `phase26d_persist_reload_roundtrip` and `phase26d_reload_rejects_invalid_records`
cover roundtrip + rejection; `phase26d_cold_replay_with_persisted_admissions` builds a 6-block chain,
clears the in-memory cache, shows phase21e **rejects** without the reload, then reconnects all 6 after
`load_persisted`.

### Restart/keep-storage live result

A restarted node logged `reloaded N persisted candidate admissions for cold replay` and rebuilt the
active chain to height 6 **from disk** (in Phase 26C this exact restart was stuck at `local=0`); tip
and irx1 matched peers; a subsequently-mined block propagated.

---

## C. Historical-admission serving (Phase 26E — `abb2fd3..0208368`)

### Root cause

A brand-new / fresh-wipe node never received the historical admissions, so even with 26D persistence
(which only restores what a node already had) it had no admitted set for historical heights and could
not validate (or sync) a pre-existing chain.

### The new bounded send-before-block behavior

`src/p2p.rs send_historical_admissions(writer_weak, peer, start_height, block_count)`: for each served
height, take the cached admissions (`admissions_for_height`) and send each as the **existing**
`PoawxCandidateAdmissionPayload` gossip message, BEFORE the block bodies. Called at **all four**
block-serve sites (two `GetBlocks` response handlers + two "no getblocks after headers, pushing N
blocks" handshake-push paths). The change is **purely additive** — the getblocks gating, locator
logic, and validation are untouched.

### Reuse of the existing gossip/ingest path; receiver revalidation

The receiver's existing `PoawxCandidateAdmission` handler calls `global_admission_cache().ingest_bytes`,
which re-validates each admission exactly like live gossip (network + signature/digest/seed/true-VRF)
and re-broadcasts only `AcceptedNew`. A fresh node's cache tip stays at `0` during P2P gossip receive
(no `set_tip` on that path), so `in_window(h) = h ∈ [0, 64]` accepts heights `1..=6`; deeper chains
are served per getblocks batch (≤ 64 blocks), each within the window. No window/tip change was needed.

### No phase21e weakening

phase21e equality is unchanged; serving only delivers already-validated admissions, which the
receiver independently re-validates. A peer cannot inject an invalid/forged/cross-network/tampered
admission (rejected on ingest). Test `phase26e_fresh_sync_via_served_admissions`: a fresh node (empty
cache, tip 0) is rejected by phase21e without admissions, ingests the served admissions via the gossip
path (rejecting a tampered one), then connects all 6 blocks to the tip.

### Fresh-wipe live result

A fully-wiped brand-new node (`0 files`, `contiguous_from_zero=0`, `local=0`) connected to a peer,
received the historical admissions served alongside the blocks (the serving peer observed the fresh
node re-broadcasting them), re-validated + ingested them, and synced the 6-block chain to the tip in
~45 s with matching tip/irx1; a subsequently-mined block was received live.

---

## D. P2P / DoS considerations

- **Bounded send limit.** `send_historical_admissions` sends at most `16 × served_block_count`
  admission messages per response; it only runs when the node is already serving those blocks (no new
  trigger), and is a no-op when the cache has no admissions for the range.
- **Spam risk.** No new request type and no unsolicited flood are introduced; admissions ride the
  existing serve path that is already rate-limited (`getblocks_request_allowed`, grace, cooldowns).
  Re-broadcast remains governed by the existing `should_rebroadcast`/dedup logic. *Auditor question:*
  is `16×` the right bound, and is the existing serve rate-limiting sufficient under adversarial
  getblocks patterns?
- **Cache-poisoning risk.** Every received admission is re-validated by `ingest_bytes` before storage;
  conflicting admissions for the same `(height, role, solver)` key are rejected; only one canonical
  admission per key is kept. phase21e then requires the block's candidate set to EQUAL the admitted
  set, so a poisoned/extra admission cannot make a non-matching block connect.
- **Wrong-network / replay risk.** `ingest_bytes` and `reload_persisted_bytes` reject `network_id`
  mismatches; the admission digest binds `(network, height, seed, candidate[, V2])`, so an admission
  for the wrong height/seed/network does not satisfy phase21e for a different context.
- **Tampered admission handling.** Digest re-computation in `validate` rejects any mutation.
- **Stale admission handling.** `prune` drops admissions far below the tip; phase21e binds to the
  block's height/seed, so a stale admission for a different epoch does not match.
- **Peer trust assumptions.** Unchanged: a syncing node trusts a peer for *delivery* only, not
  validity — it re-validates every admission and every block. phase21e's "admitted to THIS node"
  honest limitation is pre-existing and unchanged.
- **Future hardening suggestions (non-blocking):** explicit `GetAdmissions`/`Admissions`
  request/response keyed by `(network, height, seed, root)` for precise, pull-based fetch; per-peer
  admission-send rate accounting; admission-window auto-sizing for deep public-network syncs;
  signed/auditable admission provenance.

---

## E. Mainnet safety

- **PoAW-X hard-off on mainnet.** All gates and the admission cache only engage when PoAW-X is active
  on a non-mainnet (`network_id != 0`) network; on mainnet no admissions exist, so
  `send_historical_admissions` and the persisted snapshot are no-ops, and phase21e is not enforced.
- **No mainnet activation.** No activation height, gate, or default behavior was changed for mainnet;
  the Phase 26 commits add no mainnet enablement.
- **No default storage in tests.** Persistence unit tests write to a per-process file under `target/`
  (never `/tmp`, never `.irium`); live runs used isolated `IRIUM_DATA_DIR` dirs only.
- **No wallet / private-key use.** No real wallets, keys, or signing material were touched; builder
  keys are deterministic dev/test keys (devnet-only, mainnet-hard-off) and are not on the validation
  path.
