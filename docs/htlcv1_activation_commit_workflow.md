# HTLCv1 Activation Commit Workflow (Mainnet)

Status now: HTLC mainnet activation is OFF.

This document defines the exact commit workflow to activate HTLCv1 on mainnet when governance approval exists.

## 1. Activation Constant Location
- File: `src/activation.rs`
- Constant:

```rust
pub const MAINNET_HTLCV1_ACTIVATION_HEIGHT: Option<u64> = None;
```

## 2. Exact Future Edit (When Approved)
Replace `None` with the approved height:

```rust
pub const MAINNET_HTLCV1_ACTIVATION_HEIGHT: Option<u64> = Some(<APPROVED_HEIGHT>);
```

Do not edit any other activation source for mainnet.

## 3. Activation Commit Steps
1. Confirm governance/operator approval and chosen `<APPROVED_HEIGHT>`.
2. Edit only `src/activation.rs` constant for mainnet activation height.
3. Run verification:
   - `cargo test --lib`
   - `cargo test --bin iriumd -- --nocapture`
   - `cargo test --manifest-path tools/atomic-swap-coordinator/Cargo.toml -- --nocapture`
   - `cargo check --tests`
4. Commit with a dedicated message (single-purpose activation commit).
5. Push to GitHub from code-master host.
6. Build release artifacts from that commit and publish checksums.

## 4. Release + Upgrade Sequence
1. Announce release commit hash and activation height.
2. Operators/miners upgrade to that release before activation height.
3. Verify coverage and parity across major nodes.
4. Monitor chain health through activation window.

## 5. What NOT To Do
- Do not use `IRIUM_HTLCV1_ACTIVATION_HEIGHT` to activate mainnet.
- Do not mix unrelated changes into activation commit.
- Do not activate without rollout checklist completion.

## 6. Verification Checklist for Activation Commit
- [ ] `src/activation.rs` is the only consensus activation source changed.
- [ ] Mainnet constant is set to approved `Some(height)`.
- [ ] Test suite passed on activation commit.
- [ ] Release notes and operator notice include exact commit hash.
- [ ] Upgrade coverage threshold met before activation height.
