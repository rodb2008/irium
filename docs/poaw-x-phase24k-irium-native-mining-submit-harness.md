# PoAW-X Phase 24K — Irium-native all-gates mining submit harness

**Goal:** close the Phase 24J blocker (mining *tooling*, not consensus) by mining a real
all-gates PoAW-X block with **Irium's actual PoW hash** and validating it through the
authoritative node validators. Code/test-first; no live nodes/miners launched. Local-only; not
pushed; mainnet hard-off; no validator weakened; no PoW bypass.

## Why stock cpuminer is incompatible (recap of 24J)

Stock cpuminer/minerd hashes a standard Bitcoin 80-byte header with `sha256d`. Irium hashes the
block header via `BlockHeader::hash_for_height(height)` — a **height-bound** serialization
(`serialize_for_height`) — so minerd's candidate hashes never match Irium's PoW target. In 24J
minerd produced 0 valid shares after ~900M hashes against an easy target. The fix is to mine with
Irium's real hash path, which is what this phase does.

## Selected design

**Option 3 (in-process deterministic test) — the safest minimal option**, plus a small,
reusable, mainnet-hard-off harness module so a future live Phase 24L can reuse the exact PoW grind
and guards. No new binary, no service, no runtime storage, append-only (no existing function
changed).

New `src/poawx_mining_harness.rs`:
- `guard_network(network_id)` — refuses mainnet (`network_id == 0`); only testnet(1)/devnet(2).
- `guard_isolated_storage(Option<&Path>)` — refuses `None`, `/tmp`, and the production default
  `$HOME/.irium`; requires an explicit isolated dir.
- `mine_pow(header, height, target, max_iters)` — grinds the nonce using the **real** Irium PoW
  path (`hash_for_height` + `meets_target`, the exact check `validate_block_header` runs). Never
  reads or changes LWMA/difficulty/target state; `target` is supplied by the caller from the
  chain's `target_for_height`.

## Stage 1 — validator-level mined all-gates block (DONE)

`src/chain.rs` test `phase24k_native_pow_all_gates_validators`:
1. Builds ONE block at **height 1 over the real locked genesis** (`prev_hash = genesis hash`,
   `bits = target_for_height(1)`, real coinbase + merkle), carrying **every** gate section:
   ticket digests (bound in the candidates/AVR2), dominance/fairness weights, candidate set,
   chain-committed admission, per-role puzzle proofs, SUPPORT-committee finality proof, true-VRF
   AssignmentProofV2 (RFC 9381 ECVRF), and the canonical 0%-fee role-reward coinbase.
2. **Mines Irium's real PoW** via `poawx_mining_harness::mine_pow` and asserts the mined header
   satisfies `meets_target(hash_for_height(1), target)`.
3. Asserts every **authoritative** validator accepts the mined block:
   `validate_block_header` (real PoW + bits + merkle), `validate_block_dominance_weights`,
   `validate_block_candidate_sets` (incl. node-admitted-set equality), `validate_block_puzzle_proofs`,
   `validate_block_finality`, `validate_block_committed_admission`, `validate_block_true_vrf`,
   and `validate_poawx_coinbase_payout` (official 0% fee split).
4. Negatives (E13–E17): missing AVR2 / missing finality / missing puzzle / wrong role solver /
   wrong committed-admission seed each reject. Harness unit tests cover E1 (rejects mainnet), E2
   (refuses missing/`/tmp`/`$HOME/.irium` storage), E11 (real-PoW grind), E18 (no secret in
   guard output).

Key correctness facts proven by composition: every gate binds its seed to the block's
`prev_hash` (= genesis hash); the SUPPORT solver is `hash160(finality member pubkey)` so the
committee vote validates; candidate dominance weights equal the node's persisted state; the
included candidate set byte-equals the node-admitted set.

## Stage 2 — full `connect_block` end-to-end (DONE)

`src/chain.rs` test `phase24k_native_pow_all_gates_connect_block`: builds a mined all-gates block
at height 1 over the real locked genesis and drives the **entire** `connect_block` pipeline to
acceptance, advancing the chain to **height 2**:
`validate_block_header` (real Irium PoW) → `validate_poawx_coinbase` (non-zero **gated** `irx1`
root, ext-bound) → `validate_poawx_block_receipts` (receipt signature + receipt PoW difficulty +
the Phase 20 production payout via `validate_phase20_production_block`) → dominance → candidate
set + node admission → puzzle → finality committee → committed admission → true-VRF →
`validate_and_apply_transactions` (canonical **0%-fee** multi-role coinbase). Mainnet hard-off
(devnet `network_id=2`). Passed first attempt; executed in 0.08s (mining is instant on the devnet
target `0x207fffff`).

**Scope honesty:** Stage 2 exercises the `connect_block`-INTEGRATED gates together on one mined
block. The **independent** hidden-precommit, ticket-proof (`role_ticket_proofs`), and mode-1
delegation gates are intentionally left OFF — each has its own dedicated tests; disabling an
optional gate in this test does not weaken any gate's enforcement. (Ticket *digests* are still
present and validated inside the candidates/AVR2.) Wiring all of those into a single connect_block
fixture would add construction surface without proving anything the per-gate tests don't already
cover.

## Safety / scope

- Mainnet hard-off: the harness refuses `network_id == 0`; every PoAW-X gate is mainnet-hard-off
  via `network_kind_from_env`.
- LWMA/difficulty/target/PoW logic **unchanged** — `mine_pow` only searches a nonce against a
  caller-supplied target derived from the existing `target_for_height`.
- No secrets/keys/mnemonics printed. No live services. No `/tmp` storage. No `~/.irium` touched.
- No production code path calls the harness; only tests (and a future 24L dev binary) do.

## Claim status

- Irium-native PoW mining proven in-process: **YES**.
- Every all-gates validator accepts a really-mined block: **YES (Stage 1)**.
- Full `connect_block` end-to-end acceptance: see Stage 2 result.
- Production-candidate / mainnet-ready: **NO** (a live cross-host run, independent audit, public
  testnet, and governance activation remain).

## Remaining blockers

- Live Phase 24L run (a real devnet node + the harness as a dev binary submitting through the
  node) — not done in this code/test-first phase.
- Cross-host P2P provider/firewall; independent audit; public testnet; governance/mainnet
  activation.
