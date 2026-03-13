# Irium HTLCv1 Mainnet Activation Checklist (Final)

Status now: **NOT ACTIVATED**. This checklist is for approved execution only.

## A. Code and Build Readiness
- [ ] `main` includes HTLCv1 + automation hardening commit set.
- [ ] `IRIUM_HTLCV1_ACTIVATION_HEIGHT` is unset in all production node environments.
- [ ] Coordinator/public self-serve remain disabled unless explicitly enabled.
- [ ] Test suite green on release candidate commit:
  - [ ] `cargo test --manifest-path tools/atomic-swap-coordinator/Cargo.toml -- --nocapture`
  - [ ] `cargo test --lib`
  - [ ] `cargo test --bin iriumd -- --nocapture`
  - [ ] `cargo check --tests`

## B. Network Upgrade Readiness
- [ ] Operator acknowledgment collected from a majority of economically relevant nodes.
- [ ] Miner/pool upgrade acknowledgment collected.
- [ ] Public notice window completed (recommended minimum: 7 days).
- [ ] Final activation height selected with safety buffer (see runbook).

## C. Pre-Activation Operations
- [ ] Confirm live mainnet health (peer count, block propagation, no major forks).
- [ ] Confirm all intended activation nodes run identical commit hash from GitHub.
- [ ] Confirm no host runs from `/tmp` or unknown binary path.
- [ ] Snapshot/backup:
  - [ ] wallet files
  - [ ] node data dir metadata
  - [ ] coordinator DB (if used)
- [ ] Monitoring/alerts armed (see `docs/mainnet_activation_monitoring.md`).

## D. Activation Execution Gate (T-60 to T-0)
- [ ] NO-GO criteria reviewed and clear.
- [ ] ABORT criteria reviewed and clear.
- [ ] Operator on-duty matrix confirmed.
- [ ] Incident channel open.
- [ ] Activation env change prepared but not yet applied.

## E. At Activation Height
- [ ] Apply activation env only on approved hosts.
- [ ] Restart nodes with controlled staggering.
- [ ] Validate activation state on each node.
- [ ] Confirm new blocks continue and no chain split symptoms appear.

## F. Post-Activation (0-24h)
- [ ] Validate HTLC tx acceptance at/after activation.
- [ ] Validate pre-activation style transactions remain normal.
- [ ] Validate mempool/template/block connect consistency.
- [ ] Watch reject/error rates and peer churn.
- [ ] Publish status update to operators/miners/community.

## G. Sign-off
- [ ] Activation declared successful.
- [ ] Postmortem notes captured.
- [ ] Roll-forward actions logged.

Reminder: HTLCv1 remains OFF until an explicit activation-height rollout is approved and executed.
