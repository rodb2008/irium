# PoAW-X Public Testnet: Tester and Miner Draft Guide

**Status:** DRAFT â€” not yet published. For planning purposes only.  
**Date:** 2026-06-11  
**Phase:** 11-A planning output

---

> IMPORTANT DISCLAIMER: This is a testnet. All coins are valueless. Do not
> use your mainnet wallet, mainnet seed phrase, or real funds on the testnet.
> The testnet can be reset at any time without warning.

---

## What Is PoAW-X?

PoAW-X (Proof of Assigned Work â€” Extended) is a secondary work commitment layer
for the Irium blockchain. Before submitting a mined block, a miner:

1. Fetches a puzzle **assignment** from the node (tied to the current block tip).
2. Solves the assigned puzzle (currently CPU-based, difficulty=1 for testnet).
3. Posts a **receipt** of the solution to the node.
4. Mines a block via Stratum as normal.
5. The stratum automatically injects an `irx1` OP_RETURN commitment into the coinbase
   and submits via `/rpc/submit_block_extended` instead of the legacy `/rpc/submit_block`.

The `irx1` OP_RETURN anchors the puzzle receipt hash on-chain:
```
OP_RETURN PUSH36 "irx1" <32-byte receipts_root>
```

---

## What You Need

- Python 3.8+
- `curl` or any HTTP client
- Access to the testnet stratum endpoint (provided by the operator)
- The PoAW-X soak harness script from the Irium repository

---

## Connecting to Testnet Stratum

Testnet stratum endpoint (Phase 11-D and later):
```
stratum+tcp://<TESTNET_IP>:39512
```

Worker name: any label you choose  
Password: `x`

The testnet stratum speaks standard Stratum v1 protocol. Any Stratum v1 miner should
connect, but only the PoAW-X harness will post receipts to the node.

---

## Using the PoAW-X Soak Harness

The harness is at `scripts/poawx-stratum-long-soak-harness.py` in the Irium repository.

```bash
python3 poawx-stratum-long-soak-harness.py \
  --blocks 30 \
  --stratum <TESTNET_IP>:39512 \
  --rpc http://<TESTNET_RPC_IP>:<RPC_PORT> \
  --receipt \
  --rpc-token <provided-by-operator>
```

**Note:** The RPC endpoint is private. You will need to use the stratum endpoint only
unless the operator has explicitly shared RPC access.

For external testers using stratum-only mode (no receipt posting):
```bash
python3 poawx-stratum-long-soak-harness.py \
  --blocks 30 \
  --stratum <TESTNET_IP>:39512 \
  --rpc http://127.0.0.1:39511
```
(Without `--receipt`, the harness mines blocks but does not post receipts. irx1 will
not be injected since no receipts are pending. The stratum falls back to legacy submit.)

---

## What to Expect

For each block with an active receipt:

```
[soak] Block N/30 [PASS] h=M irx1=True strat=True blk=True mine=0.003s
```

- `irx1=True` â€” the block coinbase contains the irx1 OP_RETURN commitment
- `strat=True` â€” the Stratum server accepted the share
- `blk=True` â€” iriumd accepted the block via `submit_block_extended`

For blocks without a pending receipt (stratum falls back to legacy submit):
```
[soak] Block N/30 [PASS] h=M irx1=False strat=True blk=True
```

This is expected behavior. PoAW-X is only committed when a receipt was posted
before the block was found.

---

## What NOT To Do

- Do not use your mainnet wallet, keys, or seed phrase on the testnet.
- Do not attempt to connect to mainnet ports (38300, 38310, 38320, 3333, 8080).
- Do not share the RPC token if you receive one from the operator.
- Do not attempt to exploit or stress-test the node without explicit permission.
- Do not post receipts for heights more than 2 blocks in the past â€” they will be rejected (HTTP 400).
- Do not submit blocks with an invalid irx1 commitment â€” they will be rejected.

---

## How to Report Logs

If asked to report test results, include:

1. The harness output (redact the RPC token if visible).
2. The last 50 lines of your harness log.
3. Your worker name.
4. Block heights where irx1=False (if any, report why if known).
5. Any HTTP errors from receipt POST or submit_block_extended.

---

## Expected Behavior Reference

| Situation | Expected |
|-----------|----------|
| POST /poawx/receipt before block | HTTP 200, pending_count=1 |
| POST /poawx/receipt with invalid hex | HTTP 400 |
| POST /poawx/receipt to mainnet node | HTTP 503 |
| POST /poawx/receipt for height too old | HTTP 400 |
| Block with receipt pending | irx1=True, submit_block_extended called |
| Block without receipt pending | irx1=False, legacy submit_block called |
| Duplicate receipt for same worker+height | Deduped; pending_count unchanged |
| Block with wrong irx1 commitment | Rejected by iriumd |
| Testnet node restart | Receipts lost; re-post to continue |

---

## Ports and Addresses

| Service | Port | Access |
|---------|------|--------|
| Testnet stratum | 39512 | Public (Phase 11-D+) |
| Testnet iriumd P2P | 39510 | Public (Phase 11-D+) |
| Testnet iriumd RPC | 39511 | Private (operator only) |

Mainnet ports (38300, 3333, 8080, 38310, 38320) are completely separate and
are not involved in PoAW-X testnet operations.

---

## Testnet Disclaimer

- The PoAW-X testnet runs on devnet genesis (separate chain from mainnet).
- All blocks, coins, and balances on the testnet are worthless.
- The testnet chain may be reset at any time.
- This is an experimental protocol. Do not use for any value-bearing purpose.
- The puzzle solution is currently not cryptographically verified â€” this is a
  known testnet limitation. Do not infer anything about production security from
  this behavior.

---

*This guide is draft only. It will be finalized in Phase 11-C after the operator
runbook is complete and external participants have been identified.*
