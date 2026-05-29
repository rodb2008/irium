# Hosting Your Own Irium Pool

> **Heads-up:** This document describes how to run your own Irium pool.
> The official Irium pool at `pool.iriumlabs.org` no longer runs a
> separate solo port — every port (3333 ASIC / 3335 CPU/GPU / 443
> firewall-bypass) now uses the same unified direct-payout model:
> coinbase pays the full 50 IRM block reward directly to the miner's
> address, zero fees. The same `irium-stratum` binary that powers the
> official pool is open-source, and **anyone can host their own pool
> using this guide.**

The current model is what previous Irium documentation called "solo
pool mining" — every connected miner is paid directly in the coinbase
of any block they find, with no shared-payout (PPLNS) accounting and
no waiting period beyond the standard 100-block coinbase maturity.

This document explains how the current pool works, how to connect a
miner, how the on-chain payout is structured, how to verify a block
you mined, and how to run your own pool from the same open-source
binary.

---

## How blocks are paid

When you mine on an Irium pool (official or self-hosted) running the
current `irium-stratum` binary, **the coinbase has exactly one output**:

| # | Recipient | Amount |
|---|-----------|--------|
| 0 | Your Irium address (parsed from the worker username you authorized with) | **50 IRM** (full block reward) |

This means **the moment your block is accepted by the network you
can already see the IRM destined for your address on-chain** in any
block explorer. You don't have to trust the pool to ever send you
anything — the chain already says it's yours.

The reward becomes spendable after **100 blocks of coinbase maturity**
(standard for any Irium coinbase).

> The previous PPLNS arrangement (where the coinbase paid a pool
> wallet and the operator later redistributed proportionally) was
> removed after the 2026-05-29 over-distribution incident. The
> previous "solo pool" 2-output model (49.5 IRM to worker + 0.5 IRM
> pool fee) was also retired at the same time. Both are gone from
> the current open-source binary.

---

## Connection settings (any pool running the current `irium-stratum`)

Configure your ASIC or CPU/GPU firmware exactly like this:

```
URL:      stratum+tcp://<pool host>:<port>
Worker:   <YOUR-IRIUM-ADDRESS>            # MUST be a Q-prefixed or P-prefixed single-sig Irium address
Password: x                                # Any non-empty string works; "x" is conventional
```

For the official pool, see [`MINING.md`](MINING.md) — the three ports
(3333 ASIC, 3335 CPU/GPU, 443 firewall-bypass) are documented there.
A self-hosted pool typically uses port `3333` (or any port you
choose) for its public stratum listener.

### Multi-rig setup

If you run multiple rigs and want to identify them in stats, append a
suffix after a dot:

```
Worker: QYourIriumAddress.rig01
Worker: QYourIriumAddress.basement-bitaxe
```

Everything before the dot is parsed as the payout address; the part
after is a label that only appears in stats. **Both rigs mine to the
same coinbase output** because they share the same address.

### Invalid worker names

If the worker username does not decode as a valid Irium address, the
stratum server rejects the `mining.authorize` with `[20] invalid
address`. This is intentional — direct-payout mode has no fallback
"pool wallet" to pay to. Authorize fails fast so you don't burn
hashes for nothing.

---

## Expected earnings

Per-share probability of finding a block:

```
P(block) = share_difficulty / network_difficulty
```

Rough expected-revenue estimate:

```
Expected IRM per day ≈ (your_hashrate / network_hashrate) × 50 × blocks_per_day
```

Where:
- `blocks_per_day` is the network's actual block production rate. On
  Irium with a ~1-min observed block time this is roughly 1440, though
  the protocol target is 10 min (144 blocks/day) once network hashrate
  retargets there.
- `network_hashrate` can be read from `/rpc/network_hashrate` on your
  local `iriumd`, or from the Network panel in Irium Core's Explorer.

**Important:** variance is high. A miner with 1 % of network hashrate
will *on average* find ~14 blocks per day at the current ~1-min
cadence, but might find 0 today and 30 tomorrow. That's the nature
of solo-style mining — you accept the variance in exchange for
keeping every block you find as your own coinbase.

---

## How to verify your block was found

Three checks, in order of cost:

### 1. Your stratum miner log

After a block your firmware logs an accepted share with `Block found`
/ `solved` / `accepted=true` (the exact wording varies). The pool also
logs `[block] submitted worker=<your_addr>... height=N` in its
journal — but that journal is on the pool host, not visible to you.

### 2. The Explorer "Pool Stats" panel (Irium Core)

The Explorer > Pool Stats tab shows blocks found, per-miner stats,
and rolling 15-minute hashrate. Your worker row appears in the
table within ~30 seconds of your first accepted share.

### 3. On-chain verification (definitive)

Pull the block you suspect you mined from any iriumd:

```bash
curl -s 'http://127.0.0.1:38300/rpc/block?height=<N>' | python3 -m json.tool
```

In the response, decode `tx_hex[0]` (the coinbase). You should see
**one output paying your address (50 IRM)**. If the coinbase pays
some other address, the block was not yours.

---

## FAQ

### Why does the worker username have to be a valid address?

Because the coinbase template literally encodes your address as the
P2PKH output script. There is no fallback "pool wallet" to pay you
later if the username is malformed. The authorize handler refuses
connections whose username doesn't decode to a 20-byte pkh.

### Can I charge a pool fee on my own pool?

Not via a runtime knob in the current open-source binary. The
previous `IRIUM_STRATUM_SOLO_FEE_BPS` / `IRIUM_STRATUM_SOLO_MODE`
env vars were removed when the PPLNS payout system was stripped out
(2026-05-29). To charge a fee you must fork the `irium-stratum`
source and reintroduce a 2-output coinbase that diverts a slice to
your operator address. This is a deliberate friction step — the
unified direct-payout model is now the default because it eliminates
the over-distribution failure modes of the previous PPLNS payout
queue.

### What happens if the pool is offline?

Your rig loses its stratum connection and stops mining until the
pool is back. You lose nothing — there is no escrowed reward sitting
on the pool waiting to pay out. If you want failover, configure your
ASIC firmware with a second pool URL.

### Can I run a pool without running iriumd locally?

In principle yes — `irium-stratum` only needs an HTTP/HTTPS RPC
endpoint that speaks the iriumd API. But the recommended setup is
co-located `iriumd` + `irium-stratum` for low template latency and
zero inbound P2P exposure on the stratum host.

---

## Running your own pool

The direct-payout pool feature is built into the open-source
`irium-stratum` binary from the upstream `iriumlabs/irium`
repository. You can run it yourself on the same VPS as your
`iriumd`, or anywhere with TCP reachability to your `iriumd`'s RPC
port.

### Build

```bash
git clone https://github.com/iriumlabs/irium.git
cd irium/pool/irium-stratum
cargo build --release
# Binary lands at target/release/irium-stratum
```

### Environment file (`/etc/irium-pool/stratum.env`)

```env
IRIUM_RPC_BASE=http://127.0.0.1:38300
IRIUM_RPC_TOKEN=<your iriumd RPC token>
STRATUM_BIND=0.0.0.0:3333
STRATUM_METRICS_BIND=127.0.0.1:3334
STRATUM_DEFAULT_DIFF=10000
IRIUM_STRATUM_MINER_FAMILY=asic
IRIUM_STRATUM_VARDIFF_ENABLED=1
IRIUM_STRATUM_VARDIFF_MIN_DIFF=1
IRIUM_STRATUM_VARDIFF_MAX_DIFF=2000000
IRIUM_STRATUM_MAX_SESSIONS=500
IRIUM_STRATUM_MAX_CONN_PER_IP=20
IRIUM_STRATUM_BAN_THRESHOLD=20
```

That's the full env. There are **no `SOLO_MODE` / `SOLO_FEE_BPS`
variables** — the coinbase always pays the connecting worker's
address directly. There are **no PPLNS payout env vars** either
(`IRIUM_STRATUM_PAYOUT_ENABLED`, `IRIUM_STRATUM_ACCOUNTING_ENABLED`,
`IRIUM_STRATUM_SHARE_DB`, and friends were removed together with
the payout subsystem).

For CPU/GPU clients (older firmware needing lower baseline diff),
make a second env file with `IRIUM_STRATUM_MINER_FAMILY=cpuminer`,
a lower `STRATUM_DEFAULT_DIFF`, and a different `STRATUM_BIND` (e.g.
`0.0.0.0:3335`). Run a second `irium-stratum` service against it.

### systemd unit (`/etc/systemd/system/irium-stratum.service`)

```ini
[Unit]
Description=Irium Stratum Pool (port 3333)
After=iriumd.service
Wants=iriumd.service

[Service]
Type=simple
User=irium
Group=irium
EnvironmentFile=/etc/irium-pool/stratum.env
ExecStart=/bin/bash -lc "ulimit -n 262144; exec /opt/irium-pool/irium-stratum/target/release/irium-stratum"
Restart=on-failure
RestartSec=3
LimitNOFILE=262144

[Install]
WantedBy=multi-user.target
```

Then:

```bash
sudo systemctl daemon-reload
sudo systemctl enable irium-stratum
sudo systemctl start irium-stratum
journalctl -u irium-stratum -f
```

### Operational notes

- `iriumd` must be reachable on `IRIUM_RPC_BASE` and the token must
  be correct. The stratum polls `getblocktemplate` every 1 s.
- AuxPoW is not coinbase-rewardable in this build — if
  `IRIUM_AUXPOW_ACTIVATION_HEIGHT` is set, the stratum bypasses
  AuxPoW until per-session AuxPoW support is wired through.
- Port 3334 (metrics) binds to loopback only. Operators who want to
  expose stats publicly should proxy the JSON through their own HTTP
  server. The official pool uses `stats-proxy.py` for this (see
  `pool/stats-proxy.py` in the repo); that proxy also adds per-miner
  rolling reject rate and hashrate estimation using each worker's
  live `current_diff` value scraped from `/metrics`.
- The mempool now surfaces conflict-detection at submission time
  (no more silent "ghost-tx" pattern when two sends from the same
  wallet pick the same UTXO). See `iriumd` ghost-tx fix commits for
  the underlying mechanism.

---

## Source

- Self-hosted stratum bridge for ASIC mining against your own
  `iriumd`: see [`SOLO_STRATUM.md`](SOLO_STRATUM.md) — that document
  covers `irium-miner --solo-stratum`, which is a simpler alternative
  to `irium-stratum` if you only need to relay work from a single
  ASIC to your own node.
- `irium-stratum` source: `pool/irium-stratum/` in the
  [`iriumlabs/irium`](https://github.com/iriumlabs/irium) repository.
- Desktop UI (`Explorer > Pool Stats` panel): see
  `iriumlabs/irium-core`.

Issues and PRs: <https://github.com/iriumlabs/irium/issues>
