# PoAW-X Trusted Miner Pilot — Invite Template

**Version:** 1.0 (post Phase 14-F)
**Status:** TEMPLATE — fill placeholders privately when sending. Do not commit filled-in host/IP or personal info.

> Send this to an invited trusted tester via a private channel (`CONTACT_METHOD`). Fill the placeholders in the private message only — never commit real values.

---

## Plain-language invite

Hi — you're invited to a small, private test of the Irium **PoAW-X testnet**.

This is a controlled pilot with at most 3 trusted testers. You'll point a CPU miner at an isolated test network for a short session and report back what you see.

> **TESTNET ONLY.** This is an isolated test network. There are **no real Irium coins, no rewards, and no mainnet compatibility.** The chain can reset at any time. **Do not use any mainnet wallet, key, or address.** Use a throwaway worker name only.

### What you'll need
- A Stratum v1 CPU miner (e.g. `cpuminer-multi`), algorithm `sha256d`.
- Outbound TCP access to the host/port below.
- A throwaway testnet worker name (any string; no real funds, no real address).

### Connection details (provided privately)
```
Protocol:  Stratum v1 (TCP)
Host:      PILOT_HOST
Port:      STRATUM_PORT
Worker:    TESTNET_WALLET_OR_WORKER_NAME
Password:  x
Start:     START_TIME
Duration:  DURATION
```

Example:
```bash
cpuminer-multi -a sha256d \
  -o stratum+tcp://PILOT_HOST:STRATUM_PORT \
  -u TESTNET_WALLET_OR_WORKER_NAME \
  -p x -t 2
```

### What you're expected to do
1. Connect at `START_TIME` and run for about `DURATION`.
2. Confirm you receive `mining.set_difficulty` then `mining.notify`.
3. Submit shares; accepted shares return `{"result": true, "error": null}`.
4. Stop with Ctrl+C at the end (no local cleanup needed).

### What to report back (to `CONTACT_METHOD`)
```
Time (UTC):        [connection time]
Miner software:    [name + version]
OS / arch:         [e.g. Ubuntu 22.04 / x86_64]
subscribe result:  [ok / error]
authorize result:  [ok / error]
mining.notify:     [received / not received]
Accepted shares:   [N]
Rejected shares:   [N]
Session duration:  [minutes]
Disconnects:       [none / reason]
Log excerpt:       [3-5 lines around the first accepted share]
Notes:             [anything unexpected]
```

### Please DO NOT share publicly
- The host/IP or port you were given.
- Any block data from this testnet (it is not mainnet).
- Screenshots that reveal the connection details.

### Please DO NOT send (privacy)
- Wallet seed phrases, private keys, or any real wallet address.
- Personal credentials of any kind.

### Notes / limitations
- Difficulty is fixed at 1; shares accept quickly.
- The node's RPC is private; the operator verifies receipts/`irx1` privately.
- The chain may reset without warning — expected testnet behaviour.
- Stop anytime with Ctrl+C.

Thanks for helping test PoAW-X. Reply via `CONTACT_METHOD` with questions.

---

### Placeholders to fill privately (never commit real values)
- `PILOT_HOST` — testnet stratum host/IP (private)
- `STRATUM_PORT` — testnet stratum TCP port
- `TESTNET_WALLET_OR_WORKER_NAME` — throwaway worker name
- `START_TIME` — agreed UTC start
- `DURATION` — session length
- `CONTACT_METHOD` — private reporting channel
