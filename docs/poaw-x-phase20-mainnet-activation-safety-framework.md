# PoAW-X Phase 20 — Mainnet Activation Safety Framework (NO activation)

**Status:** Framework + checklists COMPLETE. **No mainnet activation is performed or
scheduled by this phase.** Mainnet PoAW-X mode-1 is **hard-disabled** in code today and
stays so until an explicit, far-future activation height is set by a future approved change.

## 1. Current safety posture (already in code + tested)
- `connect_block` **hard-rejects** any mainnet block carrying delegated (mode-1) receipts,
  **regardless of env** (`src/chain.rs`: mainnet branch returns error before mode-1 handling).
- `poawx_delegation_active(height)` returns **false on mainnet** unconditionally; testnet/
  devnet gate on `IRIUM_POAWX_DELEGATION_ACTIVATION_HEIGHT`.
- Regression test exists: `mode-1 on mainnet must hard-reject`.
- `fee_bps > 0` fails closed everywhere; official pool fee is 0%.

## 2. Activation invariants (must all hold before mainnet mode-1 is ever enabled)
- Activation requires an **explicit future height** in consensus code — not an env var, not a
  pool config, not a CLI flag.
- **Default mainnet PoAW-X = OFF.** No env accident, no config accident can enable it.
- Mainnet mode-1 stays **hard-rejected** until the code-level activation height is reached.
- The activation height must be **far future** (ample upgrade/announcement lead time).
- The activation change itself is a separately reviewed, approved, and (only then) released
  consensus change — never bundled silently.

## 3. Default-off regression coverage
The default-off / hard-reject behavior is covered by the existing mainnet hard-reject test and
the testnet/devnet activation-height gate. Before any activation work:
- Add/confirm a test asserting that with **no** activation env and mainnet context, mode-1 is
  rejected (default OFF), and that a non-mainnet activation env does **not** affect mainnet.
- (Phase 20 keeps mainnet gating untouched; these tests are the entry criteria for the future
  activation change, not part of this no-activation pass.)

## 4. Operator checklist (future activation — not now)
- [ ] Consensus spec frozen + community-reviewed (governance doc).
- [ ] Reproducible binary built; published hash; operators verified running hash.
- [ ] Far-future activation height chosen + announced with lead time.
- [ ] Monitoring in place (metrics doc); rollback rehearsed.
- [ ] Both seed nodes + prod pool confirmed healthy and isolated.
- [ ] Final approval recorded.

## 5. Rollback plan (future)
- Before the height: cancel/postpone via a new release/announcement; nodes default-off.
- After the height (if critical issue): coordinated halt/upgrade per incident response; never
  a silent change.

## 6. Monitoring plan (future)
- Per `poaw-x-phase20-metrics-monitoring.md`: track accepted mode-1 blocks, rejects by reason,
  reward-split/identity violations (must stay 0), peer sync; loopback/operator-restricted.

## 7. Public communication plan (future)
- Announce the height, the binary hash, the upgrade deadline, and the rollback policy
  (template in the governance doc).

## 8. Final approval checklist (future)
- [ ] All design gaps resolved (fairness, reward split, fee) or explicitly out of scope.
- [ ] Testnet + external pilot + public testnet success criteria met.
- [ ] Security/consensus-boundary audit signed off.
- [ ] Explicit owner approval to set the activation height and push/release.

> **Phase 20 performs none of the above.** It documents the framework and confirms the
> existing hard-disabled posture. Chain difficulty remains automatic via LWMA-144.
