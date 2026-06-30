# Irium PoAW-X Phase 20 — Trusted Miner Test Invite (testnet)

Thanks for helping test the Irium PoAW-X Phase 20 multi-role mining on **testnet**. This is a
short, bounded test on a throwaway testnet identity. **It is not mainnet and has no real value.**
Please read the safety section.

> Operator note: this invite is a **template**. Do not send it until the operator has prepared
> the run (see `poaw-x-phase20-external-miner-test-plan.md`). Placeholders in `<…>` are filled in
> by the operator at invite time.

## A. What you provide (public info only)
- your machine's **public IP**
- a **testnet wallet address** (throwaway — see below to create one)
- a **worker name** (e.g. `rig1`)
- your **machine specs** (OS, CPU, and GPU if any)
- your **availability window** (date/time + how long)

## B. What you NEVER provide (we will never ask)
- ❌ your private key
- ❌ your seed phrase / recovery words
- ❌ your wallet file
- ❌ SSH access to your machine
- ❌ any access to our servers

If anyone ever asks you for the items in section B, **do not send them** — it is not part of this
test.

## C. Stock cpuminer command (Tier 1 — connect + mine)
Use a standard SHA-256d cpuminer (`cpuminer` / `minerd`). No special build or version-rolling is
needed. Replace the placeholders with the values the operator gives you:

```
minerd -a sha256d -o stratum+tcp://<POOL_HOST>:<STRATUM_PORT> -u <WALLET_ADDRESS>.<WORKER> -p x -t <THREADS>
```

- `<POOL_HOST>` = operator stratum host/IP
- `<STRATUM_PORT>` = operator-provided port (only reachable from your IP during the test window)
- `<WALLET_ADDRESS>` = your testnet wallet address
- `<WORKER>` = your worker name (e.g. `rig1`)
- `<THREADS>` = number of CPU threads to use

## D. Wallet helper commands (you run these locally; they print text only)
First create a throwaway testnet wallet and address:

```
irium-wallet create-wallet
irium-wallet new-address
irium-wallet list-addresses        # copy the address you want to use
```

**Delegation (emit-only — signs locally, prints JSON, sends nothing over the network).** This
authorizes the operator's pool to build your multi-role coinbase. Official pool is **0% fee**;
use the third-party flags **only** if the operator explicitly asks for a fee test.

```
# Official (0% fee):
irium-wallet poawx-register --emit-only --pool-pubkey <POOL_PUBKEY> --network-id <NET_ID> \
  --addr <WALLET_ADDRESS> --worker <WORKER> --expiry-height <EXPIRY> --fee-bps 0  > delegation.json

# Third-party fee test ONLY if the operator asks (capped 2% = 200 bps, you sign the fee in):
irium-wallet poawx-register --emit-only --pool-pubkey <POOL_PUBKEY> --network-id <NET_ID> \
  --addr <WALLET_ADDRESS> --worker <WORKER> --expiry-height <EXPIRY> \
  --third-party-pool --fee-bps <BPS> --fee-pkh <FEE_PKH>  > delegation.json
```

Send only the resulting `delegation.json` (a signed payload — **no private key inside**) to the
operator out-of-band.

**Role precommit / reveal (Tier 2 — optional, only if the operator asks).** These also emit JSON
only (the precommit hides your secret/nonce; the reveal includes them but **no private key**):

```
irium-wallet poawx-role-precommit --network-id <NET_ID> --target-height <H> --role <compute|verify|support> \
  --solver <WALLET_ADDRESS> --secret <64hex> --nonce <64hex>

irium-wallet poawx-role-reveal --network-id <NET_ID> --target-height <H> --role <compute|verify|support> \
  --solver <WALLET_ADDRESS> --secret <64hex> --nonce <64hex> --prev-hash <64hex>
```

Send the printed JSON to the operator out-of-band. There is **no public submission endpoint** in
this test — the operator injects your JSON on their side via a loopback-only path.

## E. Expected result
- cpuminer **connects** to the stratum
- cpuminer **authorizes** your worker
- cpuminer **receives work** (you'll see job/difficulty lines)
- you **may** find a share/block (depends on your CPU speed and the test difficulty)
- if no share lands in the bounded window, that's fine — please **capture and send your logs**
  (cpuminer output: hash rate, "authorized", work/notify lines, any accepted/rejected lines)

## F. Safety
- The operator opens the stratum port **only to your IP**, **only for the test window**, and
  removes the rule afterward (**source-restricted, temporary**).
- This is **testnet only** — **not mainnet**, no real funds.
- **No private keys / seeds are ever shared** — you only send signed payloads, your public wallet
  address, worker name, and public IP.
- The test is **bounded** (stops after an agreed time).
- The operator's delegation / RPC / status / metrics / role endpoints stay **loopback-only** and
  are never exposed to you.

## Status notes
Phase 20 local readiness is complete and all internal tests pass; this external miner test is
**not yet complete**. Mainnet activation is **not** part of this test. Official pool fee remains
**0%**; any third-party fee is **explicit opt-in only**. Chain difficulty stays **LWMA-144
automatic**.
