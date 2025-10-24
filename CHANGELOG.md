# Irium Changelog

All notable changes to the Irium blockchain will be documented in this file.

## [v1.2.0] - 2025-10-24

### Added
- Runtime seedlist registration (incoming + outgoing peers)
- Block relay spam prevention (tracking system)
- Self-connection detection (public IP support)
- Nginx API configuration for Explorer and Wallet APIs
- NAT environment full support verification

### Fixed
- IP:PORT deduplication (was IP-only, now IP:PORT)
- Peer height tracking after block reception
- Block relay spam loop
- Self-connection detection for NAT nodes

### Changed
- API Infrastructure: All endpoints documented and verified
- Documentation: Updated README, QUICKSTART, WHITEPAPER with NAT details
- Repository: Cleaned up website assets (moved to gh-pages)

### Verified
- NAT miners fully functional (same as Bitcoin behavior)
- 4+ stable peer connections in production
- Block propagation across mixed public/NAT topology

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

See individual RELEASE_NOTES_*.md and CHANGELOG_v*.txt files for detailed information.
