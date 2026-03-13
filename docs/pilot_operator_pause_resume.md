# Pilot Pause/Resume Controls

## Pause intake
`POST /v1/admin/intake`
Body: `{ "paused": true }`
Header: `x-operator-token: <token>`

## Resume intake
`POST /v1/admin/intake`
Body: `{ "paused": false }`

## Manual review toggle
`POST /v1/admin/swaps/{id}/manual-review`
Body: `{ "manual_review": true|false }`

## Notes
- Pausing intake does not cancel existing swaps.
- Manual review blocks auto-progression for that swap.
