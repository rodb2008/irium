# HTLCv1 PR Summary (Activation-Gated, Parked)

## What HTLCv1 Adds
- Minimal native HTLC output/witness support for atomic-swap primitives:
  - Claim path: valid recipient sig + SHA256 preimage match
  - Refund path: valid refund sig + absolute block-height timeout
- RPC surface:
  - `createhtlc`, `decodehtlc`, `claimhtlc`, `refundhtlc`, `inspecthtlc`
- Activation-gated behavior via `IRIUM_HTLCV1_ACTIVATION_HEIGHT`.

## Why It Is Safe to Park on Main (Activation OFF)
- Default behavior when activation height is unset: HTLCv1 disabled.
- Legacy transaction and P2PKH behavior unchanged in this default state.
- No automatic activation behavior and no hidden runtime dependency.

## What Was Tested
- Activation boundary tests (`N-1` reject, `N/N+1` allow).
- Miner/template inclusion at activation boundary.
- Multi-node dev trial:
  - post-activation funding inclusion,
  - valid claim inclusion,
  - wrong-preimage rejection,
  - refund-before-timeout rejection,
  - refund-after-timeout success.

## What Is Not Yet Built
- Swap coordinator/orchestration layer.
- Cross-chain automation/routing/orderbook.
- GUI/operator workflow.
- Exhaustive reorg simulation harness for HTLC lifecycle.

## Future Activation Checklist
1. Keep mainnet activation unset by default in shipped configs.
2. Run extended devnet/testnet soak with reorg and persistence matrix.
3. Publish activation proposal with exact height and rollback plan.
4. Verify node/operator readiness and monitoring.
5. Activate only after explicit coordination.

## Rollback / Disable Note
- If issues arise pre-activation: keep `IRIUM_HTLCV1_ACTIVATION_HEIGHT` unset.
- If parked on main and not activated, operational rollback is config-only (no consensus behavior change active).
