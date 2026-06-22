# PoAW-X Phase 26 — Auditor Checklist

Companion to `poawx-phase26-independent-audit-package.md` and `...-technical-appendix.md`.
**NOT production-ready / mainnet-ready / audited.** Audited HEAD `0208368`; full source range
`30bce64..0208368`. No secrets/keys/wallet data included; logs summarized.

## How to start

```
git fetch origin
git checkout testnet/poawx-phase20-blueprint-completion-local
git rev-parse HEAD            # expect 0208368
git ls-remote origin main     # expect 19c496dc5f2fa08981a109b10eeb257105c28c43 (unchanged)
git diff 30bce64..0208368 -- 'src/*.rs'   # the full source change to review
```

Review order: `src/poawx_committed_admission.rs` (`admission_epoch_seed`) → `src/chain.rs`
`validate_block_candidate_sets` (phase21d/21e) → confirm `validate_block_committed_admission`
(phase22a) is unchanged vs `30bce64` → `src/poawx_admission.rs` (validate + persistence) →
`src/p2p.rs` (`send_historical_admissions` + 4 call sites) → `src/bin/iriumd.rs` startup hook →
`src/storage.rs` path.

## Questions the auditor should answer

### Seed reconciliation (Appendix A)
1. Does the epoch-seed alignment preserve **both** phase21d and phase22a? Specifically: is
   `validate_block_committed_admission` byte-for-byte unchanged vs `30bce64`, and does phase21d still
   enforce an exact node-recomputed seed plus canonical / best-for-role / dominance / AVR2 / admitted-
   set checks?
2. Is `admission_epoch_seed` correct and non-circular at every height, including the genesis/activation
   boundary (H1, H2) and `H ≥ 3`? Can a producer influence its own candidate-set seed?
3. Does the grandparent-seeded candidate set reduce any VRF unpredictability that matters, or only
   shift determinism one block earlier (as intended by commit-one-ahead)?

### Admission integrity (Appendix B/C/D)
4. Can a candidate admission be **forged, replayed, or cross-network reused**? Does the digest bind
   `(network, height, seed, candidate[, V2])`, and do `ingest_bytes` / `reload_persisted_bytes` reject
   wrong-network, tampered, and wrong-height/seed records?
5. Are the cache **keys** `(target_height, role_id, solver_pkh)` sufficient to prevent conflicting or
   duplicate admissions, and does the conflict check reject a second distinct admission per key?
6. Can a node be tricked into **accepting a block without a matching, validated candidate admission**?
   (i.e. is phase21e equality still the gate, with no bypass via persistence or serving?)

### Persistence safety (Appendix B)
7. Is admission persistence safe against **corruption and stale state**? Atomic write (temp+rename);
   truncated/garbage tail handling; revalidation on reload; bounded size; correct dir (data root, not
   `/tmp`/`.irium`)?
8. Could a crafted `candidate_admissions.dat` crash the node or smuggle an unvalidated admission?

### P2P / DoS (Appendix D)
9. Can **historical admission serving be abused for DoS**? Is `≤ 16 × served_block_count` a sufficient
   bound, is the existing serve-path rate-limiting adequate under adversarial getblocks patterns, and
   is the re-broadcast dedup sufficient to prevent amplification?
10. Are **block bodies still fully validated** after this change (no trust shortcut introduced by
    receiving admissions first)?
11. Does **fresh sync work without trusting peers beyond normal validation** (delivery-only trust;
    every admission + block independently re-validated)?

### Bounds / windows
12. Are the bounds (per-response admission cap, `ADMISSION_SEEN_CAP`, `ADMISSION_PRUNE_KEEP`,
    `candidate_admission_window = 64`) sufficient, and does the window cover per-getblocks-batch sync
    while not over-accepting future-height admissions?

### Mainnet (Appendix E)
13. Does **mainnet remain unaffected**? Confirm PoAW-X is hard-off for `network_id == 0`, no mainnet
    activation/default-behavior change, no default-storage tests, no wallet/key use.

### Tests
14. Are the tests **meaningful and sufficient**? Do they exercise the real `connect_block` path, the
    positive 6-block chain, and the negative cases (missing/tampered/wrong-network/replayed)? What
    additional adversarial / property / fuzz tests would the auditor recommend?

## Test evidence summary (repo-local, `cargo test --lib -- --test-threads=1`)

- Phase 26B: **744 / 0**. (`phase26b_multiblock_epoch_seed_soak` + 2 negatives.)
- Phase 26D: **747 / 0**. (`phase26d_cold_replay_with_persisted_admissions` + 2 admission tests.)
- Phase 26E: **748 / 0**. (`phase26e_fresh_sync_via_served_admissions`.)
- Release builds (`cargo build --release --bin iriumd --bin poawx-live-proof-harness`): passed each
  phase.
- The suite is run **serialized** (`--test-threads=1`): PoAW-X tests mutate process-global env + the
  global admission cache; one pre-existing test lacks the shared env lock and is parallel-only flaky
  (passes in isolation and serialized). **No gate was disabled/delayed to pass tests.**

## Live validation evidence summary (devnet; loopback RPC; source-restricted cross-host P2P)

- Phase 26C: 6-block three-system propagation **passed** (same final height/tip/irx1; a
  VPS-2-originated block included).
- Phase 26D: restart / keep-storage cold replay **passed** (chain rebuilt to height 6 from disk via
  reloaded admissions) + H7 received.
- Phase 26E: fully-wiped fresh node sync **passed** (synced 6-block chain from scratch via served
  admissions) + H7 received.
- Mainnet/prod processes and the production pool were alive and untouched; default storage untouched;
  firewall (UFW) unchanged. Logs are summarized; no raw machine-private logs or secrets are included.

## Out of scope / remaining

- Independent audit (this package), public testnet, governance / mainnet activation.
- phase21e "admitted to THIS node" honest limitation (pre-existing, unchanged).
- hidden-precommit / role-ticket-proof / mode-1 delegation (separately tested, unchanged).
