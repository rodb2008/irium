# Issues Tracker for v1.2.0 (DO NOT PUSH YET)

## Known Issues (Collecting)

### 1. Duplicate Peer Connections
- **Status:** Known issue
- **Impact:** Medium (causes log spam)
- **Fix planned:** Improve peer deduplication logic

### 2. User Block Broadcasting
- **Status:** User hasn't restarted services
- **Impact:** Medium (blocks stuck on user's machine)
- **Fix planned:** Better user documentation

### 3. [Add more as reported]

## Planned Improvements

- [ ] Better error messages for users
- [ ] Auto-restart detection
- [ ] Improved sync logging
- [ ] [Add more]

## Target: v1.2.0 release in 1 week

**DO NOT PUSH TO GITHUB UNTIL READY!**

### 3. Block Mined Without Miner Address (CRITICAL!)
- **Status:** Block 28 has `miner: null`
- **Impact:** HIGH - No reward, invalid block
- **Cause:** Miner code not setting miner_address field
- **Fix planned:** Validate block has miner_address before saving
- **Evidence:** block_28.json has miner: null

### 4. Node Doesn't Auto-Reload Chain Height
- **Status:** Node shows old height until restarted
- **Impact:** Medium - Sync appears broken
- **Fix planned:** Hot-reload chain height when new blocks saved

