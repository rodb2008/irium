# PoAW-X Real Miner Pilot Invite

**Version:** 2.0 (post Phase 15 — native_rewardable route proven)
**Network:** Isolated PoAW-X Testnet (devnet)
**Status:** Invite-only — 1-3 trusted testers

---

> **TESTNET ONLY. No real Irium coins. No mainnet compatibility.
> Chain may reset at any time. Do not use mainnet wallets or addresses.**

> **ROUTE UPDATE (Phase 18 — delegated mode-1).** The current pilot route is the
> non-custodial **delegated** flow: you register a one-time delegation (your wallet
> private key never leaves your machine), the pool produces the receipt for you, and
> your wallet stays the sole payout identity at 0% fee. See **Step 0** below. The
> registration uses the wallet **`--emit-only`** mode, which is a **Phase 19B build
> requirement and is not available yet** — a real external pilot starts only after
> 19B ships.

---

## What You Are Testing

The Irium PoAW-X (Proof of Assigned Work Extended) testnet stratum.
Your miner will connect over the public internet to a real testnet node.
Blocks you help mine carry an `irx1` OP_RETURN proof in the coinbase
when a PoAW-X receipt is available.

This is a controlled pilot. You are one of at most 3 trusted testers.

**Rewardable route:** your standard cpuminer/minerd is served by the gated **native_rewardable** route. When a PoAW-X receipt is pending for your address, an accepted share at the block target produces a real block via `submit_block_extended`, committing an `irx1_root` in the coinbase. (The legacy `cpuminer_compat` adapter is non-rewardable on PoAW-X and cannot promote blocks.)

---

## What You Need

- `cpuminer-multi` (recommended) or any Stratum v1 CPU miner
- Outbound TCP access to the operator-provided stratum host:port (operator-selected; not a fixed port)
- An Irium testnet address (any valid address string works; no real funds)

---

## Step 0 — Register your delegation (one-time, delegated route)

Before mining on the delegated route you register a one-time delegation. This is
non-custodial: **your wallet private key never leaves your machine**, and the operator
only ever receives a signed delegation payload.

1. The operator sends you the **public** pool identity out-of-band: `pool_pubkey`
   (66-hex), `network_id` (`1`=testnet / `2`=devnet), `fee_bps=0`, `domain`.
2. You sign the delegation locally and produce a payload file (no private key inside):
   ```
   irium-wallet poawx-register --emit-only \
     --pool-pubkey <66hex> --network-id <1|2> \
     --addr <your-testnet-address> --worker <worker> \
     --expiry-height <N> --fee-bps 0 > poawx-delegation.json
   ```
3. You send **only** `poawx-delegation.json` back to the operator. The operator submits
   it to the pool's loopback-only endpoint. Once confirmed, you mine (below).

> `--emit-only` is a **Phase 19B** wallet feature and is **not available yet**; this
> step is documented so you know the flow. Do not send anyone your seed phrase or
> private key — only the signed `poawx-delegation.json` payload. `--fee-bps` must be 0.

---

## Connection

The operator will send you the stratum host IP directly.
Do not share it publicly.

```
Protocol:    Stratum v1 (TCP)
Host:        TESTNET_STRATUM_HOST  (provided by operator)
Port:        <operator-provided>   (operator-selected stratum port; source-restricted to your IP)
Worker:      YOUR_IRIUM_ADDRESS.WORKER_NAME
Password:    x
```

**cpuminer-multi example:**

```bash
cpuminer-multi \
  -a sha256d \
  -o stratum+tcp://TESTNET_STRATUM_HOST:<STRATUM_PORT> \
  -u YOUR_IRIUM_ADDRESS.worker1 \
  -p x \
  -t 2
```

---

## P2P Seed (optional, for node operators only)

If you are running a testnet node peer (not a miner), the P2P seed is:
```
TESTNET_STRATUM_HOST:<NODE_P2P_PORT>
```
This is separate from the stratum port. Most testers do not need it.

---

## What Should Happen

1. You connect and receive `mining.set_difficulty` (difficulty=1)
2. You receive `mining.notify` with a PoAW-X job
3. You submit a share; it is accepted immediately
4. Repeat until the operator signals success

Share acceptance looks like:
```json
{"id": N, "result": true, "error": null}
```

---

## What to Send Back

After your session, send the operator a short report:

```
Time:             [UTC timestamp of connection]
Miner software:   [e.g. cpuminer-multi 1.3.7]
OS:               [e.g. Ubuntu 22.04 / x86_64]
subscribe result: [ok / error message]
authorize result: [ok / error message]
mining.notify:    [received / not received]
Accepted shares:  [N]
Rejected shares:  [N]
Session duration: [minutes]
Disconnect event: [none / error / reason]
Log excerpt:      [3-5 lines around first accepted share]
Notes:            [anything unexpected]
```

**Do NOT send:**
- Wallet seed phrases
- Private keys
- Personal wallet addresses (a dummy address is fine)

---

## Known Limitations

- Testnet chain may reset without warning
- No real Irium rewards
- No mainnet compatibility guarantee yet
- Difficulty: vardiff off; on the isolated devnet a sub-1 floor is used (operator-gated, non-mainnet only) so a CPU finds genuine blocks quickly
- RPC is not publicly accessible; operator verifies irx1 privately
- Single seed node in this pilot

---

## Do NOT

- Connect mainnet miners expecting real rewards
- Share the stratum IP/port publicly
- Attempt to access port 39511 (it is private)
- Run more than 2 simultaneous workers in this pilot
- Publish mined blocks as mainnet activity

---

## Emergency Stop

Ctrl+C stops your miner immediately. No local cleanup needed.

---

## Contact

Report issues or observations directly to the operator.
Do not open public GitHub issues for testnet-only behaviour.
