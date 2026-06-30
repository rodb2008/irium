# PoAW-X Phase 26 — One-Page Summary for the Auditor

**NOT audited. NOT production-ready. NOT mainnet-ready.** Mainnet PoAW-X is hard-off
(`network_id == 0`). No public testnet has launched.

## What PoAW-X is

A multi-role proof-of-aligned-work consensus overlay for the Irium node, enforced by gated sections
inside the node's `connect_block` validation pipeline (dominance, candidate-set/admission, puzzle,
finality, **committed-admission**, true-VRF). It runs only on non-mainnet networks; the node
independently validates every block — block builders are not trusted authorities.

## What changed in Phase 26

1. **Epoch-seed alignment (26B)** — the candidate-set gate (phase21d/21e) now expects the *epoch seed*
   (`admission_epoch_seed` = grandparent hash; genesis at the activation boundary) instead of the
   immediate parent hash. This reconciles phase21d/21e with the committed-admission gate (phase22a)
   so multi-block chains are satisfiable. **phase22a is unchanged; the phase21e equality check is
   unchanged** — only the *expected seed value* moved.
2. **Admission persistence (26D)** — already-validated candidate admissions are persisted to an
   isolated data-root file and **re-validated on reload** at startup, fixing restart cold-resync.
3. **Historical-admission serving (26E)** — when serving block bodies during sync, a node also sends
   the matching admissions; the **receiver re-validates each** via the normal ingest path, enabling a
   fresh node to sync from scratch. The send is **bounded** (`16 × served_block_count`).

Surface: `30bce64..0208368`, 8 source files, +1006/−47.

## What was tested

- Full lib suite **748 passed / 0 failed** (`cargo test --lib -- --test-threads=1`); release build of
  `iriumd` + `poawx-live-proof-harness` succeeds.
- Targeted positive + negative tests: multi-block epoch-seed soak; stale-seed rejection; tampered
  commitment/replay rejection; cold-replay with persisted admissions (phase21e rejects empty cache);
  fresh-sync via served admissions (rejects tampered).
- Prior phases live-validated 6–7 block, three-node devnet runs (summarized; loopback RPC,
  source-restricted P2P).
- Internal self-review (Phase 26I) confirmed phase22a byte-unchanged and PoW/LWMA/reward/constants
  untouched. **The self-review is not an independent audit.**

## What remains unknown (for the auditor)

- Cryptographic soundness of admission signatures/digests and VRF outputs (treated as opaque here).
- Adversarial, multi-operator, and scale behavior (no adversarial/scale testing).
- Real-network DoS bounds under sustained load (send is bounded in code, not load-tested).
- The pre-existing phase21e propagation-sensitivity property ("admitted to THIS node in the window").
- Independent re-derivation of the full change surface.

## The exact audit ask

Independently verify that the Phase 26 changes **do not weaken any validation gate** — specifically:
phase22a unchanged; phase21e equality still required; no block accepted without a matching validated
admission; admissions cannot be forged/replayed/cross-network-reused; persistence/serving are
corruption-safe and DoS-bounded; mainnet remains unaffected. Deliver findings (with severity +
exploitability), recommended fixes, retest requirements, and a scoped sign-off / non-sign-off.

## Where to start

`PACKAGE_MANIFEST.md` → `docs/audit/phase26h-kickoff/README.md` → `AUDIT_SCOPE.md` →
`AUDITOR_REVIEW_GUIDE.md` → `REPRO_COMMANDS.md`. Record findings in `EXTERNAL_FINDINGS_TRACKER_COPY.md`
(or `phase26h-kickoff/FINDINGS_TRACKER.md`).

## Claims disclaimer

This summary and the accompanying package make **no** claim that the system is audited,
production-ready, or mainnet-ready. An independent scoped sign-off is the only basis for an "audited"
statement and is a prerequisite for any public-testnet launch.
