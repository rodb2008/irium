# Solo Pool Mining on Irium

The Irium Solo Pool is a stratum endpoint where every connected miner is
paid directly in the coinbase of any block they find — no Pay-Per-Last-N-
Shares (PPLNS) accounting, no waiting period beyond standard 100-block
coinbase maturity. It runs alongside the PPLNS pool on the same VPS, on
a separate TCP port, and uses the same stratum protocol your ASIC
firmware already speaks.

This document explains how solo-pool mining works, how it differs from
the alternatives, how to connect, how the on-chain payout is structured,
how to verify a block you mined, and how to run your own solo pool from
the same open-source binary.

---

## Three ways to mine Irium

| Mode | What you get | When it pays | Variance |
|---|---|---|---|
| **PPLNS pool** (`pool.iriumlabs.org:3333`) | A fraction of every block the pool finds, weighted by the shares you submitted in the last ~10,000-share window | After the found block matures (~100 blocks ≈ 1.5 h) | Low — smooths out luck across many blocks |
| **Solo pool** (`pool.iriumlabs.org:3336`) | Full block reward (minus 1 % pool fee) when **you personally** find a block | Coinbase is on-chain instantly; spendable after 100 confirmations | High — you might find one this hour or none this week |
| **Self-hosted solo** (run your own `iriumd` + `irium-miner`) | Full block reward (no pool fee) when you find a block | Same as solo pool — coinbase is yours, spendable after 100 confirmations | High; same as solo pool |

The solo pool is the middle ground: you get true solo economics (you
keep every block you find) without the operational overhead of running
a full node and template builder yourself.

---

## How the coinbase pays you (the part that matters)

In PPLNS pool mining, the coinbase pays the pool wallet, and the pool
later distributes the reward to share-holders. **In solo pool mining,
the coinbase has two outputs:**

| # | Recipient | Amount | Notes |
|---|---|---|---|
| 0 | Your Irium address (the worker username you authorized with) | **49.5 IRM** (= 50 × 99 %) | Spendable after 100 confirmations, like every coinbase |
| 1 | Pool operator wallet | **0.5 IRM** (= 50 × 1 %) | The pool's fee for running the template builder and the TCP listener |

Total = exactly **50.0 IRM** — the protocol block reward.

This means **the moment your block is accepted by the network you can
already see the IRM destined for your address on-chain** in any block
explorer. You don't have to trust the pool to ever send you anything.

---

## Connection settings

Configure your ASIC firmware (Antminer, Whatsminer, Avalon, Bitaxe,
cgminer, bfgminer, MRR rentals, NiceHash, etc.) exactly like this:

```
URL:      stratum+tcp://pool.iriumlabs.org:3336
Worker:   <YOUR-IRIUM-ADDRESS>            # The username MUST be a Q-prefixed Irium address
Password: x                                # Any non-empty string works; "x" is conventional
```

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
address`. This is intentional — solo mode has no fallback "pool wallet"
to pay to. Authorize fails fast so you don't burn hashes for nothing.

---

## Expected earnings

Solo pool earnings follow a Poisson distribution; on any given share
the probability of finding a block is:

```
P(block) = share_difficulty / network_difficulty
```

For a rough expected-revenue estimate, use:

```
Expected IRM per day ≈ (your_hashrate / network_hashrate) × 0.99 × 50 × blocks_per_day
```

Where:
- `0.99` accounts for the 1 % pool fee.
- `blocks_per_day` is the network's actual block production rate. On
  Irium with a ~1-min observed block time this is roughly 1440, though
  protocol target is 10 min (144 blocks/day) once hashrate is high
  enough that difficulty retargets to it.
- `network_hashrate` can be read from `/rpc/network_hashrate` on your
  local `iriumd`, or from the Network panel in Irium Core's Explorer.

**Important:** variance is high. A miner with 1 % of network hashrate
will *on average* find ~14 blocks per day but might find 0 today and
30 tomorrow. The pool's PPLNS mode trades a 1 % fee for ~zero variance;
solo's 1 % fee buys you no variance smoothing — you accept that risk in
exchange for keeping every block you find as your own coinbase.

---

## How to verify your block was found

Three checks, in order of cost:

### 1. Your stratum miner log

After a block your firmware logs an accepted share with `Block found` /
`solved` / `accepted=true` (the exact wording varies). The pool also
logs `[block] submitted worker=<your_addr>... height=N` in its
journal — but that journal is on the pool host, not visible to you.

### 2. The Explorer "Solo Pool" panel (Irium Core, v1.0.65+)

The Explorer > Pool Stats tab now includes a Solo Pool panel that
shows blocks found, per-miner stats, and the connection info card.
Your worker row appears in the table within ~30 seconds of your first
accepted share.

### 3. On-chain verification (definitive)

Pull the block you suspect you mined from any iriumd:

```bash
curl -s 'http://127.0.0.1:38300/rpc/block?height=<N>' | python3 -m json.tool
```

In the response, decode `tx_hex[0]` (the coinbase). You should see
**two outputs** — the first paying your address (49.5 IRM), the second
paying the pool operator's address (0.5 IRM). If the coinbase has only
one output the block was not mined on the solo pool — it was either
PPLNS or a non-pool miner.

---

## FAQ

### Why use the solo pool instead of joining the PPLNS pool?

Choose solo if you want to keep every block you find as a coinbase
output paid to your address. Choose PPLNS if you prefer steadier daily
revenue and don't mind the pool wallet sitting on each coinbase until
it distributes 49.5 IRM across the share window.

### Why use the solo pool instead of running your own node?

The solo pool exists so you don't have to:

- Run `iriumd` 24/7 and keep its chain state synced.
- Build templates (`/rpc/getblocktemplate` polling).
- Run a stratum listener on a TCP port your ASIC can reach.
- Open inbound P2P (38291) on your home router.

For 1 % of one block per ~24 hours of mining (at typical hashrates),
the pool handles all of the above. If you prefer the zero-fee path and
already operate a full node, run `iriumd` + `irium-miner` directly —
that's documented separately under "Self-hosted solo".

### Why does the worker username have to be a valid address?

Because the coinbase template literally encodes your address as the
P2PKH output script. There is no fallback "pool wallet" to pay you
later if the username is malformed — solo mode has no PPLNS queue. The
authorize handler refuses connections whose username doesn't decode to
a 20-byte pkh.

### Is the 1 % fee non-negotiable?

The fee is per-pool-operator and is the operator's decision. The
official pool at `pool.iriumlabs.org:3336` runs `IRIUM_STRATUM_SOLO_FEE_BPS=100`
(= 1 %). Anyone running their own solo pool can set it lower (or to
zero) by editing their `stratum-solo.env`.

### Can I switch between PPLNS and solo without changing my address?

Yes. Both modes accept the same Irium address as the worker username.
Change the port (3333 vs 3336) and the rig restarts authorize with the
new pool. Your existing wallet, address, and history are unaffected.

### What happens if the pool is offline?

Your rig loses its stratum connection and stops mining until the pool
is back. You lose nothing — there is no escrowed reward sitting on the
pool waiting to pay out. If you want failover, configure your ASIC
firmware with the PPLNS pool as a secondary URL.

---

## Running your own solo pool

The solo-pool feature is built into the open-source `irium-stratum`
binary from the upstream `iriumlabs/irium` repository. You can run it
yourself on the same VPS as your `iriumd`, or anywhere with TCP
reachability to your `iriumd`'s RPC port.

### Build

```bash
git clone https://github.com/iriumlabs/irium.git
cd irium/pool/irium-stratum
cargo build --release
# Binary lands at target/release/irium-stratum
```

### Environment file (`/etc/irium-pool/stratum-solo.env`)

```env
IRIUM_RPC_BASE=http://127.0.0.1:38300
IRIUM_RPC_TOKEN=<your iriumd RPC token>
STRATUM_BIND=0.0.0.0:3336
STRATUM_METRICS_BIND=127.0.0.1:3338
STRATUM_DEFAULT_DIFF=10000
IRIUM_STRATUM_MINER_FAMILY=asic
IRIUM_STRATUM_VARDIFF_ENABLED=1
IRIUM_STRATUM_VARDIFF_MIN_DIFF=1
IRIUM_STRATUM_VARDIFF_MAX_DIFF=2000000
IRIUM_STRATUM_MAX_SESSIONS=200
IRIUM_STRATUM_MAX_CONN_PER_IP=20
IRIUM_STRATUM_BAN_THRESHOLD=20
# Solo-mode switches:
IRIUM_STRATUM_SOLO_MODE=1
IRIUM_STRATUM_SOLO_FEE_BPS=100
```

The fee field is in basis points: `100` = 1.00 %, `50` = 0.50 %, `0` =
zero fee (still emits a 2-output coinbase, just with a 0-value second
output that goes to your pool wallet). The pool wallet for the 1 %
fee is hard-compiled — `crate::payout::POOL_PAYOUT_PKH_BYTES`. Change
it before building if you want fees to land somewhere other than the
official pool wallet.

### systemd unit (`/etc/systemd/system/irium-stratum-solo.service`)

```ini
[Unit]
Description=Irium Stratum Solo Pool (port 3336)
After=iriumd.service
Wants=iriumd.service

[Service]
Type=simple
User=irium
Group=irium
EnvironmentFile=/etc/irium-pool/stratum-solo.env
ExecStart=/bin/bash -lc "ulimit -n 262144; exec /opt/irium-pool/irium-stratum-solo/irium-stratum"
Restart=on-failure
RestartSec=3
LimitNOFILE=262144

[Install]
WantedBy=multi-user.target
```

Then:

```bash
sudo systemctl daemon-reload
sudo systemctl enable irium-stratum-solo
sudo systemctl start irium-stratum-solo
journalctl -u irium-stratum-solo -f
```

### Operational notes

- `iriumd` must be reachable on `IRIUM_RPC_BASE` and the token must be
  correct. The solo pool calls `getblocktemplate` every 1 s.
- AuxPoW + solo mode is not supported in this build. If
  `IRIUM_AUXPOW_ACTIVATION_HEIGHT` is set, solo mode bypasses the
  AuxPoW path; do not run solo on a chain past AuxPoW activation until
  per-session AuxPoW is wired through.
- The PPLNS maturity-poller spawns regardless of solo mode but does
  nothing in solo (no blocks are queued). State files live at
  `/opt/irium-pool/*.json` and are harmless to leave around.
- Port 3338 (metrics) binds to loopback only. Operators who want to
  expose stats publicly should proxy the JSON through their own HTTP
  server (the official pool uses `stats-proxy.py` for this; the
  `/solo-stats` and `/solo-miners` endpoints there are an example of
  what a public scrape looks like).

---

## Source

- Solo-mode patch on `irium-stratum`: commit
  [`f709a04`](https://github.com/iriumlabs/irium/commit/f709a04)
- `stats-proxy.py` `/solo-stats` + `/solo-miners` endpoints: subsequent
  commit on the same repo.
- Desktop UI (`Explorer > Pool Stats > Solo Pool` panel):
  `iriumlabs/irium-core` v1.0.65.

Issues and PRs: <https://github.com/iriumlabs/irium/issues>
