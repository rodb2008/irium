# PoAW-X Phase 26 — Auditor Review Guide

Companion to `AUDIT_SCOPE.md`. Recommended order, invariants, threat model, attacks to consider,
tests, and known limitations. Baseline source `0208368` (HEAD `972bb9c` docs). **NOT audited /
production-ready / mainnet-ready.**

## Recommended review order

1. **Confirm the negative space first:** that phase22a (`validate_block_committed_admission`) and
   PoW/LWMA/difficulty/target/reward are unchanged in `30bce64..0208368` (see `AUDIT_SCOPE.md`).
2. **Epoch seed (A):** `admission_epoch_seed` in `src/poawx_committed_admission.rs` →
   `validate_block_candidate_sets` (phase21d/21e) seed check + admitted-set lookup in `src/chain.rs`.
   Walk H1/H2/H≥3 by hand against the appendix.
3. **Admission validity (B/C):** `CandidateAdmissionV1::validate` and `ingest_bytes` →
   `reload_persisted_bytes` / `load_persisted` / `persist_snapshot` in `src/poawx_admission.rs`.
4. **Serving (C/D):** `send_historical_admissions` + 4 call sites + the receiver handler in `src/p2p.rs`.
5. **Storage/startup:** `src/storage.rs candidate_admissions_file`, `src/bin/iriumd.rs` reload hook.
6. **Builders (confirm-only):** `src/poawx_mining_harness.rs`, `src/bin/poawx-live-proof-harness.rs`
   are mainnet-hard-off and not validators.
7. Run the tests (`REPRO_COMMANDS.md`) and read the appendix (`docs/audit/poawx-phase26-technical-appendix.md`).

## Key invariants (verify each)

- **C1** `connect_block` validates every block fully (header PoW, coinbase, receipts, phase21c/d/e/f/h,
  phase22a, phase22d) before connecting; no block connects without a matching, validated admission set.
- **C2** phase22a unchanged.
- **C3** candidate-set seed = deterministic `admission_epoch_seed` (grandparent; genesis at boundary),
  node-recomputed; mismatch rejected.
- **C4** no PoW/LWMA/difficulty/target/reward change.
- **C5** PoAW-X hard-off for `network_id == 0`.
- **P1** served admissions only ride block-serve responses; bounded `≤ 16 × served_block_count`.
- **P2** receiver re-validates every admission via `ingest_bytes` before storing.
- **S1** snapshot under isolated data root, atomic write, bounded.
- **S2** reload re-validates; corrupt/wrong-network/tampered skipped, never panics.
- **V1/V2** phase21d/21e/22a equality logic unchanged; reloaded/served admissions pass the same
  validation as live-gossiped ones.

## Security properties claimed

- **No-bypass:** persistence/serving change admission *availability*, not validity; phase21e equality
  still gates connection.
- **Delivery-only peer trust:** a node re-validates every admission and every block; peers are never
  trusted to assert validity.
- **Non-forgeability/replay-resistance:** the admission digest binds `(network, height, seed,
  candidate[, V2])`; wrong network/height/seed does not satisfy phase21e for another context.
- **Bounded resource use:** per-response admission cap; pruned cache; bounded snapshot.

## Threat model

- **Adversary:** a connected devnet/testnet peer sending arbitrary admissions/headers/blocks/getblocks,
  and a malicious block producer.
- **Deny:** (a) invalid/forged/replayed/cross-network admission accepted; (b) block connected without a
  matching, validated candidate set; (c) DoS via spam/exhaustion; (d) any mainnet effect.
- **Pre-existing trust note (unchanged):** phase21e proves "best among candidates admitted to THIS
  node in the window," not "best among all unknowable offline miners" — a documented honest limitation
  (testnet/devnet only).

## Attacks to consider

- Forge/tamper an admission (mutated candidate, wrong signature/digest) → expect rejection on ingest.
- Replay an admission at the wrong height/seed/network → expect it not to satisfy phase21e elsewhere.
- Cache poisoning: send extra/conflicting admissions for a key `(height, role, solver)` → expect
  conflict rejection and phase21e set-equality failure for a non-matching block.
- Craft a malformed/truncated/oversize `candidate_admissions.dat` → expect skip, no crash, no unvalidated
  acceptance.
- Block-without-admission: try to connect a block whose candidate set was never admitted → expect
  phase21e rejection (the persistence/serving must NOT create a bypass).
- DoS: drive getblocks to trigger large admission sends; flood gossip; partition/eclipse; deep-gap
  sync; sync-stall loops → assess bounds, rate-limits, and recovery.
- Epoch-seed manipulation: can a producer influence its own candidate-set seed (grandparent hash)?
- Window edge: admissions for heights outside `[tip, tip+64]` on a fresh vs advanced node.

## Test commands

See `REPRO_COMMANDS.md`. Summary: focused `cargo test phase26 --lib -- --test-threads=1`; full
`cargo test --lib -- --test-threads=1` (serialized — PoAW-X tests mutate process-global env + the
global admission cache; one pre-existing test lacks the shared env lock and is parallel-only flaky);
release build `cargo build --release --bin iriumd --bin poawx-live-proof-harness`.

## Live validation summaries (devnet; loopback RPC; source-restricted cross-host P2P)

- 26C: 6 all-gates blocks mined/accepted/propagated across three nodes; same final height/tip/irx1;
  a spoke-originated block included.
- 26D: restart/keep-storage cold replay — node reloaded persisted admissions and rebuilt the chain to
  height 6 from disk; H7 propagated.
- 26E: fully-wiped fresh node received served historical admissions and synced the 6-block chain from
  scratch (~45 s), matching tip/irx1; H7 received live.
- Mainnet/prod + production pool alive and untouched throughout; default storage untouched; firewall
  unchanged. Logs are summarized; no raw machine-private logs or secrets are included.

## Known limitations (not flaws to "fix," but to weigh)

- phase21e is propagation-sensitive ("admitted to THIS node in the window") — pre-existing.
- Admission window = 64; deep-chain/scale sync untested beyond small controlled runs (public-testnet
  target).
- Multi-block-from-scratch getblocks can briefly stall before handshake-push delivers blocks +
  admissions (~30–45 s on devnet).
- Tests + live runs are devnet/three-node; untrusted multi-operator, scale, and adversarial conditions
  are unvalidated (public-testnet target).
