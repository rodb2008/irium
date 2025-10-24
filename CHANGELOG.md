# Irium Changelog

All notable changes to the Irium blockchain will be documented in this file.

## [v1.1.9] - 2025-10-22

### Added
- PUSH-based bidirectional block broadcasting
- NAT traversal with adaptive ping (60s public, 30s NAT)
- Complete protocol implementation (GetHeaders, Headers, Mempool)

### Fixed
- Ghost peer bug (address correction in handshake)
- Duplicate message handlers
- Connection stability issues
- Hardcoded VPS-specific values

### Changed
- Max peers: 8 → 8000 (production scale)
- Peer timeout: 300s → 180s
- Ping interval: Adaptive (60s/30s)
- Message timeout: 120s → 180s

## [v1.1.8] - 2025-10-20

### Added
- Complete blockchain verification
- Enhanced .gitignore for security
- Multi-core mining verification

### Fixed
- P2P handshake address variable scope bug
- Bootstrap seedlist management

---

See individual RELEASE_NOTES_*.md files for detailed information.

### Fixed
- **Critical: Multicore mining coordination**
  - Fixed parameter/method bugs in simple miner (TxOutput, Transaction, BlockHeader, Target)
  - Reduced node block rescan from 30s → 5s for faster block detection
  - Added periodic height checks every 10k nonces to prevent stale block mining
  - Added unbuffered output for real-time logging
  - Miners now stop immediately when another core finds the block
  - Dramatically improved multicore mining efficiency

