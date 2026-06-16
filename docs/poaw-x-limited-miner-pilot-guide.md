# PoAW-X Limited Miner Pilot Guide

**Version:** 1.0 (Phase 11-E)
**Network:** Isolated PoAW-X Testnet (devnet)
**Status:** Limited pilot — invited participants only

> **TESTNET WARNING:**
> This is an isolated test network. No real Irium coins are mined.
> No real rewards are issued. The chain may reset at any time without notice.
> Do not use mainnet wallets, keys, or addresses on this testnet.

---

## Prerequisites

- `cpuminer-multi` (recommended) or any Stratum v1-compatible CPU miner
- A valid Irium testnet address (or use the example PKH-derived address)
- Outbound TCP access to the seed node IP on the operator-selected stratum port

---

## Connection Details

> Connection details will be provided directly by the operator.
> Do not share connection details publicly.

```
Stratum endpoint: SEED_NODE_IP<STRATUM_PORT>
Protocol:         Stratum v1 (TCP)
Worker format:    YOUR_IRIUM_ADDRESS.WORKER_NAME
Password:         x (any value)
```

**Example (cpuminer-multi):**

```bash
cpuminer-multi \
  -a sha256d \
  -o stratum+tcp://SEED_NODE_IP<STRATUM_PORT> \
  -u YOUR_IRIUM_ADDRESS.worker1 \
  -p x \
  -t 2
```

Replace `SEED_NODE_IP` with the IP provided by the operator.
Replace `YOUR_IRIUM_ADDRESS` with your testnet address.

---

## PoAW-X Assignment and Receipt

This testnet uses PoAW-X (Proof of Assigned Work Extended). The stratum handles
receipt injection into the coinbase automatically if a valid receipt exists in
the node pending pool. External miners using the stratum do not need to call
the assignment/receipt endpoints directly.

Assignment/receipt endpoints are private (localhost only).

---

## Expected Behaviour

When connected successfully, your miner should receive:

1. `mining.set_difficulty` — usually `1` on testnet
2. `mining.notify` — job with block template including PoAW-X fields
3. Shares accepted quickly due to low target difficulty

**Successful share response:**

```json
{"id": N, "result": true, "error": null}
```

**Block with irx1 receipt:** When a block includes a valid PoAW-X receipt,
the coinbase contains an `irx1` OP_RETURN (38 bytes). Verify via:
`GET /rpc/block?height=N` — look for `irx1_root` field (non-null = receipt block).

---

## poawx_mode Field

The `getblocktemplate` response includes a `poawx_mode` field:

| Value | Meaning |
|-------|---------|
| `"active"` | PoAW-X path enabled; stratum injects irx1 when receipts are pending |
| `"disabled"` | PoAW-X disabled; blocks mine without irx1 |

---

## Known Limitations

- Receipt persistence is in-memory only; lost on node restart
- Adaptive puzzle difficulty not yet implemented (hardcoded 1 on testnet)
- Testnet chain may reset at any time
- RPC is private; external participants cannot access /rpc/ directly
- No faucet yet — testnet coins have no value
- Single seed node during pilot

---

## What NOT to Do

- Do not use mainnet wallets or real Irium addresses expecting real rewards
- Do not attempt to connect mainnet miners to this testnet
- Do not share the operator-provided stratum IP publicly
- Do not attempt to access RPC port 39511 (it is private/localhost only)
- Do not run production miners on this testnet
- Do not publish testnet block data as if it were mainnet

---

## Reporting Issues

If you encounter problems, send a short report to the operator:

```
Time:          [UTC timestamp]
Miner:         [software + version]
Last height:   [height or unknown]
Error:         [exact error message]
Share stats:   [accepted / rejected count]
Notes:         [anything unusual]
```

---

## Reset Warning

The testnet chain may be reset at any time. All mined blocks will be lost.
This is expected testnet behaviour. You will be notified if a reset occurs.

---

## Emergency Stop

Simply stop your miner (Ctrl+C). No local state to clean up.
The testnet node will detect your disconnect within approximately 60 seconds.

---

## Support

Contact the pilot operator directly. Do not open public GitHub issues for
observations that only affect this isolated testnet.
