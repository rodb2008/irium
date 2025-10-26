# Irium Automatic Update Notification System

## Design Goals
- Alert miners when running outdated versions
- Provide update instructions automatically
- No forced updates (security risk)
- Miners can opt-in to auto-updates

## Implementation Plan

### Phase 1: Enhanced Version Detection (Immediate)
1. P2P handshake already exchanges versions
2. Add persistent version tracking
3. Log and alert on version mismatch
4. Show update commands to miners

### Phase 2: Update Notification Service
1. Check GitHub for latest release
2. Compare with local version
3. Display update notification
4. Provide one-command update script

### Phase 3: Optional Auto-Update (Opt-in)
1. Miners enable auto-update flag
2. System downloads and verifies new version
3. Restarts services automatically
4. Rollback on failure
