# Irium Core GUI Blueprint

This file mirrors the implementation blueprint provided for the Irium Core GUI (Tauri).
Refer to it when extending the app beyond the current alpha scaffold.

## Summary
- Desktop full-node wallet GUI for Irium.
- Managed node mode (launches iriumd) + attach mode.
- Wallet creation, receive, send, basic explorer.
- Local-only, no external services, no CDN assets.

## MVP features
- Node: start/stop, sync status, peers, log tail.
- Wallet: create + unlock (passphrase in memory), receive + QR, send, recent tx list.
- Explorer: latest blocks, block detail, tx detail.
- Settings: data dir, RPC endpoint/token, managed/attach.

Full blueprint text is available in the project prompt history.
