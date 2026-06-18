# PoAW-X Phase 21F — Puzzle work modes + fast verification

**Status:** Implemented (gated, testnet/devnet only, **mainnet hard-off**, default
off; old behavior byte-identical when off). Local-only; not pushed. Builds on Phase
21A–21E. PoAW-X is **consensus/network-level**; the pool is one miner interface, not
the owner — the node re-verifies everything.

## NOT chain PoW

Puzzle work modes are **assigned-work verification primitives**, NOT a replacement for
Irium's chain Proof-of-Work or LWMA-144 difficulty. **Nothing in this phase changes the
block interval, `bits`, the block target/PoW validation, or LWMA.** `STRATUM_DEFAULT_DIFF`
remains the stratum **share** difficulty only. Puzzle hashing is domain-separated so it
can never collide with chain PoW hashing. The puzzle difficulty (`anchor_bits`, etc.) is a
small assigned-work threshold, not a chain target.

## Puzzle modes (`src/poawx_puzzle.rs`)

`PuzzleMode`: **Sha256dAnchor**, **RandomMemory**, **ParallelCompute**,
**VerificationWork**, **FinalityWorkPlaceholder**. A deterministic, domain-separated
`assign_puzzle_mode` selects the mode from `(network, height, role, solver, ticket
digest, assignment-proof digest, seed)` — **no hardware-class assumptions; any miner may
attempt any mode** (no CPU/GPU/ASIC lanes). Mode/challenge change if any binding input
changes.

- **Sha256dAnchor** — find a nonce s.t. `sha256d(domain‖challenge‖nonce)` meets a small
  leading-zero-bit threshold. Assigned-work proof only.
- **RandomMemory** — a bounded deterministic memory-walk over a small scratch
  (`mem_words ≤ 4096`); the solution is a **compact final digest** (never a memory dump);
  threshold-gated by nonce.
- **ParallelCompute** — a bounded deterministic multi-lane hash (`lanes ≤ 16`); **no GPU
  required**; threshold-gated.
- **VerificationWork** — a deterministic reference digest binding the challenge to
  another candidate's assignment-proof reference (proves a verifier role is validatable).
- **FinalityWorkPlaceholder** — a deterministic placeholder bound to the seed for future
  finality-committee integration. **Placeholder only — full finality committee is not
  implemented.**

## Challenge / solution / difficulty

- `PuzzleChallengeV1` binds network/height/role/solver/ticket-digest/assignment-proof-
  digest/candidate-digest/seed/mode/profile into a domain-separated `challenge_digest`.
- `PuzzleSolutionV1` is a **compact fixed 41-byte wire** (`mode + nonce + proof_digest`)
  — never a memory dump.
- `PuzzleDifficultyProfile` is **integer-only and HARD-BOUNDED** (`anchor_bits ≤ 24`,
  `mem_words ≤ 4096`, `lanes ≤ 16`, `iterations ≤ 4096`); `anchor_bits` configurable only
  behind the testnet gate (`IRIUM_POAWX_PUZZLE_BITS`).

## Fast verification

`verify_solution` is **bounded, deterministic, allocation-bounded, float-free, no network
calls, no wall-clock** — consensus-safe. It never grinds: it recomputes the assigned
work once from the supplied nonce (or the deterministic reference for
verification/finality) and checks the digest + threshold. `solve_dev` (dev/pool side)
grinds within a bounded cap.

## Ext integration + node enforcement

`Phase20ReceiptExt` gains an OPTIONAL trailing **PZL1** section:
`role_puzzle_proofs[compute, verify, support]` (3 × `PuzzleSolutionV1`); **byte-identical
to pre-21F when absent**, bound into the ext digest → receipts root → irx1 when present.
The deserialize magic-dispatch handles `TPK1`/`DOM1`/`CND1`/`PZL1` in any order, strict.

When `puzzle_work_enforced(height)` (`IRIUM_POAWX_PUZZLE_WORK_ACTIVATION_HEIGHT` +
`…_REQUIRED=1`, mainnet hard-off), `connect_block` calls `validate_block_puzzle_proofs`:
it requires the candidate set (the per-role challenge is derived from each role's selected
candidate), recomputes the challenge for each of the 3 fairness roles, and **verifies the
solution before accepting the role reward**. Fails closed on missing proofs / missing
candidate set / invalid solution. Gate off ⇒ unchanged behavior; mainnet hard-off.

## Pool support

`pool/irium-stratum` mirrors `poawx_puzzle` byte-for-byte (mode assignment, challenge
digest, anchor/memory/parallel/verification/finality outputs, bounded profile, solve
grind). `build_pool_puzzle_proofs` solves the assigned puzzle for each selected candidate;
`build_synthetic`/`build_collected` attach the proofs when `pool_puzzle_work_enforced` and
**fail closed** (no ext) if the candidate set/solution is unavailable. Official fee-0 and
third-party fee paths preserved; candidate-admission/candidate-set/ticket/dominance/
penalty enforcement remain compatible. A parity test proves each pool-solved puzzle
verifies via the node lib. **The node re-verifies, so the pool is one interface, not the
owner.**

## Wallet helpers

- `irium-wallet poawx-puzzle-challenge …` — emits the deterministic challenge
  (mode/mode_name/bounded profile/challenge_digest) for a context.
- `irium-wallet poawx-puzzle-solve …` — solves it and emits the compact solution
  (mode/nonce/proof_digest + `wire_hex`) with a self-verify flag.

Both emit-only, **no private key / no seed phrase**, testnet/devnet only, mainnet hard-off.

## Gates (all default off, mainnet hard-off)

- `IRIUM_POAWX_PUZZLE_WORK_ACTIVATION_HEIGHT`, `IRIUM_POAWX_PUZZLE_WORK_REQUIRED=1`,
  `IRIUM_POAWX_PUZZLE_BITS` (testnet-only difficulty).

Each gate returns false on mainnet (`network_id == 0`). Chain difficulty remains
**LWMA-144 automatic** — puzzle work never touches PoW.

## Tests

- `poawx_puzzle`: mode determinism, challenge mutation sensitivity, solve+verify all 5
  modes, threshold/nonce/seed rejects, solution wire round-trip, profile bounds, gate
  logic + mainnet hard-off.
- `chain`: `phase21f_puzzle_enforcement` (accept valid; reject missing proofs / missing
  candidate-set / tampered / wrong-height; mainnet hard-off).
- `poawx`: `phase21f_ext_puzzle_section_roundtrip` (absent byte-identical, present changes
  digest, mutation changes digest, puzzle+dominance both).
- pool: `phase21f_pool_puzzle_proofs_parity_and_failclosed` (pool puzzle verifies via node
  lib, build_synthetic attaches + node reads PZL1, fail-closed without candidate set,
  mainnet hard-off).
- wallet: `phase21f_puzzle_challenge_and_solve_no_secret_mainnet_off`.

## Remaining technical steps

- **True cryptographic VRF** (replace the `AssignmentProofV1` placeholder).
- **Full finality-committee integration** (the FinalityWorkPlaceholder mode is a
  deterministic placeholder only).
- Provably-complete public-network candidate admission.
- **Excluded (not in this track):** public testnet with outside miners, independent
  security audit, community vote, mainnet activation.
