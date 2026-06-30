# PoAW-X Public Testnet: Tester and Miner Guide

**Status:** DRAFT — updated for Phase 11-C. Not yet published externally.
**Date:** 2026-06-12
**Phase:** 11-C

---

> IMPORTANT: This is a TESTNET. All coins are valueless. Do not use your mainnet
> wallet, mainnet seed phrase, or real funds here. The testnet can be reset at any
> time without warning.

---

## What Is PoAW-X?

PoAW-X (Proof of Assigned Work — Extended) is a secondary work commitment layer for
the Irium blockchain. Before submitting a mined block, a miner:

1. Fetches a puzzle **assignment** from the node (tied to the current block tip).
2. Solves the assigned puzzle (CPU-based, difficulty=1 for testnet).
3. Posts a **receipt** of the solution to the node via HTTP.
4. Mines a block via Stratum as normal.
5. The stratum automatically injects an `irx1` OP_RETURN commitment into the coinbase
   and submits the block via `/rpc/submit_block_extended`.

The `irx1` OP_RETURN anchors the puzzle receipt on-chain:
```
OP_RETURN PUSH36 "irx1" <32-byte receipts_root>
```

---

## Testnet-Only Disclaimer

- **All testnet coins are worthless.** Do not attempt to trade or store them.
- **The testnet chain may be reset at any time.** Your testnet history can disappear.
- **Do not use your mainnet wallet, keys, or seed phrase** on the testnet.
- **Do not connect to mainnet ports** (38300, 38310, 38320, 3333, 8080).
- This is an experimental protocol under active development.

---

## What You Need

- Python 3.8+
- `curl` or any HTTP client
- Access to the testnet stratum endpoint (provided by the operator)
- Irium repository cloned for the soak harness (optional)

---

## Connecting to Testnet Stratum

Testnet stratum endpoint (Phase 11-D and later):
```
stratum+tcp://<TESTNET_SEED_IP>:39512
```

Worker name: any label you choose
Password: `x`

The testnet stratum speaks standard Stratum v1. Any Stratum v1 miner can connect,
but only the PoAW-X harness posts receipts to the node.

**Note:** Port 39512 requires the cloud firewall rule to be open. Check with the
operator before attempting to connect externally.

---

## Using the Harness

The Phase 11-B canonical validation harness is the recommended test tool:

```bash
cd ~/irium
python3 scripts/poawx-phase11b-canonical-receipts-validation.py
```

This is a self-contained script that spawns isolated testnet processes, runs all
checks, and cleans up automatically.

For a targeted stratum-only test:
```bash
python3 scripts/poawx-stratum-long-soak-harness.py   <TESTNET_IP> 39512   http://127.0.0.1:39511 <rpc-token>   --blocks 10
```

---

## What to Expect

For each block with an active receipt:
```
[soak] Block N/10 [PASS] h=M irx1=True strat=True blk=True
```

- `irx1=True` — block coinbase contains the irx1 OP_RETURN commitment
- `strat=True` — stratum accepted the share
- `blk=True` — iriumd accepted via `submit_block_extended`

For blocks without a pending receipt (stratum falls back to legacy):
```
[soak] Block N/10 [PASS] h=M irx1=False strat=True blk=True
```

This is expected. PoAW-X is only committed when a receipt was posted before the
block was found.

---

## Verifying irx1 Commitment On-Chain

After a PoAW-X block is mined, you can verify the irx1 root in the block:

```bash
curl -sf "http://127.0.0.1:39511/rpc/block?height=<HEIGHT>" | python3 -c "
import sys, json
d = json.load(sys.stdin)
print('height:', d['height'])
print('irx1_root:', d.get('irx1_root'))
"
```

- `irx1_root`: 64-char hex string for PoAW-X blocks, `null` for legacy blocks.

---

## Understanding `poawx_mode`

The `getblocktemplate` response includes a `poawx_mode` field:

| Value | Meaning |
|-------|---------|
| `"active"` | Node has IRIUM_POAWX_MODE=active, receipts accepted |
| `"disabled"` | Node is not running in PoAW-X mode, receipt endpoints return 503 |

---

## What NOT To Do

- Do not use your mainnet wallet or seed phrase on the testnet.
- Do not connect to mainnet ports (38300, 38291, 3333, 8080).
- Do not share the operator-provided RPC token.
- Do not attempt to stress-test or exploit the node without explicit permission.
- Do not post receipts for heights more than 2 blocks in the past — they will
  be rejected with HTTP 400.
- Do not submit blocks with an invalid irx1 commitment — they will be rejected.
- Do not attempt mainnet activation of PoAW-X — this is testnet-only.

---

## Receipt API Reference

Post a receipt (requires PoAW-X active mode):
```bash
curl -X POST   -H "Content-Type: application/json"   -H "Authorization: Bearer <token>"   -d '{"height": N, "lane": "cpu", "worker_pkh": "<pkh-hex>", "solution": "<sol-hex>", "commitment_nonce": "<nonce-hex>"}'   http://<TESTNET_RPC>:39511/poawx/receipt
```

Get assignment for next block:
```bash
curl http://<TESTNET_RPC>:39511/poawx/assignment
```

Returns `{"height": N, "seed": "...", "commitment_nonce": "...", "puzzle_difficulty": 1, "lane": "cpu", "pow_bits": "..."}`

---

## How to Report Logs

If asked to report test results, provide:

1. Harness output (redact the RPC token if visible).
2. Last 50 lines of your harness log.
3. Your worker name/identifier.
4. Block heights where `irx1=False` (if any, note why if known).
5. Any HTTP errors from receipt POST or submit_block_extended.
6. The testnet iriumd version (commit hash if available).

---

## Expected Behavior Reference

| Situation | Expected |
|-----------|----------|
| `GET /poawx/assignment` on active node | HTTP 200, `commitment_nonce`, `seed` |
| `POST /poawx/receipt` valid | HTTP 200 |
| `POST /poawx/receipt` wrong commitment_nonce | HTTP 400 |
| `POST /poawx/receipt` insufficient PoW | HTTP 400 |
| `POST /poawx/receipt` fabricated (any field fake) | HTTP 400 |
| `GET /poawx/assignment` on disabled node | HTTP 503 |
| `POST /poawx/receipt` on disabled node | HTTP 503 |
| Block with valid pending receipt | irx1=True, submit_block_extended used |
| Block without pending receipt | irx1=False, legacy submit_block used |
| Duplicate receipt (same worker+height) | Deduped, HTTP 200 |
| Wrong irx1 commitment in block | Rejected HTTP 400 |
| Node restart | Receipts lost (in-memory only) — re-post |
| `GET /rpc/block?height=N` | JSON with `irx1_root` field |

---

## Ports and Addresses

| Service | Port | Access |
|---------|------|--------|
| Testnet stratum | 39512 | Public (Phase 11-D+, firewall required) |
| Testnet iriumd P2P | 39510 | Public (Phase 11-D+, firewall required) |
| Testnet iriumd RPC | 39511 | Private (operator only) |

Mainnet ports (38300, 38291, 3333, 8080, 38310, 38320) are completely separate.

---

## Known Limitations

- Receipt persistence: in-memory only. All pending receipts are lost on iriumd restart.
  Re-post a fresh receipt after the node restarts.
- Adaptive puzzle difficulty: hardcoded at 1 for testnet. Production will vary.
- External miner onboarding: full guide is in Phase 11-D.

---

*Guide updated for Phase 11-C. External distribution pending Phase 11-D launch.*


---

## Phase 11-E Status

Limited miner pilot run 2026-06-12 using VPS-2 simulation.
Results: 12/12 blocks PASS, irx1 in 9/12 blocks, receipt PASS.
External miner package: `docs/poaw-x-limited-miner-pilot-guide.md`
Real external miner validation: PENDING (next pilot session)
