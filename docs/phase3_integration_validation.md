# Phase 3 Integration Validation and UX Hardening

## Overview

This document covers the integration validation and UX improvements applied to the `irium-wallet` CLI as part of Phase 3 of the settlement proof system.

## UX Improvements

### `flow_next_step_hint` helper

A new function `flow_next_step_hint(outcome, release_eligible, refund_eligible)` centralises the logic for guiding the operator to the correct next command after evaluating a settlement policy. It returns a static string based on the evaluation result:

| Condition | Hint |
|-----------|------|
| `release_eligible = true` | run `agreement-release-build` |
| `refund_eligible = true` | run `agreement-refund-build` |
| `outcome = "satisfied"` (not executable) | re-run after holdback height |
| `outcome = "timeout"` | run `agreement-refund-build` |
| any other | run `agreement-proof-create` then `agreement-proof-submit` |

### `next_step` output fields

The following render functions now emit a `next_step` line:

- `render_policy_evaluate_summary` — uses `flow_next_step_hint` to produce context-aware guidance
- `render_policy_set_summary` — hints toward `agreement-policy-evaluate` when the policy is accepted
- `render_proof_create_summary` — hints toward `agreement-proof-submit`
- `render_proof_submit_summary` — hints toward `agreement-policy-evaluate` when accepted or duplicate

### Bug fixes

**`agreement-policy-evaluate` exit code**: The handler previously called `std::process::exit(1)` when neither `release_eligible` nor `refund_eligible` was true. This made it impossible to script the command in a loop waiting for policy satisfaction. The spurious `exit(1)` has been removed; the command now exits 0 in all non-error cases.

**`policy-build-otc` unknown argument swallow**: The catch-all match arm `_ => { i += 1; }` silently ignored unrecognised arguments, masking typos in flag names. It now prints an error message and exits 1.

## Integration Tests

Nine new tests cover the UX changes:

- `flow_next_step_hint_release_eligible` — release path produces `release-build` hint
- `flow_next_step_hint_refund_eligible` — refund path produces `refund-build` hint
- `flow_next_step_hint_satisfied_not_executable` — holdback hint when satisfied but not executable
- `flow_next_step_hint_timeout_no_eligible` — timeout hint with no eligible path
- `flow_next_step_hint_pending` — pending state guides toward proof creation
- `render_policy_evaluate_includes_next_step` — output contains `next_step` field
- `render_policy_evaluate_release_eligible_next_step` — release hint in evaluate output
- `render_policy_set_summary_accepted_has_next_step` — accepted policy response has `next_step`
- `render_policy_set_summary_rejected_no_next_step` — rejected policy response has no `next_step`

## Validation

```
cargo fmt        # clean formatting
cargo check      # 0 errors, 6 pre-existing warnings
cargo test --bin irium-wallet  # 185 passed, 0 failed
```

## End-to-end Flow Reference

### Flow 1: OTC trade

```
irium-wallet otc-create --seller <addr> --buyer <addr> --amount 10.0 --asset BTC --payment-method bank_transfer --timeout 2016
irium-wallet otc-attest --agreement <hash> --message "payment confirmed" --address <attestor>
irium-wallet otc-settle --agreement <hash>
irium-wallet otc-status --agreement <hash>
```

Each command prints `next_step` guidance pointing to the following command.

### Flow 2: Deposit protection

```
irium-wallet agreement-create-from-template --template deposit-protection --buyer <addr> --seller <addr> --amount 5.0 --asset item_id_42
irium-wallet agreement-policy-set ...
irium-wallet agreement-proof-create ...
irium-wallet agreement-proof-submit ...
irium-wallet agreement-policy-evaluate ...
```

### Flow 3: Milestone payment

```
irium-wallet agreement-create-from-template --template milestone-payment --buyer <addr> --seller <addr> --amount 20.0 --milestones 4
```

Each milestone advances independently through the proof → evaluate → release cycle.

### Flow 4: Remote attestor

```
irium-wallet proof-sign --agreement <hash> --message "delivery confirmed" --address <attestor> --json > proof.json
irium-wallet proof-submit-json < proof.json
```

The signed proof envelope is compact JSON suitable for transport over any out-of-band channel.
