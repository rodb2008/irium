# PoAW-X Phase 21 — Blueprint Gap Closure (21A foundation primitives)

**Status: Phase 21A implements the FOUNDATION layer (primitives + tests), data-only and
gated.** These primitives are not (yet) wired into live block acceptance — that is Phase 21B.
Public testnet with outside miners, independent security audit, and community vote are **out of
scope** and **not** part of Phase 21A. Mainnet remains **hard-off** for all PoAW-X gates.

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
| Anti-domination / recent reward history | **PARTIAL** — tracker + fairness weight + Phase20-feed test (not yet applied to assignment) |
| Adaptive mining/security modes | **PARTIAL** — deterministic mode/policy state machine (not yet consumed by node) |
| Penalty / fraud state | **PARTIAL** — status enum + record/escalation/expiry (slashing placeholder only) |
| Fuller private assignment / VRF eligibility | **PENDING** — only an `assignment_public_key` placeholder field exists |
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
