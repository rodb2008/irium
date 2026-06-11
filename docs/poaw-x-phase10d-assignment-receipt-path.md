# Phase 10-D: PoAW-X Assignment Receipt Path

Branch: `testnet/poawx-phase10d-assignment-receipt-path`  
Status: **PASS=30 FAIL=0 SKIP=0** (verified 2026-06-11)

## Root Cause of Phase 10-C Missing /poawx/assignment

The Phase 10-C branch had PoAW-X Rust source that was compiled into the
stratum binary (Jun 10 14:38) and then removed from disk. The iriumd binary
was rebuilt on Jun 11 00:24 *after* cleanup, so iriumd had no PoAW-X code.
Phase 10-C accepted blocks via the `/rpc/submit_block` legacy fallback.

## What Phase 10-D Does

Restores and proves the complete PoAW-X receipt path end-to-end:

```
/poawx/assignment  →  POST /poawx/receipt  →  pending in template
    →  irx1 OP_RETURN in coinbase  →  /rpc/submit_block_extended  →  accepted block
    →  pending receipts cleared
```

## Implementation

### iriumd (`src/bin/iriumd.rs`)

Three new routes added:

**`GET /poawx/assignment`**
- Returns 503 if `IRIUM_POAWX_MODE != active` or network is Mainnet
- Returns 404 if chain tip height is 0
- Derives assignment seed: `SHA256(tip_hash || height_le || bpoawx_assignment_seed_v1)`
- Derives commitment nonce: `SHA256(seed || bcommitment_nonce)`
- Returns: `{height, seed, commitment_nonce, puzzle_difficulty, lane, pow_bits}`

**`POST /poawx/receipt`**
- Validates `solution` and `commitment_nonce` fields are valid hex
- Validates height is current or recent (within 1 block)
- Deduplicates by (height, lane, worker_pkh)
- Appends to `AppState.poawx_pending_receipts`
- Returns: `{ok: true, pending_count: N}`

**`POST /rpc/submit_block_extended`**
- Accepts: header, tx_hex, poawx_receipts, poawx_receipts_root
- Validates receipts_root matches computed root from receipts
- Validates irx1 OP_RETURN in coinbase when receipts present:
  `0x6a 0x24 irx1 <32-byte receipts_root>` = 38 bytes
- Runs `connect_block` (same as submit_block)
- Clears pending receipts for the committed height
- Returns: `{accepted: true, height, tip}`

**receipts_root algorithm:**
```
SHA256(concat(SHA256(height_le || lane_bytes || worker_pkh_decoded ||
                     solution_decoded || commitment_nonce_decoded)
              for each receipt))
```

**Template (`GET /rpc/getblocktemplate`) extended fields:**
- `poawx_mode`: `active` when PoAW-X is enabled
- `poawx_pending_receipts`: list of pending receipts
- `receipts_root`: hex-encoded SHA256 root (empty string if no receipts)

### Stratum (`pool/irium-stratum/`)

- `src/template.rs`: added `poawx_mode`, `poawx_pending_receipts`, `receipts_root` to `GetBlockTemplate`; new `PoawxPendingReceipt` struct
- `src/block.rs`: `compute_receipts_root_from_pending` and `build_irx1_commitment_script` functions
- `src/stratum.rs`: when `IRIUM_STRATUM_POAWX=1` and receipts are pending, injects irx1 OP_RETURN into coinbase extras and posts to `/rpc/submit_block_extended` instead of `/rpc/submit_block`
- `src/main.rs`: logs PoAW-X mode at startup

## irx1 OP_RETURN Format

```
byte[0]    = 0x6a  (OP_RETURN)
byte[1]    = 0x24  (PUSH 36 bytes)
byte[2..6] = irx1 (4 ASCII bytes)
byte[6..]  = receipts_root (32 bytes)
total      = 38 bytes
```

## Test Script

`scripts/testnet-poawx-phase10d-assignment-receipt-path.sh`

| Section | What it tests |
|---------|--------------|
| 0 | Pre-flight: binaries exist, mainnet alive |
| 1 | Start testnet iriumd (devnet, POAWX_MODE=active) |
| 2 | Template returns bits=207fffff, poawx_mode=active |
| 3 | GET /poawx/assignment returns 200 with valid seed/nonce |
| 4 | POST /poawx/receipt stores receipt (pending_count=1) |
| 5 | Template shows non-empty receipts_root and pending receipts |
| 6 | Start stratum with IRIUM_STRATUM_POAWX=1 |
| 7 | Mine 10 blocks via Phase 10-C soak harness (--receipt mode) |
| 8 | Stratum log: irx1 injection logged, submit_block_extended called |
| 9 | iriumd log: submit_block_extended blocks accepted |
| 10 | /rpc/submit_block_extended endpoint reachable |
| 11 | Mainnet iriumd still alive, port 38300 still bound |
| 12 | No panics in either log |

## Testnet Ports (isolated, no mainnet overlap)

| Port | Service |
|------|---------|
| 39500 | P2P (testnet) |
| 39501 | RPC (testnet) |
| 39502 | Stratum (testnet) |

## Environment Variables

| Variable | Value | Service |
|----------|-------|---------|
| IRIUM_POAWX_MODE | active | iriumd |
| IRIUM_NETWORK | devnet | iriumd |
| IRIUM_STRATUM_POAWX | 1 | stratum |

## Result (2026-06-11)

```
PASS=30  FAIL=0  SKIP=0
blocks_pass=10  irx1_in_coinbase_count=10  receipt_test_passed=True
```

All 10 stratum-mined blocks included the irx1 OP_RETURN commitment.  
submit_block_extended called 10 times, all accepted.  
Mainnet untouched throughout.
