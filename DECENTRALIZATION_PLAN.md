# Irium Network Decentralization Plan

## Current Risk
- Single VPS (207.244.247.86) as primary bootstrap node
- If VPS goes down, new nodes cannot join network
- Network survives but becomes isolated

## Solution: Multi-Node Resilience

### 1. Dynamic Seedlist
✅ Already implemented: bootstrap/seedlist.runtime
- Nodes share peer lists automatically
- Runtime seedlist updates with discovered peers
- Survives single node failure

### 2. Multiple Bootstrap Nodes Needed
Current seedlist has:
- 207.244.247.86:38291 (your VPS)
- 178.78.34.62:38291 (external peer)
- 50.5.78.17:38291 (external peer)

Status: ✅ Multiple bootstrap nodes available

### 3. Peer Persistence
✅ Implemented: state/peers.json
- Nodes remember previously connected peers
- Reconnect automatically on restart
- No dependency on seedlist after first connection

### 4. Block Propagation
✅ Implemented: PUSH-based broadcasting
- Blocks relay to all connected peers
- Bidirectional synchronization
- Network continues even if bootstrap offline
