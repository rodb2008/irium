# PoAW-X Phase 26E — serve historical candidate admissions for fresh-wipe sync

**Status: implemented + proven by repo-local tests.** Peers that hold validated candidate
admissions now serve them to a syncing peer alongside the block bodies, so a brand-new / fresh-wipe
node (empty cache, no persisted snapshot) can sync a 6+ block PoAW-X chain from scratch and pass the
**UNCHANGED phase21e gate**. This closes the remaining Phase 26D blocker. NOT production-ready /
mainnet-ready / audited.

Branch `testnet/poawx-phase20-blueprint-completion-local`. Implemented at HEAD `abb2fd3`.

## Root cause (recap from 26D)

phase21e (`validate_block_candidate_sets`, `src/chain.rs`) requires a block's candidate set to
EQUAL the node's **admitted** candidate set (`global_admission_cache().admitted_candidate_set`).
Admissions are populated only by live gossip of *fresh* admissions. Phase 26D made them durable
across restart, but a **fresh node that never received them** still has no admitted set for
historical heights, so it cannot validate (and therefore cannot sync) a pre-existing chain.

## Implemented sync mechanism (phase21e unchanged)

Reuse the existing admission gossip message. **When a node serves block bodies to a peer, it first
sends the candidate admissions for those served heights** (from its own cache — including the
26D-persisted ones it reloaded at startup). The receiver ingests each through the **normal
`ingest_bytes` path**, which re-validates it (network + signature/digest/seed/true-VRF) and stores
it; then the block bodies arrive and `connect_block` passes phase21e exactly as before.

Why this is safe and not a weakening:
- phase21e / `admitted_candidate_set` equality logic is **byte-for-byte unchanged**.
- The receiver **re-validates** every served admission via the same path live gossip uses — a peer
  cannot inject an unvalidated/wrong/tampered admission (rejected on ingest).
- The admission window (default 64) covers a fresh node's sync gap: a fresh node's cache tip is `0`,
  so `in_window(h)` accepts heights `1..=64`; deeper chains are served per getblocks batch (≤64
  blocks), each within window. No window/tip change was needed.
- Admissions are sent **before** the block bodies, so the receiver's cache is populated before it
  connects those blocks (in-order per connection).
- Bounded (anti-spam): at most `16 × served_block_count` admission messages per response.
- **No-op on mainnet** (no admissions exist; PoAW-X hard-off), so mainnet behavior is unchanged.

The serve was wired into **all four** block-serve paths in `src/p2p.rs`: both `GetBlocks` response
handlers and both "no getblocks after headers, pushing N blocks" handshake-push handlers, via one
shared helper `send_historical_admissions`.

## Files changed

- `src/p2p.rs` — new `send_historical_admissions(writer, peer, start_height, block_count)` helper
  (bounded, no-op when empty); called before the block bodies in all four block-serve sites.
  **Purely additive**; the GetBlocks gating, locator logic, and validation are untouched.
- `src/chain.rs` — **test only** (`phase26e_fresh_sync_via_served_admissions`).

NOT changed: phase21d / phase21e / phase22a logic; `connect_block`; the admission `ingest_bytes`
validation; `src/pow.rs`, LWMA, difficulty, target, block reward; mainnet behavior. (The 26D
admission persistence is reused unchanged.)

## Tests (repo-local, `cargo test --lib -- --test-threads=1`)

- `chain::phase26e_fresh_sync_via_served_admissions` — **PASS**: a "server" builds a 6-block chain
  and admits each height; a **fresh node (empty cache, tip 0)** (a) is **rejected by phase21e**
  without admissions, (b) **ingests the served admissions via the normal gossip path** (and rejects
  a tampered served record), and (c) then **connects all 6 blocks to the tip**.
- Regression: `phase26b_multiblock_epoch_seed_soak`, `phase26d_cold_replay_with_persisted_admissions`,
  `phase26d_persist_reload_roundtrip`, `phase26d_reload_rejects_invalid_records`, and the admission
  suite — all **PASS**.
- **Full lib suite: 748 passed / 0 failed** (serialized).
- Release build `--release --bin iriumd --bin poawx-live-proof-harness` — success.

## Live validation (fresh-wipe sync)

Performed after this code commit (controlled Windows + VPS-1 + VPS-2 devnet run); results recorded
in a follow-up docs update.

## Cleanup proof

Recorded in the follow-up docs update alongside the live validation.

## Remaining blockers

- Independent audit; public testnet; governance / mainnet activation.
- (Admission window tuning for very deep public-network syncs is future work; the per-batch serve
  keeps each request within the window.)
