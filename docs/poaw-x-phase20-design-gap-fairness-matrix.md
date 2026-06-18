# PoAW-X Phase 20 — CPU/GPU/ASIC Fairness Matrix (hidden-precommit commitment root NOW implemented in Step 6A; live role-claim networking pending)

**Status (updated Step 6A, 2026-06-18):** Deterministic lane assignment, role-claim
reveal/validation primitives, the 34/33/33 distribution, serialization, and activation gates
(mainnet hard-off) are implemented and tested. **Step 6A now adds the previously-missing
hidden-precommit commitment root: a role claim revealed in block H must reconstruct a leaf
committed in the PARENT block H-1's `precommit_root`, enforced in `connect_block` after the
`IRIUM_POAWX_HIDDEN_PRECOMMIT_ACTIVATION_HEIGHT` gate** (mainnet hard-off; one transition-block
grace). Claims can no longer be invented only at reveal time. It uses a prior-block sorted-root
scheme (no Merkle proofs); the leaf binds height/role/solver/secret-commitment (the lane stays
enforced separately via `validate_role_claim` — see §"Honest limitation" for why the lane is not
in the leaf). **Still pending (Step 6B):** the LIVE public role-claim networking/gossip — Step 6A
uses a gated testnet/devnet **synthetic** precommit/reveal builder, NOT a public protocol.
Local-only; not pushed. See `poaw-x-phase20-production-wiring-status.md` (Step 6A) for full detail.

## Core principle (implemented as designed)
PoAW-X does **not** detect or trust hardware. The chain never asks "is this really a
CPU/GPU/ASIC?". Instead there are verifiable puzzle **lanes** with different resource
profiles; **any miner may attempt any lane**. The protocol deterministically rotates/balances
lane assignment per `(height, role slot)` so no hardware class permanently dominates, targeting
a **34/33/33** split across the three production lanes. This distribution applies to the
**auxiliary PoAW-X role slots**, NOT to normal chain difficulty (which stays automatic via
LWMA-144).

## Lanes (implemented)
- `CPU_FRIENDLY` (id 0), `GPU_PARALLEL` (id 1), `ASIC_STREAMING` (id 2) — the three production
  fairness lanes.
- `UNIVERSAL_FALLBACK` (id 255) — **dev/test only**, excluded from production fairness
  distribution; `assign_lane` never returns it and `validate_role_claim` rejects it.

## Role slots (implemented)
`COMPUTE_CONTRIBUTOR` (1), `VERIFY_CONTRIBUTOR` (2), `SUPPORT_CONTRIBUTOR` (3). `PRIMARY_MINER`
is **not** a fairness role and is never assigned a lane here (the existing primary path is
unchanged).

## Deterministic assignment (implemented, `src/poawx.rs`)
`fairness_assignment_digest = SHA256(b"IRIUM_POAWX_FAIRNESS_V1" || network_id || height_le8 ||
prev_hash || role_id || slot_index_le4)`. `assign_lane(...)` reduces the first 8 digest bytes
(LE) mod 10000 and maps: `0..3399 → CPU`, `3400..6699 → GPU`, `6700..9999 → ASIC`.
Independently verifiable by every node; deterministic (same inputs → same lane).

## Role-claim reveal primitive (implemented)
`role_claim_digest = SHA256(b"IRIUM_POAWX_ROLE_CLAIM_V1" || network_id || height_le8 ||
prev_hash || role_id || lane_id || solver_pkh || nonce || secret)`.
`PoawxRoleClaim { role_id, lane_id, solver_pkh[20], nonce[32], secret[32], claim_digest[32],
commitment_hash: Option<[32]> }` with canonical serialize/deserialize (fixed 118-byte prefix +
1 flag + optional 32-byte commitment). `validate_role_claim(...)` (pure) checks: role id known,
lane id a production lane, claim digest recomputes from revealed fields, and the claimed lane
equals the deterministic assignment for `(net, height, prev_hash, role, slot)`. Malformed/
truncated/unknown-lane/unknown-role/wrong-lane/tampered-nonce all reject.

## Activation gate (implemented)
`IRIUM_POAWX_FAIRNESS_MATRIX_ACTIVATION_HEIGHT` via `chain::fairness_matrix_active(height)`:
**mainnet hard-false** regardless of env; default off; testnet/devnet gate on the height.
Before activation, existing behavior is unchanged (the primitives are not called anywhere yet).

## Tests (all passing)
poawx: assignment deterministic + sensitive to height/prev; **34/33/33 distribution over 3600
deterministic assignments (±3pp tolerance, never fallback)**; lane/role id round-trip + unknown
rejects + fallback excluded; role-claim wire round-trip (with/without commitment) + truncation +
bad-flag reject; validation accept + reject (wrong lane, tampered nonce, unknown role, unknown
lane, fallback, wrong height/slot). chain: fairness gate mainnet-off + testnet height + no-height-off.

## Honest limitation (why PARTIAL, not COMPLETE)
The assignment digest includes `prev_hash` (block H-1's hash), which a miner of block H already
knows. There is currently **no mechanism to prove a commitment existed *before* the assignment
seed (`prev_hash`) was known** — i.e. no on-chain / prior-block **commitment root**. Therefore
the "hidden-before-reveal" property is **not yet consensus-enforceable**: a claimant could craft
`nonce/secret` after seeing `prev_hash`. The `commitment_hash` field is provided as the future
binding point, but enforcing it requires a prior-block commitment root (or equivalent) that
records commitments before the seed is revealed. **Until that exists, this is PARTIAL** and is
not wired into `connect_block`.

## How this feeds the multi-role reward split
Once live (with a commitment root), `validate_role_claim` produces verified
`COMPUTE_CONTRIBUTOR`, `VERIFY_CONTRIBUTOR`, and `SUPPORT_CONTRIBUTOR` claimant pkhs for a
block/height. Those pkhs become the `compute_contributor_pkh` / `verify_contributor_pkh` /
`support_contributor_pkh` inputs to the already-implemented `RoleReward` + multi-role coinbase
validator. So the multi-role split's remaining "role-claim source" gap is **filled at the
primitive level**; production wiring (claim collection → RoleReward → coinbase) remains the
follow-up, and depends on the commitment root for the hidden-precommit guarantee.

## Remaining (follow-up)
1. **Commitment root** (on-chain / prior-block) to make hidden-precommit consensus-enforceable.
2. Live `connect_block`/role-claim enforcement gated on `fairness_matrix_active`.
3. Production wiring: claim collection → role pkhs → multi-role coinbase (ties to the multi-role
   production follow-up).
4. Separate: **third-party pool fee** (own design-gap doc).

Mainnet remains disabled; chain difficulty remains automatic via LWMA-144.
