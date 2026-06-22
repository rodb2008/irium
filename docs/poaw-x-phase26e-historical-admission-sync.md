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

## Live validation (fresh-wipe sync) — PASSED

Controlled Windows + VPS-1 + VPS-2 devnet run on this build (HEAD `9de939f`):

- Windows egress IP `122.162.151.91` (unchanged); existing source-restricted VPS-1 UFW rule for
  `41210` reused; **no firewall change**. RPC loopback-only; isolated `p26e` dirs.
- **6 all-gates blocks mined + propagated to all three**; final height 6, tip
  `607b6069ccd05ceabc8b89f6d45b3ac83a1322d4cc1305a1fdca11c5251df572`,
  irx1 `4d8611dbec979495512b9894e0889f1df12e75b7960be14b2899a1fee0466b4a`.
- **Fresh-wipe (the Phase 26D remaining blocker):** stopped VPS-2 by exact PID, then **fully wiped
  its Phase 26E storage** (`rm -rf irium-p26e-vps2-node` — blocks + state + data, incl. the
  `candidate_admissions.dat` snapshot; `0 files` left), and restarted it as a **brand-new node**
  (`contiguous_from_zero=0`, `local height=0`). It connected to VPS-1, **received the historical
  candidate admissions** served alongside the blocks (the hub logged VPS-2 re-broadcasting them),
  ingested + re-validated them via the normal path, **connected the blocks, and reached height 6**
  in ~45 s — tip + irx1 **exactly matching** VPS-1 and Windows. (In Phase 26C/26D this fresh-wipe
  case could not sync.)
- **New block after fresh sync:** mined H7 (Windows); the fresh-synced VPS-2 received it live — all
  three at height 7, tip `2ad6f9cb1b5aec217177c46132edb23b12c2bd6307434c6fe945a767d10014da`,
  irx1 `030563892c6cbd00fc01de199f07db81d140c50343452e029aaa89311f95f25a`.

## Cleanup proof

- All three Phase 26E nodes stopped by **exact pidfile PIDs** (no pkill/killall): Windows `24756`,
  VPS-1 `48363`, VPS-2 `2109223` — all STOPPED. All p26e ports closed
  (`41210/41411/41408`, `41420/41421/41418`, `41430/41431/41428`).
- Mainnet/prod alive and untouched: Windows `33752`, VPS-1 `219530`, VPS-2 `1851441`; VPS-1 prod
  pool alive.
- Default storage untouched (all predate this run): Windows `%USERPROFILE%\.irium` (2026-06-07),
  VPS-1 `~/.irium` (2026-06-21), VPS-2 `~/.irium` (2026-06-06). UFW unchanged. No real wallets
  touched. Artifacts preserved under `phase26e-artifacts-vps1/vps2` and the Windows
  `irium-poawx-phase26e\artifacts`.

## Remaining blockers

- Independent audit; public testnet; governance / mainnet activation.
- (Admission window tuning for very deep public-network syncs is future work; the per-batch serve
  keeps each request within the window.)
