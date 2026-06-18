# PoAW-X Phase 20 — CPU/GPU/ASIC Fairness Matrix (Step 6A hidden-precommit root; 6B role collection; 6C role gossip plumbing; 6D live node↔pool bridge; 6E loopback live E2E PASS; 6F two-VPS live E2E PASS; Phase 21A anti-domination/adaptive/ticket/penalty foundation primitives added)

> **Phase 21A foundation primitives (2026-06-18).** The fairness track is extended with
> blueprint-gap foundation primitives in the **node library** (consensus/network-level, **not**
> pool-owned — the pool is only one miner interface): anti-domination recent-reward tracking +
> deterministic fixed-point fairness weight (`src/poawx_dominance.rs`), adaptive mining/security
> modes with **no hardware-class assumptions** (`src/poawx_adaptive.rs`), Miner Work Tickets +
> Sybil-work cost (`src/poawx_ticket.rs`), and penalty/fraud state (`src/poawx_penalty.rs`).
> These are **data-only, gated, mainnet hard-off**. **Phase 21B now ENFORCES tickets + penalty**
> in connect_block behind `IRIUM_POAWX_TICKETS_ACTIVATION_HEIGHT`+`_REQUIRED` /
> `…PENALTY_STATE_…` (testnet/devnet, mainnet hard-off; old behavior unchanged when off) — a
> compact per-role `TicketProof` is bound into `Phase20ReceiptExt` (byte-identical when absent),
> attached by the pool, emitted by the wallet (`poawx-ticket-proof`). Anti-domination remains
> data-only (persistent/reorg-safe enforcement = Phase 21C). Details:
> `poaw-x-phase21-blueprint-gap-closure.md`. Chain difficulty remains LWMA-144 automatic.

> **Live E2E status (Steps 6E/6F, 2026-06-18):** The role-gossip → Phase 20 production path has
> been validated live, end-to-end, twice. **Step 6E** (single-VPS loopback) and **Step 6F**
> (two-VPS, role gossip over real cross-VPS P2P with an observer node validating byte-identical)
> each produced an official fee-0 block (height 2) and a third-party-fee block (height 3) from
> **collected role-gossip data with synthetic fallback OFF**, with hidden-precommit enforcement
> and restart/reload preservation. Step 6E found and fixed a real pool-only bug
> (`fee_terms_from_ext_hex` × the Step 6A trailing `precommit_root`) in commit `cdbe24c`.
> Details: `poaw-x-phase20-step6e-loopback-role-gossip-e2e.md`,
> `poaw-x-phase20-step6f-two-vps-role-gossip-e2e.md`. **Remaining:** public/external
> non-self-operated miner test; remote slow-cpuminer low-devnet PoW caveat. Mainnet remains
> disabled; chain difficulty remains LWMA-144 automatic.

**Status (updated Step 6A, 2026-06-18):** Deterministic lane assignment, role-claim
reveal/validation primitives, the 34/33/33 distribution, serialization, and activation gates
(mainnet hard-off) are implemented and tested. **Step 6A now adds the previously-missing
hidden-precommit commitment root: a role claim revealed in block H must reconstruct a leaf
committed in the PARENT block H-1's `precommit_root`, enforced in `connect_block` after the
`IRIUM_POAWX_HIDDEN_PRECOMMIT_ACTIVATION_HEIGHT` gate** (mainnet hard-off; one transition-block
grace). Claims can no longer be invented only at reveal time. It uses a prior-block sorted-root
scheme (no Merkle proofs); the leaf binds height/role/solver/secret-commitment (the lane stays
enforced separately via `validate_role_claim` — see §"Honest limitation" for why the lane is not
in the leaf). **Step 6B (2026-06-18) adds a local/testnet role precommit + reveal COLLECTION
protocol** (loopback-only pool endpoints `POST /poawx/role-precommit` + `/poawx/role-reveal`, gated
by `IRIUM_POAWX_ROLE_PROTOCOL_ENABLED=1`; a height-keyed store with canonical one-per-role selection;
production prefers collected real role data over the synthetic fallback). So testnet no longer relies
solely on synthetic claims. **Step 6C (2026-06-18) adds role precommit/reveal GOSSIP PLUMBING for
testnet/devnet**: forward-compatible node P2P wire types (`PoawxRolePrecommit = 26`,
`PoawxRoleReveal = 27`; old/mainnet peers drop them safely) + a versioned pool gossip envelope and a
conservative `RoleGossipEngine` (validate → dedupe → height-window → store in the Step 6B store →
rebroadcast only-if-newly-accepted), proven with an in-memory multi-node relay + production parity.
**Step 6D (2026-06-18) wires the live cross-process bridge**: the node P2P receive loop ingests
`PoawxRolePrecommit`/`PoawxRoleReveal` into a node-side role-gossip cache (`src/poawx_gossip.rs`,
process-global) and rebroadcasts newly-accepted payloads; four **loopback-only** RPC endpoints
(`/poawx/role-gossip/{precommit,reveal,precommits,reveals}`) let the pool POST local submissions
(forwarded to P2P) and GET node-collected gossip, which the pool ingests into its `RoleProtocolStore`
before production. So external/testnet peer gossip now reaches pool block production end-to-end over
loopback. **Still pending:** a local loopback live role-gossip E2E, then a two-VPS live role-gossip
E2E (only with operator firewall handoff). All Step 6D bridge endpoints are loopback-only — **no
public ports**; mainnet hard-off; needs both `IRIUM_POAWX_ROLE_PROTOCOL_ENABLED=1` and
`IRIUM_POAWX_ROLE_GOSSIP_ENABLED=1` (plus optional `IRIUM_POAWX_ROLE_GOSSIP_WINDOW`,
`IRIUM_POAWX_ROLE_GOSSIP_NODE_RPC`). The synthetic builder remains a testnet/devnet-only fallback
behind `IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS=1`.
Mainnet hard-off; chain difficulty remains LWMA-144 automatic. Local-only; not pushed. See
`poaw-x-phase20-production-wiring-status.md` (Steps 6A/6B) for full detail.

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
