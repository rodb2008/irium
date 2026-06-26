# PoAW-X proposer key registration (onboarding) — design

Status: DESIGN (devnet-gated, mainnet `network_id==0` hard-off). Consensus change.
Activation: `IRIUM_POAWX_PROPOSER_REGISTRATION_ACTIVATION_HEIGHT`. Builds on the
Phase 31 VRF-assigned proposer (`docs/poawx-proposer-vrf-design.md`).

## 0. Problem

The proposer eligibility registry (`ProposerEligibilityRegistry`) only gains keys
from `proposer_keys_from_block` on **connected** blocks, and `validate_block_proposer`
rejects a block whose proposer key is not frozen-registered once `eligible_count > 0`.
A key is registered only by winning a block, and a block can only be won by an
eligible key. After the genesis bootstrap window (heights `< FREEZE_DEPTH`, where
`n==0` is permissive) there is **no onboarding path**: a miner joining later can
never become eligible (observed live with rodb2008 at height ~824:
`proposer SELECTED ... eligible=8` on his miner, but his node rejects with
`proposer: vrf key not eligible (not frozen-registered)`).

## 1. Fix overview

A miner registers its proposer VRF key **on-chain** without winning a block, by
paying a one-time sybil PoW cost. After `FREEZE_DEPTH` blocks the key is eligible.
Carried as a new trailing block section (not a transaction). Works at any height.

## 2. Wire: `ProposerRegistrationV1`

```
vrf_pubkey    : [u8;33]   compressed secp256k1; pkh = hash160(vrf_pubkey) (derived)
anchor_height : u64       a recent canonical block this registration binds to
sybil_nonce   : [u8;32]
sybil_digest  : [u8;32]   = compute_sybil_digest(net, anchor_hash, hash160(vrf_pubkey),
                            anchor_height, vrf_pubkey, sybil_nonce)
signature     : [u8;64]   k256 ECDSA by the vrf key over the payload digest
```
Self-validation (pure, given `anchor_hash`, `net`, `required_bits`):
1. `sybil_digest` recomputes (reuses `poawx_ticket::compute_sybil_digest`, which already
   binds net + prev/anchor hash + key + nonce — anti-precompute, anti-reuse).
2. `meets_sybil_target(sybil_digest, required_bits)` where `required_bits =
   poawx_ticket::effective_sybil_bits()` — the same sybil PoW cost as tickets.
3. `signature` verifies against `vrf_pubkey` (proves key ownership; no junk keys).

## 3. Forced-inclusion: on-chain FIFO queue (the sound mechanism)

Block validity cannot depend on a node's local gossip pool (that would make validity
nondeterministic across nodes and split the chain — and is a trivial DoS). So
forced-inclusion is built on **deterministic on-chain state**:

- `ChainState.proposer_reg_queue : VecDeque<ProposerRegistrationV1>` — reorg-safe FIFO
  of announced-but-not-yet-activated registrations.
- The `PRG1` block section carries two lists:
  - `announces`  — new registrations the producer enqueues (best-effort from its pool).
  - `activations` — the registrations being activated this block; **must equal the
    queue head** `head[0 .. min(REG_CAP, queue.len())]`, in order.
- **Forced-drain validity rule:** a block is invalid if its `activations` are not
  exactly the deterministic queue head up to `REG_CAP`. A producer therefore cannot
  skip, reorder, or starve a queued registration — once announced, a registration is
  activated within `ceil(position / REG_CAP)` blocks, regardless of producer goodwill.
- `connect_block(H)`: verify `activations == head k`; pop head k, register each into the
  eligibility registry at `H` (eligible at `H + FREEZE_DEPTH`); append `announces` to the
  tail. `disconnect_tip_block(H)`: exact inverse — drop the tail `announces`, un-register
  the k activated keys and push them back onto the FRONT in order. Reorg-safe.

Competing blocks at the same height share the same deterministic `activations` (queue
head), so the registry converges regardless of which block fork-choice selects.

## 4. Known limitation (honest, documented)

The forced-drain queue makes **activation** unstoppable once a registration is
**announced** (on-chain). It does **not** force the first gossip→chain hop: a
registration must be included in some block's `announces` by a cooperating producer to
enter the queue. If **every** producer colludes to never announce a given newcomer,
that newcomer cannot enter — an irreducible property of any decentralized system
(no validity rule can force a producer to include something that exists only in local
gossip). In practice this reduces the trust assumption from "every producer cooperates"
to "at least one producer announces you, ever," after which activation is forced. This
is the standard honest-minority-of-producers censorship-resistance model. Closing the
first-hop vector further (e.g. proposer-rotation duties, slashing) is deferred to a
mainnet hardening pass.

## 5. Part B: eligible_count reflects real proposers only

`proposer_keys_from_block` currently also registers the 3 sub-role assignment keys
(`role_assignment_v2`), inflating `eligible_count` ~4x (why rodb saw `eligible=8` for 2
miners) and tightening `tau`. Under the gate, registration is fed **only** by the
proposer key (`proposer_assignment`) and by activated `PRG1` registrations — sub-role
keys are no longer counted. `tau = U64_MAX/n * slots` then reflects the true proposer
count.

## 6. Gating / anti-grind / mainnet

- Gated on `IRIUM_POAWX_PROPOSER_REGISTRATION_ACTIVATION_HEIGHT`; `network_id==0` hard-off.
  `PRG1` is a trailing-optional magic section ⇒ byte-identical when absent ⇒ no mainnet
  or pre-activation change.
- Freeze: the existing `frozen_window` applies to the activation height, so the seed
  `S_target` (revealed at `target-1`) cannot be used to register a winning key — and a
  key's per-height priority depends on a future, unknown seed, so keys cannot be ground
  for a target height. The sybil work binds to a recent anchor hash, bounding
  precomputation; one PoW per key, expiring with the registry window, bounds sybil.

## 7. Components

R1 type + sybil + sig + gate predicates · R2 `PRG1` wire (announces+activations) ·
R9 FIFO queue state + reorg apply/revert · R3 validation (self-validity + forced-drain) ·
R4 registry from activations + Part B · R5 pending pool + gossip + RPC + producer
assembles announces/activations · R6 miner build/broadcast · R7 template fields ·
R8 gating audit + env + mainnet-off test. `cargo test --all -- --test-threads=1`
green after each.
