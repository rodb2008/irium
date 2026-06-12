# PoAW-X Real Miner Pilot Invite

**Version:** 1.0 (Phase 11-F)
**Network:** Isolated PoAW-X Testnet (devnet)
**Status:** Invite-only — 1-3 trusted testers

---

> **TESTNET ONLY. No real Irium coins. No mainnet compatibility.
> Chain may reset at any time. Do not use mainnet wallets or addresses.**

---

## What You Are Testing

The Irium PoAW-X (Proof of Assigned Work Extended) testnet stratum.
Your miner will connect over the public internet to a real testnet node.
Blocks you help mine carry an `irx1` OP_RETURN proof in the coinbase
when a PoAW-X receipt is available.

This is a controlled pilot. You are one of at most 3 trusted testers.

---

## What You Need

- `cpuminer-multi` (recommended) or any Stratum v1 CPU miner
- Outbound TCP access on port 39512
- An Irium testnet address (any valid address string works; no real funds)

---

## Connection

The operator will send you the stratum host IP directly.
Do not share it publicly.

```
Protocol:    Stratum v1 (TCP)
Host:        TESTNET_STRATUM_HOST  (provided by operator)
Port:        39512
Worker:      YOUR_IRIUM_ADDRESS.WORKER_NAME
Password:    x
```

**cpuminer-multi example:**

```bash
cpuminer-multi \
  -a sha256d \
  -o stratum+tcp://TESTNET_STRATUM_HOST:39512 \
  -u YOUR_IRIUM_ADDRESS.worker1 \
  -p x \
  -t 2
```

---

## P2P Seed (optional, for node operators only)

If you are running a testnet node peer (not a miner), the P2P seed is:
```
TESTNET_STRATUM_HOST:39510
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
- Difficulty is hardcoded at 1 (no adaptive difficulty)
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
