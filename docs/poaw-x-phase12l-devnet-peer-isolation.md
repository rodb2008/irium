# Phase 12-L: Devnet P2P Seed Isolation

**Status:** Complete
**Branch:** testnet/poawx-phase12-completion-rc-hardening
**Date:** 2026-06-12
**Root cause fixed:** devnet nodes dialing mainnet P2P seeds (port 38291) instead of each other

---

## 1. Root Cause

When `IRIUM_DATA_DIR` points to a path not under `$HOME` (e.g. `/tmp/...`),
`storage::configured_dir()` rejects it and `bootstrap_dir()` silently falls back
to `~/.irium/bootstrap/`. That directory contains the mainnet runtime seed list
with port-qualified entries like `207.244.247.86:38291`. Because
`parse_seed_to_socketaddr` uses the embedded port directly, devnet nodes dialled
VPS-1 mainnet P2P (38291) instead of the devnet port (39510), resulting in:

- VPS-2 devnet connected to VPS-1 mainnet P2P
- Received mainnet headers (height ~30,500) that devnet could not apply
- VPS-2 devnet chain stayed at height 0, P2P sync never happened

---

## 2. Fix: Three Network-Aware Guards in `src/bin/iriumd.rs`

### Change 1 — `load_persisted_startup_seeds`: skip runtime seeds for non-mainnet

```rust
// Mainnet only: don't import mainnet runtime seeds into devnet/testnet nodes.
if network_kind_from_env() == NetworkKind::Mainnet {
    for seed in load_runtime_seeds() { ... }
}
```

### Change 2 — `build_seed_addrs`: skip fallback seeds for non-mainnet

```rust
// Devnet/testnet must not fall back to mainnet bootstrap peers.
let fallback_seeds = if network_kind_from_env() == NetworkKind::Mainnet {
    load_builtin_fallback_seeds()
} else {
    Vec::new()
};
```

### Change 3 — `build_seed_addrs`: skip signed seeds for non-mainnet

```rust
// Devnet/testnet: skip signed (mainnet-only) seed list entirely.
let signed_seeds =
    if network_kind_from_env() != NetworkKind::Mainnet || load_runtime_seeds().len() >= 5 {
        Vec::new()
    } else {
        load_signed_seeds()
    };
```

---

## 3. Tests Added (5 new, in `src/bin/iriumd.rs` test module)

| Test | What it verifies |
|------|-----------------|
| `test_12l_devnet_network_kind_is_not_mainnet` | `IRIUM_NETWORK=devnet` is not classified as Mainnet |
| `test_12l_testnet_network_kind_is_not_mainnet` | `IRIUM_NETWORK=testnet` is not classified as Mainnet |
| `test_12l_default_network_kind_is_mainnet` | Unset `IRIUM_NETWORK` defaults to Mainnet |
| `test_12l_devnet_fallback_seed_gate_returns_empty` | Fallback seed gate returns empty for devnet |
| `test_12l_devnet_signed_seed_gate_skipped` | Signed seed gate returns empty for devnet |

All 5 pass; cargo build 0 errors; 236 tests pass total (VPS-1).

---

## 4. Devnet Startup Fix: Isolated Data Directory

`IRIUM_DATA_DIR` must be set to a path **under `$HOME`** so that
`configured_dir()` accepts it and devnet gets its own isolated blocks directory.

| Node | `IRIUM_DATA_DIR` | `IRIUM_STATE_DIR` | P2P port |
|------|-----------------|------------------|----------|
| VPS-1 devnet | `/home/irium/irium-devnet-data-vps1` | `/home/irium/irium-devnet-state-vps1` | `0.0.0.0:39510` |
| VPS-2 devnet | `/home/irium/irium-devnet-data-vps2` | `/home/irium/irium-devnet-state-vps2` | `0.0.0.0:39514` |

Also: iriumd must be started from the repo root (`cd /home/irium/irium`) so that
`./bootstrap/trust/allowed_anchor_signers` resolves correctly.

Correct env var names for HTTP binding:
- `IRIUM_NODE_PORT` (not `IRIUM_RPC_BIND`) — default 38300
- `IRIUM_STATUS_PORT` (not `IRIUM_STATUS_BIND`) — default 8080

---

## 5. Live Two-Node E2E Result

### Bootstrap log (VPS-1 devnet, post-patch)
```
[bootstrap] persisted=0 manual=0 fallback=0 dns_hosts=0 dns_addrs=0 signed=0 filtered_local=0
no bootstrap seeds resolved; continuing with persisted peers and inbound discovery
```

### Bootstrap log (VPS-2 devnet, post-patch)
```
[bootstrap] persisted=0 manual=1 fallback=0 dns_hosts=0 dns_addrs=0 signed=0 filtered_local=0
dialing seed 207.244.247.86:39510 (h=0)
P2P outbound 207.244.247.86:39510: received handshake (agent Irium-Node, height 1)
P2P 207.244.247.86:39510: accepted block height 1 hash 2730f85d0d91d3c6...
```

### Final status (both nodes)
| Node | height | peer_count | tip |
|------|--------|-----------|-----|
| VPS-1 devnet | 1 | 1 | `2730f85d0d91d3c6...` |
| VPS-2 devnet | 1 | 1 | `2730f85d0d91d3c6...` |

### E2E test score (phase12k_e2e.py)
24/25 PASS. The one FAIL (`VPS-2 synced to height 1 via P2P: timeout`) occurs only when
VPS-2 was pre-connected before the block was submitted (new-block announcement path).
When VPS-2 connects after the block already exists, sync completes immediately via handshake.

This behaviour (sync stall for pre-connected peers on new-block announce) is a pre-existing
P2P protocol limitation, not introduced by Phase 12-L.

---

## 6. Negative Checks (all pass, unchanged from Phase 12-K)

| Check | Result |
|-------|--------|
| N-1: legacy submit_block rejected 405 | PASS |
| N-2: empty receipts rejected 400 | PASS |
| N-3: bad signature rejected 400 | PASS |
| N-4: spoofed pkh rejected 400 | PASS |
| N-5: insufficient PoW rejected 400 | PASS |
| N-6: mainnet PoAW-X not active | PASS |
| N-7: RPC 39511 localhost-only | PASS |

---

## 7. Mainnet Safety

- Mainnet node (VPS-1, P2P 38291) untouched throughout
- VPS-2 mainnet iriumd.service remains stopped (pre-existing, not restarted)
- No mainnet blocks submitted or modified
- All private keys, tokens, miner IPs redacted

---

## 8. Known Limitations (Carried Forward)

1. **P2P bypass gap**: P2P-relayed blocks bypass receipt validation (irx1 presence only).
2. **New-block sync stall**: Pre-connected peers can stall on new-block announce; reconnect resolves.
3. **Block-contained receipt data (P-1)**: worker pubkeys and solutions not in block wire format.
4. **Consensus-level PoW (P-2)**: puzzle PoW only on submit path, not in connect_block.
5. **Reorg handling (R-2)**: receipts cleared on commit not restored on disconnect.

---

## 9. Summary

| Dimension | Result |
|-----------|--------|
| Devnet seed isolation | **FIXED** — 3 guards in iriumd.rs, 5 tests pass |
| VPS-2 P2P sync (block pre-exists) | **PROVEN** — accepted block 1 at 207.244.247.86:39510 |
| VPS-2 P2P sync (new-block announcement) | PARTIAL — stall in headers_processing; reconnect resolves |
| Mainnet | **UNTOUCHED** |
| Real miner testing | **FROZEN** (P-1, P-2, R-2 still open) |
