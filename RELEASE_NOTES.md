# Irium Core v0.1 Alpha Release Notes

## What this is
Irium Core is the reference full-node wallet desktop app for the Irium blockchain. This Alpha focuses on:
- Managed local node start/stop
- Wallet create/unlock/receive/send via node-managed RPC
- Basic local explorer (blocks + tx lookup)
- Log viewer and node status

## How to run (Linux AppImage)
1. Download the AppImage.
2. Make it executable: `chmod +x Irium_Core_v0.1_Alpha_*.AppImage`
3. Run it: `./Irium_Core_v0.1_Alpha_*.AppImage`

## Known limitations (Alpha)
- Feature set is limited to the MVP (no multi-wallet UI, no hardware wallet).
- Some fields may show `--` if the local node is still syncing.
- Explorer view is local-only and limited to recent data.
- UI/UX is still in flux; expect changes between Alpha builds.
