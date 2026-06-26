# PoAW-X VRF-Assigned Proposer — Design Document

Status: DESIGN (devnet-gated, mainnet hard-off). Consensus change + fork-choice
change. Activation: `IRIUM_POAWX_PROPOSER_VRF_ACTIVATION_HEIGHT` /
`IRIUM_POAWX_PROPOSER_VRF_REQUIRED`. `network_id == 0` (mainnet) hard-off.

## 0. Problem and principle

Today the PoAW-X PRIMARY proposer (block winner + 55% reward) is **whoever solves
the PoW first**; the VRF (`AssignmentProofV2`, RFC-9381 ECVRF over secp256k1) only
orders the three sub-roles (compute/verify/support) *inside* a block, and each
miner builds its own candidate set, so that selection is self-referential. Result:
hashrate wins every block (observed: one operator's miners win 100%).

**Principle.** The chain decides who may propose each height via a VRF lottery.
CPU/GPU/ASIC are equal candidates; hashrate gives **zero** advantage for winning the
proposer slot. PoW is demoted to a trivial anti-spam floor and **removed from fork
choice**. If the assigned proposer is offline, a backup cascade (by VRF priority)
takes over so the chain never stalls.

## 1. Proposer VRF sortition (seed, proof, verification)

### 1.1 Seed `S_H` (reuse existing, committee-controlled)
`S_H = expected_epoch_seed(H, prev_hash, parent)` (`resolve_epoch_seed_parts`):
grandparent hash + **parent finality-proof digest (2/3 committee controlled)** +
parent precommit_root + epoch keying. Unpredictable until H-1 is finalized; a
single miner cannot grind it (Phase-28 anti-grind core). This is the lottery
randomness.

### 1.2 Eligible set `E_H` (anti-Sybil, frozen)
An eligible proposer is a **registered VRF public key** backed by a valid,
unexpired **sybil-PoW ticket** (existing ticket system + `MIN_TICKET_SYBIL_BITS`
floor). The registry is **frozen at `H - FREEZE_DEPTH`** (default 16), so `S_H`
(revealed at H-1) cannot be used to register a favorable key after the fact. The
validator derives `n = |E_H|` and per-key eligibility deterministically from chain
state. This is the cost that stops costless key-grinding for a low VRF output.

### 1.3 Private sortition (per miner, round `r`)
```
proof   = AssignmentProofV2::prove(sk, net, H, ROLE_PROPOSER, pkh, ticket_digest, S_H)   // existing ECVRF
priority = assignment_v2_score_from_output(proof.vrf_output)        // first 8 bytes LE, u64
selected = priority < tau(n, r)
```
`tau(n, r) = saturating( (U64_MAX / n) * cumulative_slots(r) )`, where
`cumulative_slots = [1, 4, 14, n, n, ...]` (round 0 = expected top 1, round 1 =
top 4, round 2 = top 14, round 3+ = all). `P(priority < tau(n,r)) =
cumulative_slots(r)/n`, so the EXPECTED count selected at round r is exactly
`cumulative_slots(r)`. The miner runs this **privately with its own sk** — it knows
its own score but learns no one else's score or rank until they reveal a block.

**Rank vs verifiable threshold (important).** A privately-verifiable "the single
miner with the globally-lowest score" is impossible without every eligible miner
revealing its VRF output (a miner cannot know its rank, only its own score). So the
ordered cascade (1 / next 3 / next 10 / all) is realized by the cumulative
thresholds above (top-1, top-4, top-14, all by expectation) PLUS fork choice
selecting the lowest revealed priority (Section 5). Net effect = your model: round 0
is the lowest-score online miner; if it is offline or absent, round 1 admits the
next ~3, round 2 the next ~10, etc. "Exactly 1 at round 0" holds in expectation and
is enforced as a single canonical winner by fork choice (lowest priority), not by a
per-miner global-rank proof.

### 1.4 Verification (public, deterministic)
At connect_block / submit, for the block's proposer assignment:
1. `proof.verify()` (RFC-9381 ECVRF against `assignment_public_key`).
2. `role == ROLE_PROPOSER`, `seed == S_H`, `solver_pkh == worker_pkh` (the PRIMARY
   IS the proposer), `target_height == H`.
3. `assignment_public_key` is registered + eligible + frozen at `H - FREEZE_DEPTH`.
4. `priority = proposer_priority(proof.vrf_output)`; require `priority < tau(n, round)`.
5. `block.header.time >= parent_time + round * ROUND_INTERVAL` (round timing).
Any failure ⇒ block invalid. Unforgeable (only `sk` produces a passing proof),
verifiable after reveal, hidden before reveal.

## 2. Backup proposer cascade (liveness)

The full ordered proposer list for height H is fixed by the per-miner VRF scores on
the same seed `S_H` — all positions are pre-determined but hidden, since no miner
knows its own score until it computes its VRF, and no one knows anyone else's until
they reveal. The cascade admits positions in widening rounds keyed to wall-clock
time since the parent:

- **Round 0 (primary):** opens at `parent_time`. Admits the **top 1** by VRF score
  (the lowest score) — `cumulative_slots(0) = 1`, `tau(n,0) = U64_MAX/n`.
- **Round 1 (first backups):** opens at `parent_time + 1*ROUND_INTERVAL`. Admits the
  **next 3** (top 4 cumulative) — `cumulative_slots(1) = 4`, `tau(n,1) = 4*U64_MAX/n`.
- **Round 2 (second backups):** opens at `parent_time + 2*ROUND_INTERVAL`. Admits the
  **next 10** (top 14 cumulative) — `cumulative_slots(2) = 14`,
  `tau(n,2) = 14*U64_MAX/n`.
- **Round 3+:** opens at `parent_time + r*ROUND_INTERVAL`. Admits **all remaining**
  eligible miners — `cumulative_slots(r>=3) = n`, `tau = U64_MAX`.

So if the primary is offline, the first backup (one of the next 3) takes over after
exactly **one block time**; if those are also offline, the next 10 after two block
times; and by round 3 every online eligible miner can propose — the chain advances
as long as **>= 1 eligible miner is online**.

`ROUND_INTERVAL` = **target block time**: env `IRIUM_POAWX_PROPOSER_ROUND_INTERVAL_SECS`,
default **120 s (mainnet)**, set to **30 s on devnet/testnet** (matching
`IRIUM_POAWX_MINER_INTERVAL_SECS`). It must exceed worst-case block propagation so a
round-0 block reliably reaches the network before round 1 opens — one block time
satisfies this.

A miner only *attempts* round `r` once `now >= parent_time + r*ROUND_INTERVAL`, and
the validator enforces the same via the timestamp rule (1.4.5), so a miner cannot
jump to a high round early. The canonical block is the lowest open round, then
lowest priority (Section 5) — a round-0 winner always beats a round-1 block that
raced ahead, and among same-round revealers the lowest VRF score wins.

## 3. PoW as a trivial anti-spam floor

When the gate is enforced, the block's puzzle/header PoW requirement drops to a
fixed tiny difficulty (`PROPOSER_ANTISPAM_BITS`, default ~8 leading zero bits — a
CPU solves it in milliseconds). PoW is kept ONLY to (a) bound spam of round-`r`
blocks and (b) bind the receipt/assignment into the header (so the proposer proof
cannot be swapped after mining). **PoW is removed from fork choice** (Section 5), so
extra hashrate buys nothing toward winning a height. A 1000 TH/s ASIC and a laptop
both clear the floor instantly; only the VRF assignment decides the winner.

## 4. Eligibility registry

A persistent, reorg-safe map in `ChainState`:
```
ProposerKeyRegistration { vrf_pubkey: [u8;33], pkh: [u8;20], ticket_digest: [u8;32],
                          registered_height: u64, expiry_height: u64 }
```
- **Registration:** a miner registers its VRF key via a ticket (the existing
  sybil-PoW ticket, bound to the key). Applied on connect_block of the registering
  block; reverted on disconnect (reorg-safe, like the dominance registry).
- **Freeze:** eligibility for height `H` is evaluated against the registry state as
  of `H - FREEZE_DEPTH`. A key registered at height `> H - FREEZE_DEPTH` is NOT
  eligible for `H`. Because `S_H` is unknown until H-1, and FREEZE_DEPTH >> 1, an
  attacker cannot register a key chosen to win `H` after seeing `S_H`.
- **Count:** `eligible_count(H) = |{ keys registered <= H-FREEZE_DEPTH and not
  expired at H }|`, deterministic from chain state. Used for `tau`.
- **Expiry:** keys expire (e.g. registered + TICKET window) so the active set
  tracks live miners; re-registration costs fresh sybil-PoW.

## 5. Fork choice change (the consensus-critical part)

When `proposer_vrf_enforced(height)`:
- A block's **rank** at its height is `(round, priority)` (both ascending: lower is
  better). The canonical tip at a contested height is the valid block with the
  **lowest round, then lowest priority**.
- Chain comparison uses `(sum/seq of per-height proposer ranks)` instead of
  cumulative PoW. Concretely: total "work" becomes a lexicographic chain score where
  each block contributes its `(round, priority)`; a chain that, at the first height
  they differ, has the better-ranked block wins. PoW is only a validity floor.
- **Reorg + finality interaction:** the existing finality-checkpoint reorg
  protection (`finalized_height`) is unchanged and still caps reorg depth; the VRF
  rank only decides *among* blocks above the finalized height.
- **Gating:** when the gate is OFF (mainnet, pre-activation), fork choice is
  byte-identical heaviest-cumulative-PoW. The VRF rank path is only taken when
  `proposer_vrf_enforced`.

This is what makes requirement (4)/(6) true: a CPU miner selected at round 0
(rank `(0, low)`) beats an ASIC that is unselected at round 0 and only wins at round
1 (rank `(1, ...)`) — regardless of PoW.

## 6. Adversarial considerations

- **Round/timestamp grinding:** an attacker sets a future timestamp to claim a
  higher (looser) round and get selected. Mitigated by (a) the validator's
  `time >= parent_time + round*ROUND_INTERVAL` rule, AND (b) the existing
  median-time-past / future-time header bounds — a block too far in the future is
  rejected. A miner cannot reach round `r` before `r*ROUND_INTERVAL` real seconds
  elapse, and a lower-round honest block beats it in fork choice.
- **Selective reveal / withholding to bias the seed:** because `S_{H+1}` mixes the
  parent finality digest (committee-signed) and precommit, a single proposer
  withholding or selectively revealing block H gains negligible control over
  `S_{H+1}` (the committee, not the proposer, controls the dominant source). A
  proposer that withholds simply forfeits its slot to the backup cascade.
- **Equivocation / nothing-at-stake:** a selected proposer could sign two different
  blocks at H (same round/priority). Resolved by (a) fork choice (both have equal
  rank ⇒ first-finalized wins, deeper reorg blocked by finality watermark), and (b)
  the existing fraud-proof path can be extended to slash a proposer that signs two
  blocks at the same (H, round). Nothing-at-stake across rounds is bounded by the
  round-timing rule (you cannot cheaply occupy many rounds at once).
- **Sybil key-grinding:** each eligible key costs sybil-PoW (ticket), and the freeze
  prevents post-seed registration; grinding many keys to lower the expected min
  priority costs linearly in sybil-PoW and is further damped by anti-domination
  weighting on the reward.
- **Eligibility-registry reorg safety:** registrations apply/revert exactly with
  connect/disconnect (mirroring the dominance registry) so the frozen view is
  deterministic on any fork; tests must cover reorg add/remove symmetry.

## 7. Simulation requirements before mainnet

Before any mainnet consideration (mainnet stays hard-off regardless):
1. **Fairness simulation:** N eligible miners with wildly unequal hashrate (incl. a
   1000 TH/s ASIC and a single CPU); over >= 1 anti-domination window (2016 blocks)
   the proposer distribution must match the **eligible-weighted lottery**, not
   hashrate (within statistical bounds). The CPU must win ~its fair share.
2. **Liveness simulation:** randomly take the round-0 (and round-1, ...) selected
   proposers offline; confirm the cascade advances every height with >= 1 online
   eligible miner, and the chain never stalls.
3. **Adversarial simulation:** timestamp-grinding, withholding, and equivocating
   nodes; confirm no advantage and no consensus split.
4. **Reorg/finality soak:** induced reorgs around the finalized watermark; confirm
   the VRF-rank fork choice + finality protection never disconnect a finalized block
   and converge.
5. Two-VPS + external-miner devnet soak (like the existing phaseNN rehearsals) for
   >= the exit-criteria window with zero consensus-split/crash.

## 8. Components (implementation map)

1. `src/poawx_proposer.rs` — gates, `tau`, `priority`, round/timing math, constants.
2. `ChainState` eligibility registry — reorg-safe, frozen view.
3. `Phase20ReceiptExt` — `ProposerAssignmentV1` trailing-optional section `PRP1`
   (byte-identical when absent).
4. `connect_block` — `validate_block_proposer` gate.
5. Fork-choice rewrite — VRF rank replaces cumulative PoW at contested heights, gated.
6. `submit_block_extended` — fail-fast reject non-assigned; PoW → anti-spam floor.
7. `getblocktemplate` — `proposer_seed`, `eligible_count`, `max_allowed_round`,
   `round_interval`, `freeze_height`.
8. `irium-miner` `run_poawx_solo` — private sortition; build only if selected;
   include `ProposerAssignmentV1`.

Tests: `non_selected_proposer_rejected_even_with_max_pow`,
`selected_cpu_beats_unselected_asic`, `single_deterministic_winner`,
`liveness_round_escalation`, `vrf_unforgeable_and_threshold`,
`eligibility_freeze_anti_grind`, multi-miner CPU-vs-ASIC integration.
