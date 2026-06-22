# PoAW-X Phase 26 — Independent Audit Package

**Audit-prep only. NOT production-ready, NOT mainnet-ready, NOT audited.** This package summarizes
the Phase 26 changes (epoch-seed reconciliation + cold-resync hardening) for an independent review.
It contains no secrets, private keys, wallet data, or machine credentials; logs are summarized.

- Repository: `https://github.com/iriumlabs/irium.git`
- Branch: `testnet/poawx-phase20-blueprint-completion-local`
- Audited HEAD: **`0208368`** (`origin/main` unchanged at `19c496dc5f2fa08981a109b10eeb257105c28c43`).
- Full source audit range: **`30bce64..0208368`** (8 source files, +1006/−47; the rest is tests + docs).

See the companion docs:
- `docs/audit/poawx-phase26-technical-appendix.md` — detailed per-change analysis (A–E).
- `docs/audit/poawx-phase26-auditor-checklist.md` — questions for the auditor to answer.

## Executive summary

PoAW-X is a multi-role proof-of-aligned-work overlay validated by gated sections inside
`connect_block` (phase21c dominance, phase21d/21e candidate set/admission, phase21f puzzle, phase21h
finality, **phase22a committed admission**, phase22d true-VRF). It is **hard-off on mainnet**
(`network_id == 0`) and only enforced on devnet/testnet.

Three problems were fixed in Phase 26, each without changing the gate equality logic:

1. **Multi-block was unsatisfiable** (Phase 26A/26B). For any block at height `H ≥ 2`, phase21d
   demanded `candidate_set.seed == block.prev_hash` (`hash(H-1)`) while phase22a's commit-one-ahead
   demanded `candidate_set.seed == parent_commitment.seed` (`hash(H-2)`). No block could satisfy
   both. Fix: define the candidate-set seed as the **admission epoch seed** = the grandparent hash
   (the value the parent already froze), and validate phase21d against it. **phase22a unchanged.**
2. **Restart cold-resync failed** (Phase 26D). The admitted-candidate cache (needed by phase21e) was
   in-memory only, so a restarted node could not re-validate persisted blocks. Fix: persist
   validated admissions to the data root; reload + **re-validate** at startup. **phase21e unchanged.**
3. **Fresh-wipe sync failed** (Phase 26E). A brand-new node never received historical admissions.
   Fix: when serving block bodies, first send the matching admissions (existing gossip message); the
   receiver re-validates each through the **normal ingest path**. **phase21e unchanged.**

All three were proven by repo-local `connect_block` tests and live-validated across Windows + VPS-1 +
VPS-2 (devnet, loopback RPC, source-restricted cross-host P2P, mainnet/prod untouched).

## Phase timeline / commit map

| Phase | What | Key commit(s) | Diff range |
|------|------|---------------|------------|
| 26A | Seed-reconciliation **design doc** (no code) | `30bce64` | — |
| 26B | Epoch-seed alignment **code + tests** | `081a1bd` | `30bce64..081a1bd` |
| 26C | Live three-system multiblock soak (docs) | `bfe16fd` | — |
| 26D | Admission-cache **persistence** code + docs | `de13a83`, `abb2fd3` | `bfe16fd..abb2fd3` |
| 26E | Historical-admission **serving** code + docs | `9de939f`, `0208368` | `abb2fd3..0208368` |

Audit diff ranges: seed reconciliation `30bce64..081a1bd`; cold-replay persistence
`bfe16fd..abb2fd3`; fresh-sync admissions `abb2fd3..0208368`; full range `30bce64..0208368`.

## Files changed (source)

Consensus-relevant (in the `connect_block` / phase21e path):
- `src/chain.rs` — phase21d/21e expected candidate-set seed now = `admission_epoch_seed(...)` and the
  phase21e admitted-set lookup is keyed on it. **phase22a, phase21d's other checks, and all gate
  equality logic are unchanged.** (The large line count here is the added tests.)
- `src/poawx_committed_admission.rs` — new pure helper `admission_epoch_seed(parent_prev, block_prev)`.
- `src/poawx_admission.rs` — admission-cache **persistence** (`persist_snapshot`/`load_persisted`/
  `reload_persisted_bytes`) reused by both phases; `ingest_bytes` validation **unchanged**.
- `src/storage.rs` — `candidate_admissions_file()` path under the isolated data root.
- `src/bin/iriumd.rs` — startup reload hook (before persisted-block replay).
- `src/p2p.rs` — `send_historical_admissions` helper called at the 4 block-serve sites
  (**purely additive**; getblocks gating/locator/validation untouched).

Devnet/test-only (NOT validators, NOT on the consensus path):
- `src/poawx_mining_harness.rs`, `src/bin/poawx-live-proof-harness.rs` — the live-proof block
  **builder** (mainnet-hard-off). Produces blocks for tests/devnet; the node still validates every
  block independently.

## Threat model (summary; see appendix D)

- **Adversary:** a connected devnet/testnet peer that can send arbitrary P2P messages (admissions,
  headers, blocks, getblocks) and a malicious block producer.
- **Goals to deny:** (a) get an invalid/forged/replayed/cross-network admission accepted; (b) get a
  block connected whose candidate set was not independently admitted+validated; (c) DoS via spam or
  resource exhaustion; (d) any effect on mainnet.
- **Trust assumption (unchanged from before Phase 26):** phase21e proves "best among candidates
  ADMITTED to THIS node in the window," not "best among all unknowable offline miners" — a
  documented honest limitation of the admission model, testnet/devnet only.

## Invariants

### Consensus invariants (must hold)
- C1. `connect_block` validates **every** block fully (header PoW, coinbase, receipts, phase21c/d/e/f/h,
  phase22a, phase22d) before connecting. No block connects without a matching, validated candidate
  admission set (phase21e equality).
- C2. phase22a (committed-admission self-consistency + parent match) is **unchanged**.
- C3. The candidate-set seed is the deterministic `admission_epoch_seed` (grandparent hash; genesis
  at the activation boundary), node-recomputed; a mismatch is rejected (phase21d).
- C4. No change to PoW, LWMA, difficulty, target, block reward, or finality threshold.
- C5. PoAW-X is hard-off on mainnet (`network_id == 0`); none of the Phase 26 paths engage there.

### P2P invariants
- P1. Served admissions are sent only with block bodies the node is already serving; bounded to
  `≤ 16 × served_block_count` per response.
- P2. The receiver re-validates every admission via `ingest_bytes` (network match + signature/digest/
  seed/true-VRF) before storing; bad records are dropped, not connected.
- P3. No new unsolicited gossip flood; no new request type; mainnet sends nothing (no admissions).

### Storage invariants
- S1. The admission snapshot lives under the configured isolated data root (never `/tmp`, never a
  default `.irium`), written atomically (temp + rename), bounded by the (pruned) cache size.
- S2. Reload re-validates every record; corrupt/truncated/wrong-network/tampered records are skipped
  without crashing the node.

### Validation invariants
- V1. phase21e equality (`cs == admitted_candidate_set`) logic is byte-for-byte unchanged in all
  three phases; persistence/serving only change *availability* of already-validated admissions.
- V2. Reloaded/served admissions pass the same validation as live-gossiped ones.

## Known non-goals

- Not production-ready, not mainnet-ready, not audited.
- phase21e remains propagation-sensitive ("admitted to THIS node"), a pre-existing devnet/testnet
  limitation — not addressed or weakened here.
- Admission-window tuning for very deep public-network syncs is future work (per-getblocks-batch
  serving keeps each request within the window).
- The hidden-precommit / role-ticket-proof / mode-1 delegation paths are out of scope (separately
  tested, unchanged).

## Test matrix (repo-local, `cargo test --lib -- --test-threads=1`)

| Phase | Suite total | Phase-specific tests |
|------|-------------|----------------------|
| 26B | 744 / 0 | `phase26b_multiblock_epoch_seed_soak` (6-block chain), `phase26b_stale_immediate_parent_seed_rejected`, `phase26b_committed_admission_root_and_replay_rejected` |
| 26D | 747 / 0 | `phase26d_cold_replay_with_persisted_admissions`, `phase26d_persist_reload_roundtrip`, `phase26d_reload_rejects_invalid_records` |
| 26E | 748 / 0 | `phase26e_fresh_sync_via_served_admissions` |

All release builds (`--release --bin iriumd --bin poawx-live-proof-harness`) passed. The suite must
be run serialized (`--test-threads=1`) because PoAW-X tests mutate process-global env + the global
admission cache; one pre-existing test lacks the shared env lock and is parallel-only flaky.

## Live validation matrix (devnet, loopback RPC, source-restricted cross-host P2P)

| Phase | Result | Evidence (summary) |
|------|--------|--------------------|
| 26C | PASS | 6 all-gates blocks mined/accepted/propagated across all three; same final height/tip/irx1; incl. a VPS-2-originated block. |
| 26D | PASS | restart/keep-storage: node reloaded persisted admissions and rebuilt the chain to height 6 from disk; H7 propagated. |
| 26E | PASS | fully-wiped brand-new node received served historical admissions, synced the 6-block chain from scratch (~45 s), matching tip/irx1; H7 received live. |

Mainnet/prod processes and the VPS-1 production pool were alive and untouched throughout; default
storage (`.irium`) untouched; UFW left unchanged. (Logs summarized; no raw machine-private logs or
secrets included.)

## Remaining blockers

- Independent audit (this package).
- Public testnet exposure.
- Governance / mainnet activation.

## What auditors should review first

1. The phase21e/phase21d equality + seed checks in `src/chain.rs validate_block_candidate_sets`, and
   `admission_epoch_seed` in `src/poawx_committed_admission.rs` (Appendix A).
2. `CandidateAdmissionV1::validate` + `NodeCandidateAdmissionCache::{ingest_bytes, reload_persisted_bytes,
   load_persisted, persist_snapshot}` in `src/poawx_admission.rs` (Appendix B/C).
3. `send_historical_admissions` and its four call sites in `src/p2p.rs` (Appendix C/D).
4. Confirm phase22a (`validate_block_committed_admission`) is unchanged from before `30bce64`.
