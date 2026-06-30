# PoAW-X Trusted Miner Pilot — Stop Conditions

**Version:** 1.0 (post Phase 14-F)

If **any** condition below is observed, **STOP the pilot immediately**: signal the miner to disconnect, stop the testnet stratum and node (devnet ports only — never mainnet), then verify both mainnets are unchanged. Investigate before any retry.

## Immediate stop conditions

1. **Mainnet affected in any way** — either mainnet node (VPS-1 / irium-eu) changes PID unexpectedly, changes binary hash, loses sync, errors, or shows any PoAW-X activity.
2. **RPC 39511 becomes publicly reachable** — or status 39508, or any private endpoint answers from a public IP.
3. **Unexpected public peers/seeds** — the testnet node dials or accepts mainnet seeds, or unknown peers appear beyond the invited tester.
4. **Accepted share but invalid receipt/root** — a share is accepted yet the corresponding receipt or `irx1_root` is missing, zero, or inconsistent.
5. **Block rejected by consensus** — a block that should be valid is rejected by `connect_block` / PoAW-X validation.
6. **Worker identity mismatch** — receipt `worker_pkh`/pubkey/signature does not validate, or differs from the submitting worker.
7. **Reward split mismatch** — coinbase does not pay the correct PoAW-X worker share.
8. **Resource pressure** — unsafe memory/CPU/disk pressure on the host (risk to co-located mainnet/pool services).
9. **Miner sees mainnet-looking information** — any mainnet height/tip/seed/wallet data leaks into the testnet session or stratum responses.
10. **Secret or private data in logs** — any token, RPC auth, private key, wallet secret, env value, real IP, or personal info appears in logs/output.
11. **Node/pool crash or repeated restart** — testnet node or stratum panics, exits, or enters a restart loop.
12. **Suspected abuse or unknown external connection** — any connection to the stratum that is not the invited trusted miner, or any sign of probing/abuse.

## Delegated mode-1 route — additional stop conditions (Phase 18; for external pilots)

13. **Delegate paid** — the coinbase contains an output to the pool delegate pkh (the delegate must be signer-only and never paid).
14. **Non-zero fee** — pool identity or a registered delegation reports `fee_bps>0`, or any `fee_bps>0` is accepted instead of rejected.
15. **Delegation endpoint exposed** — `/poawx/delegation` (or the delegation bind) is reachable from anything other than `127.0.0.1`.
16. **Private key exposure** — a miner private key or seed reaches the operator/registry, or appears in any payload/log (only a signed delegation payload may be transferred).
17. **Variant-sweep promotion** — a PoAW-X block is promoted via a compat/variant byte-order sweep rather than the single deterministic canonical reconstruction.
18. **Delegation mismatch on sync** — an independent peer rejects the embedded delegation, or the embedded delegation differs across peers for the same block.

## Stop procedure

1. Tell the miner to Ctrl+C (stop mining).
2. Stop the testnet stratum (devnet stratum unit only).
3. Stop the testnet node and free devnet ports only:
   ```bash
   for p in 39512 39510 39511 39508; do fuser -k $p/tcp 2>/dev/null; done
   ```
4. Confirm devnet ports clear (39512/39510/39511/39508).
5. **Verify both mainnets unchanged** — PID + official binary hash `7c07ae2c…` on VPS-1 and irium-eu; height advancing; PoAW-X OFF.
6. Capture and **sanitize** logs for post-mortem (mask IPs, strip secrets).
7. Do not retry until the root cause is understood and the relevant stop condition can no longer occur.

## Escalation

Any condition touching mainnet (items 1, 2, 9) is highest severity: stop first, then report with sanitized evidence before any further action.
