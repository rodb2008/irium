# Irium Pool Operator Guide

## Overview

This guide is for operators who want to run a community Irium mining pool. Irium uses SHA-256d, so standard pool software that supports Bitcoin's getblocktemplate protocol can be adapted to work with Irium nodes.

AuxPoW merged mining activates at height 26,347 (~12 June 2026). The `irium-stratum` server handles both standard and AuxPoW modes automatically — no manual intervention required at activation.

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
| Difficulty algorithm | LWMA v2 (30-block window) |
| Max supply | ~24,500,000 IRM |
| Block reward | 50 IRM (current); halves every 210,000 blocks |
| Coinbase maturity | 100 blocks |
| AuxPoW activation height | 26,347 (~12 June 2026) |
| P2P port | 38291 (configurable via `IRIUM_P2P_BIND`) |
| RPC port | 38300 (configurable via `IRIUM_NODE_PORT`) |

## Node Setup

Run a fully-synced `iriumd` instance on your pool server. The pool software talks to the node via its REST API.

```bash
# Build from source (recommended)
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
  "height": 20299,
  "prev_hash": "000000000735e2852fc54680a93b982de52592594b9fbfbeda711f648598e17e",
  "bits": "1c078745",
  "target": "0000000007874500000000000000000000000000000000000000000000000000",
  "time": 1746097200,
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
  "height": 20300,
  "header": {
    "version": 1,
    "prev_hash": "000000000735e2852fc54680a93b982de52592594b9fbfbeda711f648598e17e",
    "merkle_root": "<your computed merkle root>",
    "time": 1746097260,
    "bits": "1c078745",
    "nonce": 2847361,
    "hash": "<solved block hash>"
  },
  "tx_hex": ["<coinbase tx hex>"],
  "submit_source": "pool_stratum"
}
```

For AuxPoW blocks, include the additional field:
```json
{
  "auxpow_hex": "<hex-encoded AuxPoW extension bytes>"
}
```

See `docs/API.md` for the full field reference.

## Stratum Server (irium-stratum)

The repository includes a complete Stratum V1 server at `pool/irium-stratum`. This is the recommended deployment for all pool operators. It handles standard mining today and AuxPoW merged mining automatically after activation height.

### Building

```bash
cd irium/pool/irium-stratum
cargo build --release
```

### Configuration

All configuration is via environment variables. No hardcoded values.

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `IRIUM_RPC_BASE` | Yes | — | Base URL of the iriumd REST API |
| `IRIUM_RPC_TOKEN` | Yes | — | Bearer token for iriumd authentication |
| `STRATUM_BIND` | No | `0.0.0.0:3333` | Stratum listener address |
| `STRATUM_METRICS_BIND` | No | `127.0.0.1:3334` | Prometheus metrics listener (loopback only) |
| `STRATUM_DEFAULT_DIFF` | No | `16` | Starting share difficulty |
| `STRATUM_EXTRANONCE1_SIZE` | No | `4` | Extranonce1 size in bytes |
| `TEMPLATE_REFRESH_MS` | No | `1000` | Template poll interval in milliseconds |
| `IRIUM_TEMPLATE_MAX_AGE_SECONDS` | No | `60` | Maximum template age before forced refresh |
| `IRIUM_POW_LIMIT_HEX` | No | mainnet default | PoW limit for share validation |
| `IRIUM_HASH_CMP_MODE` | No | `be` | Hash comparison mode: `be` or `le` |
| `IRIUM_STRATUM_SOFT_ACCEPT_INVALID_SHARES` | No | `true` | Accept invalid shares without rejecting the connection |
| `IRIUM_STRATUM_MINER_FAMILY` | No | auto | Miner family hint for protocol handling |
| `IRIUM_STRATUM_ADAPTER_MODE` | No | auto | Adapter mode selection |
| `IRIUM_STRATUM_NATIVE_REWARDABLE_ENABLED` | No | `false` | Enable native rewardable adapter |
| `IRIUM_STRATUM_SHARECHECK_SAMPLES` | No | `3` | Share validation sample count |
| `IRIUM_STRATUM_VARDIFF_ENABLED` | No | `true` | Enable variable difficulty |
| `IRIUM_STRATUM_VARDIFF_MIN_DIFF` | No | `1` | Minimum vardiff difficulty |
| `IRIUM_STRATUM_VARDIFF_MAX_DIFF` | No | `1024` | Maximum vardiff difficulty |
| `IRIUM_STRATUM_VARDIFF_TARGET_SHARE_SECS` | No | `15` | Target seconds between shares |
| `IRIUM_STRATUM_VARDIFF_RETARGET_SECS` | No | `30` | Vardiff adjustment interval |
| `IRIUM_STRATUM_COINBASE_BIP34` | No | `true` | Include BIP34 height in coinbase |
| `IRIUM_STRATUM_FOUND_BLOCKS_FILE` | No | `/opt/irium-pool/data/found_blocks.jsonl` | Path for found block log |
| `IRIUM_STRATUM_KEEPALIVE_NOTIFY_SECS` | No | `120` | Keepalive notify interval |
| `IRIUM_AUXPOW_ACTIVATION_HEIGHT` | No | `26347` | AuxPoW activation height override (for testnet/devnet only) |
| `LOG_LEVEL` | No | `info` | Log level: `trace`, `debug`, `info`, `warn`, `error` |

### Running

```bash
export IRIUM_RPC_BASE=http://127.0.0.1:38300
export IRIUM_RPC_TOKEN=your_secret_token_here
export STRATUM_BIND=0.0.0.0:3333
export IRIUM_STRATUM_VARDIFF_ENABLED=true
export IRIUM_STRATUM_FOUND_BLOCKS_FILE=/opt/irium-pool/data/found_blocks.jsonl
export LOG_LEVEL=info

./target/release/irium-stratum
```

### AuxPoW Mode

The server switches to AuxPoW mode automatically when the template height reaches 26,347. In AuxPoW mode:

- A fixed Irium coinbase is built for each job (extranonce is zero)
- The Irium block hash is computed and embedded in a parent coinbase commitment
- Miners solve the parent block — any SHA-256d ASIC, no changes required
- Winning shares are submitted to iriumd with the full AuxPoW proof attached

Miners connecting to the pool see no change in the Stratum protocol. The Stratum `mining.notify` message format is identical; only the coinbase and prevhash values change internally.

## Stratum V1 Message Flow

1. **Client connects** to pool on the configured Stratum port
2. **Client sends** `mining.subscribe` — pool responds with subscription ID and extranonce values
3. **Client sends** `mining.authorize` with `address.worker` as username, any password
4. **Pool sends** `mining.notify` — includes job ID, prevhash, coinbase parts, Merkle branches, version, nbits, ntime
5. **Client submits** `mining.submit` with job ID, extranonce2, ntime, nonce
6. **Pool validates** the submission and calls `POST /rpc/submit_block`

## Payout Construction

Irium uses standard P2PKH transaction format. Payouts are standard transactions.

- Build a standard P2PKH transaction for each payout
- Use `POST /rpc/submit_tx` to broadcast payout transactions (see `docs/API.md`)
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

The Stratum server exposes Prometheus metrics at the configured metrics bind address:

```
GET http://127.0.0.1:3334/metrics
```

Key metrics:
```
irium_stratum_shares_accepted_total
irium_stratum_shares_rejected_total
irium_stratum_blocks_found_total
irium_stratum_connected_workers
```

The node also exposes metrics:
```
GET http://localhost:8080/status
```

Set up alerts for:
- Peer count dropping to 0 (node isolated from network)
- Chain height not advancing for 30+ minutes
- Stratum submission failure rate exceeding 5%

## Stratum V2 Notes

Stratum V2 is not currently implemented. If you implement Stratum V2 for Irium, open a pull request adding your implementation.

## Getting Listed as an Official Community Pool

Once your pool is operational and has been running stably for at least 7 days:

1. Open a GitHub Issue at https://github.com/iriumlabs/irium with title "Pool listing request: [your pool name]"
2. Include: pool name, Stratum endpoint, fee structure, payout model, contact method
3. The team will verify the endpoint is reachable and the pool is producing valid blocks, then list it on the official documentation and website

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
Pool operator guide: https://github.com/iriumlabs/irium/blob/main/docs/POOL-OPERATOR.md
Merged mining guide: https://github.com/iriumlabs/irium/blob/main/docs/MERGED-MINING.md
