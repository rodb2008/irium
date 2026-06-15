# PoAW-X Trusted Miner Pilot — Acceptance Criteria

**Version:** 1.0 (post Phase 14-F)

The pilot is considered **successful only if ALL** of the following hold.

## Must-pass (success requires every item)

1. **External connection** — a trusted, invited external miner connects to the testnet stratum at `PILOT_HOST:STRATUM_PORT` (not localhost).
2. **Valid accepted share** — the miner submits **at least one** valid share that the stratum accepts (`{"result": true, "error": null}`).
3. **PoAW-X receipt path** — where applicable, the accepted work corresponds to a valid PoAW-X receipt in the node's pending pool (valid worker identity, nonce, signature, puzzle PoW).
4. **Valid irx1 root** — a resulting block (or block candidate) includes a well-formed, **non-zero** `irx1` OP_RETURN commitment in the coinbase.
5. **Operator irx1 verification** — the operator verifies `irx1_root` for the block via the **private** RPC (`127.0.0.1:39511`) and it matches the receipt set.
6. **No mainnet service changes** — both mainnet nodes (VPS-1, irium-eu) unchanged: same PID, official binary hash `7c07ae2c…`, height advancing, PoAW-X OFF.
7. **No public RPC exposure** — 39511 and 39508 remain private (refused from public IP) for the entire session.
8. **No crash / panic / restart loop** — the testnet node and stratum stay up; no repeated restarts; no panics in logs.
9. **No invalid reward split** — coinbase pays the worker the correct PoAW-X share; no reward-split validation failures.
10. **No failed consensus validation** — `connect_block` accepts the good block; the 7-rule PoAW-X validation passes; no unexpected rejections of valid blocks.
11. **Clean evidence** — logs contain enough to demonstrate the above **without** exposing secrets (no tokens, auth, private keys, real IPs, personal info in shared excerpts).

## Quality / nice-to-have (not required for pass)

- Multiple accepted shares across the session.
- A second testnet peer confirms P2P propagation (same height + tip hash).
- Receipt clears from pending after block commit (lifecycle confirmed).
- Miner report is complete (subscribe/authorize/notify/accepted/rejected/duration).

## Disqualifiers (any one = NOT successful)

- Any mainnet impact whatsoever.
- RPC/status port reachable publicly at any point.
- An accepted share that does **not** map to a valid receipt/`irx1` path.
- A valid block rejected by consensus, or an invalid block accepted.
- Worker identity or reward-split mismatch.
- Secret/private data appearing in any log or shared artifact.
- Node/stratum crash or restart loop during the session.

## Sign-off

Record at session end: branch/hash (`a0aedc6`), connection confirmed (external), accepted-share count, irx1 verified (height + root prefix), both-mainnet-untouched confirmation, and a PASS/FAIL against the must-pass list.
