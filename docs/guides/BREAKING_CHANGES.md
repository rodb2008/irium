# Breaking Changes

## v1.3.0 (Hard Fork - October 25, 2025)

**⚠️ This is a HARD FORK. All nodes must upgrade.**

### Consensus Changes:
- Coinbase maturity enforced (100 blocks)
- Timestamp validation (max 2h future, strictly increasing)
- Transaction signature verification required

### Migration:
Nodes running v1.1.9 or earlier MUST:
1. Upgrade to v1.3.0
2. Clear chainstate: `rm -rf ~/.irium/chainstate/*`
3. Restart services

### Incompatibility:
- v1.3.0 nodes will REJECT blocks from older versions
- Older nodes will REJECT blocks from v1.3.0
- Running old code will result in network fork

---

## v1.1.9 and Earlier

These versions are **DEPRECATED** and incompatible with the current network.
