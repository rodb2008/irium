# PoAW-X Phase 22A — Chain-committed candidate admission

**Status:** Implemented (gated, testnet/devnet only, **mainnet hard-off**, default
off; old behavior byte-identical when off). Local-only; not pushed (remote branch was
deleted; this branch is local-only). PoAW-X is **consensus/network-level**; the **pool
is one miner interface only**. **Not mainnet-ready; true VRF is NOT complete.**

## What gap this closes

Phase 21E validated a block's candidate set against the validating node's **local**
admission cache — which is propagation-sensitive (different nodes may hold different
caches, and the producer of H could omit candidates). Phase 22A **chain-commits** the
admitted candidate-set root in a prior block so selection is anchored by chain state:

- Block **H-1** carries an `AdmissionCommitmentV1` for target height **H**: the root over
  the admitted candidate set, bound to a **freeze seed = H-1's parent hash (grandparent
  of H)**, which is known when H-1 is produced — **no circularity** with H's own hash.
- Block **H**'s candidate set must reproduce that **exact committed root** (root + count +
  seed). The producer of H therefore **cannot silently add or omit candidates** relative
  to the H-1 commitment, and every node validates against the **same chain-committed root**
  instead of its own local cache.

### Honest limitation (still open)

This **does NOT prove** that offline / never-gossiped miners existed — that is an open
distributed-systems limit, not a bug. Phase 22A strengthens public-network integrity by
making the admitted set **chain-committed before selection** (removing per-node-cache
divergence at selection and the silent-omission attack at H). The committing producer
(H-1) still derives the set from gossiped admissions (21E cache).

## Primitive

`src/poawx_committed_admission.rs` — `AdmissionCommitmentV1` binds
`version, network_id, target_height, commit_height (=target-1), seed (freeze boundary),
candidate_admission_root, candidate_count, window_id` into a domain-separated digest.
Fixed 124-byte wire; `validate()` recomputes the digest and requires
`commit_height + 1 == target_height` + a candidate-count cap; `from_candidate_set` /
`matches_candidate_set` tie it to a `CandidateSet` root (which itself binds
network/height/seed/candidates). Integer-only; no floats; no wall-clock.

## Ext binding

`Phase20ReceiptExt` gains an OPTIONAL trailing **CAC1** section (`committed_admission`);
**byte-identical to pre-22A when absent**, bound into the ext digest → receipts root →
irx1 when present. The deserialize magic-dispatch handles
`TPK1`/`DOM1`/`CND1`/`PZL1`/`FIN1`/`CAC1` in any order, strict.

## Node validation

When `committed_admission_enforced(height)` (`IRIUM_POAWX_COMMITTED_ADMISSION_ACTIVATION_HEIGHT`
+ `…_REQUIRED=1`, mainnet hard-off), `connect_block` calls
`validate_block_committed_admission`:

1. **Outgoing** — any commitment THIS block carries (for H+1) must be self-consistent:
   `target = H+1`, `commit_height = H`, `seed == this block's prev_hash`.
2. **Incoming** — the PARENT block must carry a commitment for target H whose
   `seed == parent's prev_hash` (the grandparent hash), and block H's candidate set must
   **exactly match** that committed root (`matches_candidate_set`: root + count + seed +
   network + target). Missing/mismatched ⇒ reject.
3. **Low participation / bootstrap:** a one-block **activation-height grace** allows a
   pre-gate parent (no commitment) at exactly the activation height; otherwise a missing
   parent commitment **fails closed**.

## Persistence / reorg

The commitment is **block data** (carried in the parent block's ext), so it is inherently
**deterministic, replayable on restart, reorg-safe, and reverted on disconnect** — no
separate `ChainState` field. On a reorg A→B the new parent carries B's commitment, and
`connect_block` replay re-validates H against it. Tested by validating a child against a
parent-with-commitment vs a bare parent.

## Pool integration

The pool mirrors `AdmissionCommitmentV1` byte-for-byte (`AdmissionCommitmentMirror`) and
`build_pool_committed_admission` builds the commitment for the next height from the **21E
pool admitted cache** (freeze seed = block prev_hash). `build_synthetic`/`build_collected`
attach it when `pool_committed_admission_enforced` and **fail closed** (no ext) if no
admitted candidates. Official fee-0 + third-party fee paths preserved; Phase 21E local
admission behavior is unchanged when the 22A gate is off. The pool never bypasses node
validation — the node re-validates the commitment and that the candidate set reproduces the
committed root. (Full multi-block production threading of the committed freeze seed into the
current candidate set is the operational follow-up; the consensus enforcement is complete.)

## Wallet

`irium-wallet poawx-committed-admission --network-id <id> --target-height <H>
--candidate-admission-root <64hex> --candidate-count <N> --seed <64hex> [--commit-height]
[--window-id]` emits the commitment JSON + `wire_hex` (for the producer to embed in the
commit block). **No private key / no seed phrase**; testnet/devnet only; mainnet hard-off.
The existing emit-only candidate-admission/assignment/finality helpers are unchanged.

## Gates (default off, mainnet hard-off)

`IRIUM_POAWX_COMMITTED_ADMISSION_ACTIVATION_HEIGHT`, `…_REQUIRED=1`, `…_WINDOW`. Each
returns false on mainnet (`network_id == 0`). When off, Phase 21E behavior is unchanged.
Chain difficulty remains **LWMA-144 automatic** — committed admission never touches PoW.

## Tests

- `poawx_committed_admission`: wire round-trip + validate, mutation breaks digest/match,
  `commit_height == target-1` enforced, count cap, gate logic + mainnet hard-off.
- `chain`: `phase22a_committed_admission_enforcement` (match accepts; mutated/missing-set/
  missing-parent-commit reject; activation grace; wrong-freeze-seed own-commit rejects;
  mainnet hard-off).
- `poawx`: `phase22a_ext_committed_admission_roundtrip` (absent byte-identical; present +
  mutation change digest; committed-admission + candidate-set together).
- pool: `phase22a_pool_committed_admission_parity` (node validates the pool commitment;
  committed root == node admitted-set root; matches; fail-closed; mainnet hard-off).
- wallet: `phase22a_committed_admission_emit_no_secret_mainnet_off`.

## Remaining technical gaps (after Phase 22A)

- **True cryptographic VRF** — still a placeholder (`AssignmentProofV1`); Phase 21G
  Outcome B (no safe dep/key-model path in-tree). Not solved here.
- **Provably-complete public-network admission** — strengthened (chain-committed) but
  cannot prove offline/never-gossiped miners existed.
- **Excluded (not in this track):** public testnet with outside miners, independent
  security audit, community vote, mainnet activation.
