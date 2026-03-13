# HTLCv1 Mainnet Activation Plan (Code-Defined Model)

## Scope
This plan describes how to activate HTLCv1 on mainnet without runtime env activation.

## Activation Mechanism
- Mainnet activation is controlled only by code constant:
  - `src/activation.rs` → `MAINNET_HTLCV1_ACTIVATION_HEIGHT`
- Activation is effective when released nodes run code with `Some(<height>)`.

## Current Status
- Constant is currently `None`.
- Mainnet HTLC remains OFF.

## Pre-Activation Requirements
- operator/miner readiness window complete
- monitoring/abort criteria approved
- release candidate test suite green
- final activation height approved

## Execution Steps
1. Set activation constant in code to approved height.
2. Build and tag release.
3. Publish upgrade notice (operators/miners/community).
4. Operators upgrade binaries before activation height.
5. Observe activation at chain height without env changes.

## Non-Mainnet Testing
- `IRIUM_HTLCV1_ACTIVATION_HEIGHT` may be used for testnet/devnet only.
- This env path must not be treated as mainnet activation path.

## Operator Requirement
- Upgrade software to the release containing the approved activation height.
- Do not rely on env activation for mainnet.
