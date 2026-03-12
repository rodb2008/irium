# Pilot Self-Serve Flow

Scope: selected testers only, BTC testnet + IRM trial only.

## Tester flow
1. `POST /v1/public-swaps` with `tester_handle`, `btc_testnet_receive_address`, `invite_code`.
2. Coordinator returns `swap_id`, `session_token`, `state`, `next_action`.
3. Tester polls `GET /v1/public-swaps/{id}?token=...` and `.../status`.
4. Tester sends BTC testnet funds to returned `btc_htlc_address`.
5. Tester either submits txid (`POST /v1/public-swaps/{id}/submit-btc-txid`) or waits for auto-detect.
6. Coordinator tracks confirmations and auto-progresses state.
7. Tester sees terminal outcome: `claimed` / `refunded` / `failed` / `expired`.

## Safety controls
- Intake can be paused by operator.
- Per-swap manual-review mode supported.
- Invite code gate enabled by default.
- No private key storage in coordinator.

## Mainnet safety
HTLCv1 stays OFF by default on Irium mainnet. No mainnet activation is performed by this flow.
