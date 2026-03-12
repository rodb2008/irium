# Pilot Access Control

Implemented controls:
- Invite-code gate for session creation (`COORDINATOR_INVITE_CODES`).
- Session-token gate for public swap read/status/events.
- Operator-token gate for admin endpoints.

Recommended policy:
- Rotate invite codes per pilot window.
- Rotate operator token per deployment.
- Do not expose admin token to testers.
