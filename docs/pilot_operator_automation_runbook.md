# Pilot Operator Automation Runbook

## Pre-open
- Verify coordinator health: `GET /healthz`
- Verify invite code list and operator token configuration.
- Confirm intake state (`intake_paused=false`).

## During window
- Watch live non-terminal swaps: `GET /v1/admin/swaps` with `x-operator-token`.
- For suspicious swaps, set manual review: `POST /v1/admin/swaps/{id}/manual-review`.
- Pause intake if needed: `POST /v1/admin/intake` with `{ "paused": true }`.

## Incident response
- Pause intake immediately.
- Mark affected swap(s) manual-review.
- Capture swap events and RPC logs.
- Resume only after root-cause confirmation.
