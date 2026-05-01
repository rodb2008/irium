# Irium Pool Operator Guide

## Overview

This guide is for operators who want to run a community Irium mining pool. Irium uses SHA-256d, so standard pool software that supports Bitcoin's getblocktemplate protocol can be adapted to work with Irium nodes.

## Minimum Server Requirements

| Resource | Minimum | Recommended |
|----------|---------|-------------|
| CPU | 2 cores | 4 cores |
| RAM | 2 GB | 4 GB |
| Disk | 40 GB SSD | 100 GB SSD |
| Upload | 20 Mbps | 100 Mbps |
| OS | Ubuntu 22.04+ | Ubuntu 22.04+ |

The Irium node (`iriumd`) itself needs 1 CPU core and 512 MB RAM. The remaining resources are for your Stratum server and database.

## Chain Parameters

| Parameter | Value |
|-----------|-------|
| Algorithm | SHA-256d |
| Block time target | 600 seconds |
| Difficulty algorithm | LWMA (60-block window) — fast retarget |
| Max supply | 100,000,000 IRM |
| Block reward | 50 IRM (current); halves every 210,000 blocks |
| Coinbase maturity | 100 blocks |
| P2P port | 38291 (configurable via `IRIUM_P2P_BIND`) |
| RPC port | 38300 (configurable via `IRIUM_NODE_PORT`) |

## Node Setup

Run a fully-synced `iriumd` instance on your pool server. The pool software talks to the node via its REST API.

```bash
# Using Docker (recommended)
docker pull ghcr.io/iriumlabs/irium:latest
docker run -d \
  -e IRIUM_P2P_BIND=0.0.0.0:38291 \
  -e IRIUM_NODE_PORT=38300 \
  -e IRIUM_NODE_HOST=127.0.0.1 \
  -e IRIUM_STATUS_PORT=8080 \
  -e IRIUM_RPC_TOKEN=your_secret_token_here \
  -p 38291:38291 \
  -v irium-data:/home/irium/.irium \
  --name iriumd \
  ghcr.io/iriumlabs/irium:latest
```

Or build from source on the pool server:

```bash
git clone https://github.com/iriumlabs/irium.git
cd irium
cargo build --release --bin iriumd
export IRIUM_RPC_TOKEN=your_secret_token_here
export IRIUM_NODE_PORT=38300
export IRIUM_NODE_HOST=127.0.0.1
export IRIUM_P2P_BIND=0.0.0.0:38291
export IRIUM_STATUS_PORT=8080
./target/release/iriumd
```

Wait for the node to fully sync before starting pool work:

```bash
curl http://localhost:8080/status | python3 -m json.tool | grep persisted_height
```

## Block Template API

The node exposes a REST getblocktemplate endpoint. Your Stratum server calls this to get work units.

**Get block template:**
```
GET http://localhost:38300/rpc/getblocktemplate
Authorization: Bearer your_secret_token_here
```

**Response:**
```json
{
  "height": 20297,
  "prev_hash": "000000000697c1d50667fbde625d93dbc172f915021c63d42bd79abbde0f5fed",
  "bits": "1c078745",
  "target": "0000000007874500000000000000000000000000000000000000000000000000",
  "time": 1777651024,
  "txs": [],
  "total_fees": 0,
  "coinbase_value": 5000000000,
  "mempool_count": 0
}
```

**Submit a solved block:**
```
POST http://localhost:38300/rpc/submit_block
Authorization: Bearer your_secret_token_here
Content-Type: application/json

{
  "version": 1,
  "prev_hash": "000000000697c1d50667fbde625d93dbc172f915021c63d42bd79abbde0f5fed",
  "merkle_root": "<your computed merkle root>",
  "time": 1777651024,
  "bits": "1c078745",
  "nonce": 2847361,
  "hash": "<solved block hash>",
  "txs": ["<coinbase tx hex>"]
}
```

See `docs/API.md` for the full submit_block request and response format.

## Stratum Configuration

### Stratum Server Selection

The Irium node does not include a built-in Stratum server. You need to deploy a separate Stratum bridge layer that wraps the getblocktemplate REST API.

Suitable options:
- **ckpool** (SHA-256d capable, getblocktemplate native) — https://bitbucket.org/ckolivas/ckpool
- **p2pool** (distributed, SHA-256d) — adapt for Irium chain parameters
- **Custom Stratum bridge** — implement `mining.subscribe`, `mining.authorize`, `mining.notify`, `mining.submit` over the REST template API

### ckpool Configuration

```ini
[btc]
btcd = [{"url": "http://localhost:38300", "auth": "ignored", "pass": "your_secret_token_here"}]
blockpoll = 500
nonce1length = 4
nonce2length = 8
update_interval = 5
```

Note: ckpool expects Bitcoin RPC protocol. You may need a thin adapter that translates getblockwork / getblocktemplate from the Irium REST API to JSON-RPC format.

### Stratum V1 Message Flow

1. **Client connects** to pool on port 3333 (ASIC) or 3335 (CPU/GPU)
2. **Client sends** `mining.subscribe` — pool responds with subscription ID and extranonce values
3. **Client sends** `mining.authorize` with `address.worker` as username, any password
4. **Pool sends** `mining.notify` — includes job ID, prevhash, coinbase parts, merkle branches, version, nbits, ntime
5. **Client submits** `mining.submit` with job ID, extranonce2, ntime, nonce
6. **Pool validates** the submission, assembles the full block, calls `POST /rpc/submit_block`

### Stratum V2 Notes

Stratum V2 is not natively supported by the current pool infrastructure. If you implement Stratum V2 for Irium, open a pull request adding your bridge to the documentation.

## Payout Construction

Irium uses standard P2PKH transaction format. Payouts are standard transactions with inputs from the coinbase or from collected fees and outputs to miner addresses.

- Build a standard P2PKH transaction for each payout
- Use `POST /rpc/submit_tx` to broadcast payout transactions
- Coinbase outputs cannot be spent until block height + 100 (coinbase maturity)

Query a miner's balance:
```
GET http://localhost:38300/rpc/balance?address=Q...
```

List UTXOs to spend:
```
GET http://localhost:38300/rpc/utxos?address=Q...
```

## Monitoring

The node exposes Prometheus-format metrics:

```
GET http://localhost:38300/metrics
```

Key metrics:
```
irium_height 20296
irium_peers 5
irium_mempool_size 0
irium_difficulty 35.73
irium_hashrate 157431698
```

Set up alerts for:
- `irium_peers` dropping to 0 (node isolated from network)
- `irium_height` not advancing for 30+ minutes
- Stratum submission failure rate exceeding 5%

## Getting Listed as an Official Community Pool

Once your pool is operational and has been running stably for at least 7 days:

1. Open a GitHub Issue at https://github.com/iriumlabs/irium with the title "Pool listing request: [your pool name]"
2. Include: pool name, Stratum endpoint, fee structure, payout model, contact method
3. The team will verify the endpoint is reachable and the pool is producing valid blocks, then add it to the official documentation and website

Requirements for listing:
- Stratum endpoint publicly reachable and responding to `mining.subscribe`
- Running a fully-synced Irium node (not a relay-only configuration)
- Valid blocks submitted to mainnet at least once
- Contact method published (Telegram, Discord, or email)

## Stratum Endpoint Verification

To test that your Stratum endpoint is accepting connections:

```bash
printf '{"id":1,"method":"mining.subscribe","params":[]}\n' | nc your-pool-host 3333
```

Expected response contains a subscription ID and extranonce values.

## Support

Telegram: https://t.me/iriumlabs
GitHub Issues: https://github.com/iriumlabs/irium
