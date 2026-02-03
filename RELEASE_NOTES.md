# Irium Core v0.1 Alpha Release Notes

## What this is
Irium Core is the reference full-node wallet desktop app for the Irium blockchain. This Alpha focuses on:
- Managed local node start/stop
- Wallet create/unlock/receive/send via node-managed RPC
- Basic local explorer (blocks + tx lookup)
- Log viewer and node status

## Verify downloads
`sha256sum -c SHA256SUMS.txt`
SHA256SUMS.txt is available on the GitHub release page alongside the installers.

## Windows install (NSIS)
1. Download `Irium_Core_v0.1_Alpha_0.1.0_x64-setup.exe`.
2. Run the installer and follow the prompts.
3. Launch Irium Core, then complete first-run setup:
   - Choose data directory
   - Set RPC token
   - Start node

Note: Windows Defender/SmartScreen may warn for unsigned Alpha builds. This is expected for early releases.

## Linux AppImage
1. Download `Irium_Core_v0.1_Alpha_0.1.0_amd64.AppImage`.
2. Make it executable:
   - `chmod +x Irium_Core_v0.1_Alpha_0.1.0_amd64.AppImage`
3. Run it:
   - `./Irium_Core_v0.1_Alpha_0.1.0_amd64.AppImage`

## Known limitations (Alpha)
- Feature set is limited to the MVP (no multi-wallet UI, no hardware wallet).
- Some fields may show `--` if the local node is still syncing.
- Explorer view is local-only and limited to recent data.
- UI/UX is still in flux; expect changes between Alpha builds.
