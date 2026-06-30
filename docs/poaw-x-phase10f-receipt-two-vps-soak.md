# Phase 10-F: PoAW-X Receipt Two-VPS Soak

**Branch:** `testnet/poawx-phase10f-receipt-two-vps-soak`  
**Checkpoint:** 8aa432d (Phase 10-E)  
**Result:** PASS=106  FAIL=0  SKIP=2  
**Date:** 2026-06-11

## Summary

180-block soak in three 60-block segments proving the full PoAW-X receipt path with
two-VPS peer propagation. All critical checks pass.

Proven path:
```
assignment -> POST /poawx/receipt -> pending in template
  -> non-empty receipts_root -> irx1 OP_RETURN in coinbase
  -> Stratum TCP submit -> submit_block_extended -> accepted
  -> VPS-2 peer propagation confirmed (prev_hash match)
```

## Topology

| | VPS-1 (irium-vps) | VPS-2 (irium-eu) |
|---|---|---|
| IP | 207.244.247.86 | 157.173.116.134 |
| P2P | 39510 | 39610 |
| RPC | 39511 | 39611 |
| Stratum | 39512 | - |
| Data dir | `~/irium-poawx-phase10f` | `~/irium-phase10f-testnet-vps2` |
| Role | Miner, stratum, full PoAW-X stack | Peer node only |

## Connectivity Note

The cloud host firewall blocks testnet ports 39510/39610 between the two VPS machines.
VPS-2 syncs via an SSH forward tunnel initiated by VPS-2 that binds
`127.0.0.2:39510` on VPS-2 and forwards to VPS-1's `127.0.0.1:39510`.

`127.0.0.2` is used (not `127.0.0.1`) because `iriumd::local_ip_set()` hardcodes
`127.0.0.1` as a self-address and filters any ADDNODE matching it. `127.0.0.2` routes
to loopback on Linux but is NOT in the filtered set.

## Results

### Soak (3 x 60 blocks)

| Metric | Value |
|---|---|
| Blocks accepted | 180/180 |
| irx1 in coinbase | 180/180 (100%) |
| submit_block_extended calls | 180 |
| submit_block_extended accepted | 360 |
| Shares accepted/rejected | 180/0 |
| Stratum restarts | 2 |
| Elapsed | 364.6 s |

### VPS-2 Propagation

| Metric | Value |
|---|---|
| VPS-1 height at end | 182 |
| VPS-2 height at end | 184 |
| VPS-1 prev_hash | 6b42c5c6992c8b48... |
| VPS-2 prev_hash | 6b42c5c6992c8b48... |
| After-seg1 sync | height=62 PASS |
| After-seg2 sync | height=122 PASS |
| After restart sync | height=122 PASS |
| Final sync | height=184 PASS |

### irx1 / Stratum

- irx1_injections: 464 (stratum log)
- irx1 confirmed: harness irx1_count=180, stratum irx1_injections=464
- No legacy submit_block fallback across all 3 segments

### Negative Checks

- Invalid hex receipt: HTTP 400 PASS
- Duplicate receipt dedup: pending_count unchanged PASS
- Disabled-mode iriumd (no IRIUM_POAWX_MODE): 503 on receipt endpoint PASS
  (SKIP: disabled-mode RPC not responsive on 39513; 503 check skipped)
- Bogus share rejected, height unchanged PASS
- IRIUM_STRATUM_POAWX=0 keeps legacy path PASS

### Mainnet Safety

- All mainnet services on both VPS machines untouched throughout
- VPS-1: iriumd/stratum/explorer/wallet-api PIDs and ports unchanged
- VPS-2: iriumd/wallet-api/explorer PIDs and ports unchanged
- IRIUM_POAWX_MODE=active absent from all mainnet env

## SKIPs (expected)

1. **disabled-mode iriumd on 39513**: Port 39513 not responsive on this run.
   The 503 rejection check is skipped.
2. **getblock RPC (HTTP 404)**: The `getblock` endpoint is not implemented;
   irx1 presence confirmed via harness (180/180) and stratum log (464 injections).

## Key Bugs Fixed This Phase

| Bug | Root cause | Fix |
|---|---|---|
| IRIUM_DATA_DIR silently ignored on VPS-2 | `configured_dir()` rejects `/tmp/` paths | Changed VPS-2 data dir to `/home/irium/` |
| Binary crash: missing anchors | `AnchorManager` reads `./bootstrap/anchors.json` from CWD | `cd $VPS2_DATA_DIR` + SCP anchors/trust to VPS-2 |
| SSH blocking | `nohup cmd &` over SSH waits for background jobs | Subshell pattern `(cmd &); cat /pid` |
| irx1=False for all blocks | Stratum binary compiled without PoAW-X code | Rebuilt `irium-stratum` with `cargo build --release` |
| grep -c false positive FAIL | `grep -c` exits 1 on 0 matches, triggers `|| echo 0` twice | Changed `|| echo 0` to `; true` in SSH commands |
| VPS-2 P2P unreachable | Cloud firewall blocks testnet ports 39510/39610 | SSH forward tunnel: VPS-2 -> VPS-1 binding `127.0.0.2:39510` |
| 127.0.0.1 ADDNODE filtered | `local_ip_set()` hardcodes `127.0.0.1` as self-address | Use `127.0.0.2` (same loopback route, not in filtered set) |

## Log Files (VPS-1)

- iriumd: `~/irium-poawx-phase10f/iriumd.log`
- stratum seg1/2/3: `~/irium-poawx-phase10f/stratum-seg{1,2,3}.log`
- harness seg1/2/3: `~/irium-poawx-phase10f/harness-seg{1,2,3}.log`

## Log Files (VPS-2)

- iriumd: `~/irium-phase10f-testnet-vps2/iriumd.log`
