# Irium RPC API Reference

This document covers every HTTP endpoint exposed by `iriumd`. All amounts are in satoshis. 1 IRM = 100,000,000 satoshis.

Default ports:
- P2P listener: `38291`
- RPC / explorer API: `38300`
- Lightweight `/status` server: `8080` (loopback only by default; override with `IRIUM_STATUS_HOST` / `IRIUM_STATUS_PORT`)

Address prefixes:
- `Q` — single-sig P2PKH (Base58Check version byte `0x39`)
- `P` — multisig (Base58Check version byte `0x28`)

## Authentication

If the environment variable `IRIUM_RPC_TOKEN` is set to a non-empty value on the node, protected endpoints require an `Authorization: Bearer <token>` header.

The following endpoints are always public (no token required):

- `GET /status`
- `GET /peers`
- `GET /metrics`
- `GET /rpc/balance`
- `GET /rpc/utxos`
- `GET /rpc/history`
- `GET /rpc/fee_estimate`
- `GET /rpc/block`
- `GET /rpc/block_by_hash`
- `GET /rpc/blocks`
- `GET /rpc/tx`
- `GET /rpc/utxo`
- `GET /rpc/richlist`
- `GET /rpc/network_hashrate`
- `GET /rpc/mining_metrics`
- `GET /offers/feed`
- `GET /explorer/*`
- `GET /ws` · `GET /events` (streaming endpoints; see [WEBSOCKET.md](WEBSOCKET.md))

All wallet endpoints (`/wallet/...`) and settlement endpoints (`/rpc/createagreement`, etc.) require authentication if a token is configured.

### Where are the agreement/proof/policy list endpoints?

For programmatic access:

- Agreements list — **`GET /explorer/agreements`** (paginated, no auth). The other agreement-* endpoints (`/rpc/agreementstatus`, `/rpc/agreementtimeline`, etc.) are POST and operate on a single agreement at a time.
- Proofs list — **`GET /explorer/proofs`** (paginated, no auth, accepts `?agreement_hash=` filter). Alternative: **`POST /rpc/listproofs`** (auth required if token set) which takes an `{"agreement_hash": "…"}` body and returns the same data without pagination.
- Policies list — **`POST /rpc/listpolicies`** (auth required if token set; no public list endpoint).

There are no `GET /rpc/agreements`, `GET /rpc/proofs`, or `GET /rpc/policies` routes — those would shadow the per-resource POST endpoints. Use the explorer routes above for plain HTTP GET access.

---

## Node Status and Health

### `GET /status`

Returns the current state of the node including chain tip, peer count, and anchor status.

**Parameters:** None

**Example request:**
```
curl http://localhost:38300/status
```

**Example response:**
```json
{
  "height": 20296,
  "genesis_hash": "0000000028f25d65557e9d8d9e991f516c00d68f5aeae10b750645b398bd10a3",
  "network_era": "Early Miner Era",
  "peer_count": 4,
  "anchor_loaded": true,
  "anchors_digest": "0475f8e5b5daad5bfbdcfe323b743b9b3388d1774862a3addd81493dca800a23",
  "node_id": "675de6172873fd4ecd552f795ef7571fd400375fd19e89ae8ebc0bc8bc9fdaf7",
  "sybil_difficulty": 10,
  "best_header_tip": {
    "height": 20296,
    "hash": "000000000697c1d50667fbde625d93dbc172f915021c63d42bd79abbde0f5fed"
  },
  "persisted_height": 20296,
  "persist_queue_len": 0
}
```

**Response fields:**

| Field | Type | Description |
|-------|------|-------------|
| `height` | integer | Current chain height (best known block) |
| `genesis_hash` | string | SHA256d hash of block 0 |
| `network_era` | string | Human-readable name for the current emission era |
| `peer_count` | integer | Number of currently connected peers |
| `anchor_loaded` | boolean | Whether the trust anchor file is loaded |
| `anchors_digest` | string | SHA256 digest of the loaded anchor set |
| `node_id` | string | This node's public identity hash |
| `sybil_difficulty` | integer | Current sybil-resistance proof-of-work difficulty |
| `best_header_tip.height` | integer | Height of the best known block header |
| `best_header_tip.hash` | string | Hash of the best known block header |
| `persisted_height` | integer | Height of the last block fully written to disk |
| `persist_queue_len` | integer | Number of blocks queued for disk write |

---

### `GET /peers`

Returns a list of all currently known peers.

**Parameters:** None

**Example request:**
```
curl http://localhost:38300/peers
```

**Example response:**
```json
{
  "peers": [
    {
      "multiaddr": "/ip4/92.47.113.196/tcp/50779",
      "agent": null,
      "source": "live",
      "height": 20296,
      "last_seen": 1777648915.808,
      "dialable": false,
      "last_successful_handshake": 1777646784.624
    }
  ]
}
```

**Response fields (per peer):**

| Field | Type | Description |
|-------|------|-------------|
| `multiaddr` | string | Peer network address in multiaddr format |
| `agent` | string or null | Peer software version string, if advertised |
| `source` | string | How this peer was discovered: `live`, `seed`, `gossip` |
| `height` | integer | Last known chain height reported by this peer |
| `last_seen` | float | Unix timestamp when this peer was last active |
| `dialable` | boolean | Whether the node believes this peer is reachable outbound |
| `last_successful_handshake` | float | Unix timestamp of last completed handshake |

---

### `POST /admin/add-seed`

Adds a peer address to the runtime seed list and attempts an immediate connection.
Requires `IRIUM_RPC_TOKEN` authentication if configured.

**Request body:** JSON object with the peer address.

**Example request:**
```
curl -X POST http://localhost:38300/admin/add-seed \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer <token>' \
  -d '{"addr": "1.2.3.4:38291"}'
```

**Example response:**
```json
{ "added": true }
```

**Error codes:**

| Code | Meaning |
|------|---------|
| 400 | Invalid address format |
| 401 | Missing or invalid authentication token |

---

### `GET /metrics`

Returns Prometheus-format plain-text metrics for use with monitoring systems.

**Parameters:** None

**Example request:**
```
curl http://localhost:38300/metrics
```

**Example response:**
```
irium_height 20296
irium_peers 4
irium_anchor_loaded 1
irium_tip_hash 000000000697c1d50667fbde625d93dbc172f915021c63d42bd79abbde0f5fed
irium_mempool_size 0
irium_anchor_digest 0475f8e5b5daad5bfbdcfe323b743b9b3388d1774862a3addd81493dca800a23
```

---

## Chain Queries

### `GET /rpc/balance`

Returns the balance and UTXO count for an address.

**Query parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `address` | Yes | Irium address (Q-prefix, Base58Check) |

**Example request:**
```
curl "http://localhost:38300/rpc/balance?address=Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa"
```

**Example response:**
```json
{
  "address": "Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa",
  "pkh": "79dbb6fd908884fc994b8aa34dcef392fe2d9d65",
  "balance": 1945000000000,
  "mined_balance": 1945000000000,
  "utxo_count": 389,
  "mined_blocks": 389,
  "height": 20296
}
```

**Response fields:**

| Field | Type | Description |
|-------|------|-------------|
| `address` | string | The queried address |
| `pkh` | string | Public key hash (hex) corresponding to this address |
| `balance` | integer | Total spendable balance in satoshis |
| `mined_balance` | integer | Portion of balance from coinbase outputs |
| `utxo_count` | integer | Number of unspent outputs |
| `mined_blocks` | integer | Number of blocks mined to this address |
| `height` | integer | Chain height at time of query |

---

### `GET /rpc/utxos`

Returns all unspent transaction outputs for an address.

**Query parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `address` | Yes | Irium address |

**Example request:**
```
curl "http://localhost:38300/rpc/utxos?address=Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa"
```

**Example response:**
```json
{
  "address": "Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa",
  "pkh": "79dbb6fd908884fc994b8aa34dcef392fe2d9d65",
  "height": 20296,
  "utxos": [
    {
      "txid": "cb7d25dc615df7e64726c171b18f401c916133f9335ed5153e3e14312b001b12",
      "index": 0,
      "value": 5000000000,
      "height": 5143,
      "is_coinbase": true,
      "script_pubkey": "76a91479dbb6fd908884fc994b8aa34dcef392fe2d9d6588ac"
    }
  ]
}
```

**Response fields (per UTXO):**

| Field | Type | Description |
|-------|------|-------------|
| `txid` | string | Transaction ID containing this output |
| `index` | integer | Output index within the transaction |
| `value` | integer | Value in satoshis |
| `height` | integer | Block height where this output was confirmed |
| `is_coinbase` | boolean | Whether this output is from a coinbase transaction |
| `script_pubkey` | string | Locking script (hex) |

---

### `GET /rpc/utxo`

Returns a single UTXO by transaction ID and output index.

**Query parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `txid` | Yes | Transaction ID (hex) |
| `index` | Yes | Output index (integer) |

**Example request:**
```
curl "http://localhost:38300/rpc/utxo?txid=cb7d25dc615df7e64726c171b18f401c916133f9335ed5153e3e14312b001b12&index=0"
```

---

### `GET /rpc/history`

Returns the transaction history for an address.

**Query parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `address` | Yes | Irium address |

**Example request:**
```
curl "http://localhost:38300/rpc/history?address=Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa"
```

---

### `GET /rpc/tx`

Returns a transaction by its ID.

**Query parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `txid` | Yes | Transaction ID (hex) |

**Example request:**
```
curl "http://localhost:38300/rpc/tx?txid=17edd1b2363712e2f380ba6e10510f9ff3a2b45881433d718859d1bbb116293c"
```

**Example response:**
```json
{
  "txid": "17edd1b2363712e2f380ba6e10510f9ff3a2b45881433d718859d1bbb116293c",
  "height": 1,
  "index": 0,
  "block_hash": "0000000064d3cb70a6b44320608957b6b02e7f876a37e35725d795811c39ca8d",
  "inputs": 1,
  "outputs": 1,
  "output_value": 5000000000,
  "is_coinbase": true,
  "tx_hex": "0100000001200000000000000000000000000000000000000000000000000000000000000000ffffffff07426c6f636b2031ffffffff0100f2052a010000001976a91479dbb6fd908884fc994b8aa34dcef392fe2d9d6588ac00000000"
}
```

**Response fields:**

| Field | Type | Description |
|-------|------|-------------|
| `txid` | string | Transaction ID |
| `height` | integer | Block height where confirmed, or -1 if unconfirmed |
| `index` | integer | Index within the block |
| `block_hash` | string | Hash of the containing block |
| `inputs` | integer | Number of inputs |
| `outputs` | integer | Number of outputs |
| `output_value` | integer | Total output value in satoshis |
| `is_coinbase` | boolean | Whether this is a coinbase transaction |
| `tx_hex` | string | Full serialised transaction (hex) |

**Error codes:**

| Code | Meaning |
|------|---------|
| 404 | Transaction not found |

---

### `GET /rpc/block`

Returns a block by height.

**Query parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `height` | Yes | Block height (integer) |

**Example request:**
```
curl "http://localhost:38300/rpc/block?height=1"
```

**Example response:**
```json
{
  "header": {
    "bits": "1d00ffff",
    "hash": "0000000064d3cb70a6b44320608957b6b02e7f876a37e35725d795811c39ca8d",
    "merkle_root": "3c2916b1bbd15988713d438158b4a2f39f0f51106eba80f3e2123736b2d1ed17",
    "nonce": 1307954509,
    "prev_hash": "0000000028f25d65557e9d8d9e991f516c00d68f5aeae10b750645b398bd10a3",
    "time": 1767591035,
    "version": 1
  },
  "height": 1,
  "miner_address": "Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa",
  "submit_source": null,
  "tx_hex": [
    "0100000001200000000000000000000000000000000000000000000000000000000000000000ffffffff07426c6f636b2031ffffffff0100f2052a010000001976a91479dbb6fd908884fc994b8aa34dcef392fe2d9d6588ac00000000"
  ]
}
```

**Response fields:**

| Field | Type | Description |
|-------|------|-------------|
| `header.bits` | string | Compact difficulty target |
| `header.hash` | string | Block hash |
| `header.merkle_root` | string | Merkle root of transactions |
| `header.nonce` | integer | Proof-of-work nonce |
| `header.prev_hash` | string | Hash of the previous block |
| `header.time` | integer | Block timestamp (Unix) |
| `header.version` | integer | Block version |
| `height` | integer | Block height |
| `miner_address` | string | Address that received the coinbase reward |
| `submit_source` | string or null | How this block was submitted (`stratum`, `rpc`, or null) |
| `tx_hex` | array of strings | Serialised transactions in this block (hex) |

**Error codes:**

| Code | Meaning |
|------|---------|
| 404 | Block not found at this height |

---

### `GET /rpc/block_by_hash`

Returns a block by its hash.

**Query parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `hash` | Yes | Block hash (hex) |

**Example request:**
```
curl "http://localhost:38300/rpc/block_by_hash?hash=000000000697c1d50667fbde625d93dbc172f915021c63d42bd79abbde0f5fed"
```

Response structure is identical to `GET /rpc/block`.

---

### `GET /rpc/blocks`

Returns a contiguous range of blocks starting at a given height. Used by
block explorers and the desktop wallet's Explorer page for batch backfill.

**Query parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `from` | Yes | Start height (integer, inclusive) |
| `count` | Yes | Number of blocks to return. Capped at 500 per request. |

**Example request:**
```
curl "http://localhost:38300/rpc/blocks?from=20000&count=10"
```

**Example response:**
```json
{
  "from": 20000,
  "count": 10,
  "blocks": [ /* same shape as /rpc/block, one entry per height */ ]
}
```

**Error codes:**

| Code | Meaning |
|------|---------|
| 404 | `from` is past the current tip |

---

### `GET /rpc/richlist`

Returns the top N IRM holders ranked by spendable on-chain balance at the
current tip. Added in iriumd v1.9.17. Always public — no authentication
required. Computed in-memory over the live UTXO set so the response is
authoritative for the tip and reflects every confirmation, not a stale
index.

**Query parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `limit` | No | Maximum number of entries to return (default: 100; clamped to 1000) |

**Example request:**
```
curl "http://localhost:38300/rpc/richlist?limit=10"
```

**Example response:**
```json
{
  "total_supply_sats": 105450000000000,
  "generated_at_height": 22058,
  "entries": [
    {
      "rank": 1,
      "address": "Q9KxBRfrnb6v9Vb8vuHjwkZaxj3ZRhJWpg",
      "balance_sats": 3175000000000,
      "utxo_count": 635
    },
    {
      "rank": 2,
      "address": "Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa",
      "balance_sats": 1945000000000,
      "utxo_count": 389
    }
  ]
}
```

**Response fields:**

| Field | Type | Description |
|-------|------|-------------|
| `total_supply_sats` | integer | Total minted supply at the snapshot height (includes coinbase rewards + genesis premine) |
| `generated_at_height` | integer | Chain tip height at which the snapshot was taken |
| `entries[].rank` | integer | 1-based rank by balance |
| `entries[].address` | string | Irium address (`Q…` single-sig or `P…` multisig) |
| `entries[].balance_sats` | integer | Spendable balance in satoshis |
| `entries[].utxo_count` | integer | Number of unspent outputs at the address |

---

### `GET /rpc/fee_estimate`

Returns the current fee estimate for transaction construction.

**Parameters:** None

**Example request:**
```
curl http://localhost:38300/rpc/fee_estimate
```

**Example response:**
```json
{
  "min_fee_per_byte": 1.0,
  "mempool_size": 0
}
```

**Response fields:**

| Field | Type | Description |
|-------|------|-------------|
| `min_fee_per_byte` | float | Minimum fee in satoshis per serialised byte |
| `mempool_size` | integer | Number of transactions currently in the mempool |

---

## Mining

### `GET /rpc/network_hashrate`

Returns the estimated network hashrate and current mining metrics.

**Parameters:** None

**Example request:**
```
curl http://localhost:38300/rpc/network_hashrate
```

**Example response:**
```json
{
  "tip_height": 20296,
  "current_network_era": "Early Miner Era",
  "difficulty": 35.73702327800689,
  "hashrate": 157431698.348234,
  "avg_block_time": 974.9583333333334,
  "window": 120,
  "sample_blocks": 120
}
```

**Response fields:**

| Field | Type | Description |
|-------|------|-------------|
| `tip_height` | integer | Current chain tip height |
| `current_network_era` | string | Human-readable emission era name |
| `difficulty` | float | Current proof-of-work difficulty |
| `hashrate` | float | Estimated network hashrate in hashes per second |
| `avg_block_time` | float | Average block time in seconds over the sample window |
| `window` | integer | Sample window size in blocks |
| `sample_blocks` | integer | Number of blocks actually sampled |

---

### `GET /rpc/mining_metrics`

Returns extended mining metrics used by pool software and miners.

**Parameters:** None

**Example request:**
```
curl http://localhost:38300/rpc/mining_metrics
```

---

### `GET /rpc/getblocktemplate`

Returns a block template for mining. Used internally by `irium-miner` and compatible pool software.

**Parameters:** None

**Example request:**
```
curl http://localhost:38300/rpc/getblocktemplate
```

---

### `POST /rpc/submit_block`

Submits a solved block to the network.

**Request body:** JSON object containing the solved block.

**Example request:**
```
curl -X POST http://localhost:38300/rpc/submit_block \
  -H "Content-Type: application/json" \
  -d '{"block_hex": "<solved_block_hex>"}'
```

---

## Transactions

### `POST /rpc/submit_tx`

Broadcasts a signed transaction to the network.

**Request body:** JSON object containing the signed transaction.

**Example request:**
```
curl -X POST http://localhost:38300/rpc/submit_tx \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <token>" \
  -d '{"tx_hex": "<signed_tx_hex>"}'
```

**Error codes:**

| Code | Meaning |
|------|---------|
| 400 | Invalid transaction (malformed, invalid signature, double spend) |
| 401 | Missing or invalid authentication token |

---

## Marketplace

### `GET /offers/feed`

Returns the public OTC marketplace offer feed. No authentication required.

**Parameters:** None

**Example request:**
```
curl http://localhost:38300/offers/feed
```

**Example response:**
```json
{
  "count": 13,
  "exported_at": 1777648997,
  "offers": [
    {
      "offer_id": "d1-gossip-t4",
      "seller_address": "Q9KxBRfrnb6v9Vb8vuHjwkZaxj3ZRhJWpg",
      "seller_pubkey": "03e918af472e63de044c983df9f09bae57d4c78a70998d5d5fded408672886f868",
      "amount_irm": 100000000,
      "payment_method": "bank-transfer",
      "status": "open",
      "timeout_height": 25000,
      "created_at": 1777624133
    }
  ]
}
```

**Response fields:**

| Field | Type | Description |
|-------|------|-------------|
| `count` | integer | Total number of offers in the feed |
| `exported_at` | integer | Unix timestamp when the feed was generated |
| `offers[].offer_id` | string | Unique offer identifier |
| `offers[].seller_address` | string | Seller's Irium address |
| `offers[].seller_pubkey` | string | Seller's compressed public key (hex) |
| `offers[].amount_irm` | integer | Offer amount in satoshis |
| `offers[].payment_method` | string | Payment method description |
| `offers[].status` | string | Offer status: `open`, `taken`, or `settled` |
| `offers[].timeout_height` | integer | Block height after which the offer expires |
| `offers[].created_at` | integer | Unix timestamp when the offer was created |

---

## Explorer Endpoints

These endpoints power public block explorers and node-status dashboards. All
are GET, always public (no token required), and CORS-enabled so they can be
called directly from a browser.

### `GET /explorer/agreements`

Paginated list of agreements known to this node, newest first.

**Query parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `page` | No | 1-based page index (default `1`) |
| `limit` | No | Page size, clamped to `[1, 100]` (default `25`) |

**Example response:**
```json
{
  "agreements": [
    {
      "hash": "96dfc2a96630e6d6…",
      "agreement_id": "offer-1777888495-1777888517",
      "template_type": "otc",
      "total_amount": 50000000,
      "creation_time": 1777888517,
      "parties": [
        {"role": "seller", "display_name": "", "address": "Q…"},
        {"role": "buyer",  "display_name": "", "address": "Q…"}
      ]
    }
  ],
  "total": 132,
  "page": 1,
  "limit": 25
}
```

---

### `GET /explorer/agreement/:hash`

Full detail for a single agreement: the raw agreement JSON, derived lifecycle
state, and every proof submitted against it.

```
curl http://localhost:38300/explorer/agreement/96dfc2a96630e6d6…
```

Response includes `lifecycle` (deterministic state derived from on-chain
linked transactions) and `proofs[]` with each proof's `status` (`active` /
`expired`).

---

### `GET /explorer/proofs`

Paginated list of proofs known to this node. Optionally filter to a single
agreement.

**Query parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `agreement_hash` | No | Only return proofs for this agreement |
| `page` | No | 1-based page index (default `1`) |
| `limit` | No | Page size, clamped to `[1, 100]` (default `25`) |

Each entry carries: `proof_id`, `proof_type`, `agreement_hash`, `attested_by`,
`attestation_time`, and a derived `status` field (`active` while the proof
has not expired at the current tip; `expired` once `expires_at_height`
is in the past).

---

### `GET /explorer/reputation/:pubkey`

Reputation summary for an attestor or seller, derived locally on this node
from agreement and proof storage. Returns:

```json
{
  "pubkey": "03e918af472e63de…",
  "total_agreements_as_seller": 12,
  "proofs_submitted": 47,
  "note": "Reputation derived from locally stored agreement and proof data on this node."
}
```

The richer reputation fields (`default_count`, `risk_signal`, `ranking_score`)
are computed wallet-side by `irium-wallet reputation-show`; this RPC returns
the minimal explorer summary.

---

### `GET /explorer/stats`

Network-wide settlement statistics: total agreements, total proofs, proof
type counts, current chain height, peer count. Useful for explorer
dashboards.

---

## HTLC Endpoints

### `POST /rpc/createhtlc`

Creates a new Hash Time-Locked Contract output.

**Request body:** JSON with HTLC parameters (secret hash, recipient, refund address, timeout height).

**Example request:**
```
curl -X POST http://localhost:38300/rpc/createhtlc \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <token>" \
  -d '{
    "secret_hash": "<32bytehex>",
    "recipient_address": "Q...",
    "refund_address": "Q...",
    "timeout_height": 20500
  }'
```

---

### `POST /rpc/decodehtlc`

Decodes an HTLC script, returning its parameters.

**Request body:** JSON containing the HTLC script hex.

---

### `POST /rpc/claimhtlc`

Builds a transaction to claim an HTLC output using the preimage secret.

**Request body:** JSON with the HTLC UTXO reference and the secret.

---

### `POST /rpc/refundhtlc`

Builds a transaction to refund an HTLC output after the timeout height.

**Request body:** JSON with the HTLC UTXO reference.

---

### `GET /rpc/inspecthtlc`

Inspects an HTLC output on-chain.

**Query parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `txid` | Yes | Transaction ID containing the HTLC output |
| `index` | Yes | Output index |

**Example request:**
```
curl "http://localhost:38300/rpc/inspecthtlc?txid=<txid>&index=0"
```

---

## Settlement Endpoints

All settlement endpoints use `POST` and accept JSON request bodies. All require authentication if `IRIUM_RPC_TOKEN` is configured.

### `POST /rpc/createagreement`

Registers a new agreement with the node.

**Request body:** A signed agreement JSON object.

---

### `POST /rpc/computeagreementhash`

Computes the deterministic hash of an agreement without storing it.

**Request body:** An agreement JSON object.

**Response:** `{"agreement_hash": "<hex>"}`

---

### `POST /rpc/inspectagreement`

Returns the parsed fields of an agreement JSON for verification before funding.

**Request body:** An agreement JSON object.

---

### `POST /rpc/fundagreement`

Builds (and optionally broadcasts) the funding transaction for an agreement.

**Request body:** Agreement reference and funding parameters.

---

### `POST /rpc/agreementstatus`

Returns the current on-chain status of an agreement.

**Request body:** `{"agreement_hash": "<hex>"}` or an agreement JSON object.

---

### `POST /rpc/agreementtimeline`

Returns the full event timeline for an agreement.

**Request body:** Agreement reference.

---

### `POST /rpc/agreementaudit`

Returns a full audit record for an agreement including all on-chain events, proofs, and policy evaluations.

**Request body:** Agreement reference.

---

### `POST /rpc/agreementreleaseeligibility`

Checks whether the conditions for releasing funds from an agreement are currently met.

**Request body:** Agreement reference.

**Response:** `{"eligible": true|false, "reason": "..."}`

---

### `POST /rpc/agreementrefundeligibility`

Checks whether the timeout conditions for a refund are met.

**Request body:** Agreement reference.

**Response:** `{"eligible": true|false, "reason": "..."}`

---

### `POST /rpc/buildagreementrelease`

Builds the release transaction (unlocks funds to the recipient using the secret).

**Request body:** Agreement reference and secret preimage.

---

### `POST /rpc/buildagreementrefund`

Builds the refund transaction (returns funds to the funder after timeout).

**Request body:** Agreement reference.

---

### `POST /rpc/listproofs`

Returns all proofs submitted for a given agreement.

**Request body:** `{"agreement_hash": "<hex>"}`

---

### `POST /rpc/getproof`

Returns a single proof by its ID.

**Request body:** `{"proof_id": "<id>"}`

---

### `POST /rpc/storepolicy`

Stores a release policy for an agreement.

**Request body:** Policy JSON object.

---

### `POST /rpc/getpolicy`

Returns the stored policy for an agreement.

**Request body:** `{"agreement_hash": "<hex>"}`

---

### `POST /rpc/evaluatepolicy`

Evaluates the stored policy against currently submitted proofs.

**Request body:** Agreement reference.

**Response:** `{"satisfied": true|false, "details": [...]}`

---

### `POST /rpc/listagreementtxs`

Returns all on-chain transactions associated with an agreement.

**Request body:** Agreement reference.

---

### `POST /rpc/agreementmilestones`

Returns milestone status for a milestone-type agreement.

**Request body:** Agreement reference.

---

### `POST /rpc/verifyagreementlink`

Verifies the cryptographic link between a bundle, agreement, and on-chain funding transaction.

**Request body:** Bundle or agreement reference.

---

### `POST /rpc/submitproof`

Submits a signed proof to the node for an agreement.

**Request body:** A signed proof JSON object.

---

## Wallet Endpoints

Wallet endpoints require authentication if `IRIUM_RPC_TOKEN` is set. These endpoints mirror the `irium-wallet` CLI commands and are used by the wallet binary when communicating with the node.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/wallet/create` | POST | Create a new wallet |
| `/wallet/unlock` | POST | Unlock the wallet with passphrase |
| `/wallet/lock` | POST | Lock the wallet |
| `/wallet/addresses` | GET | List wallet addresses |
| `/wallet/receive` | GET | Get current receive address |
| `/wallet/new_address` | POST | Generate a new address |
| `/wallet/export_wif` | POST | Export private key in WIF format |
| `/wallet/import_wif` | POST | Import a WIF private key |
| `/wallet/export_seed` | POST | Export wallet seed |
| `/wallet/import_seed` | POST | Import a wallet seed |
| `/wallet/send` | POST | Build and broadcast a transaction |

---

## P2P Handshake (binary protocol on port 38291)

The HTTP API documented above is for clients. Node-to-node communication
uses the binary message framing defined in `src/protocol.rs`. The first
message exchanged by both sides of a TCP connection is a `HandshakePayload`
(JSON payload, message type `1`):

| Field | Type | Description |
|-------|------|-------------|
| `version` | u32 | Protocol version (currently 1) |
| `agent` | string | User-agent string |
| `height` | u64 | Local chain tip height |
| `timestamp` | i64 | Sender's clock (Unix seconds) |
| `port` | u16 | Listen port advertised by the sender |
| `checkpoint_height` | u64? | Optional best checkpoint height the sender knows |
| `checkpoint_hash` | string? | Optional checkpoint hash (hex) |
| `relay_address` | string? | Operator's IRM payout address for tx-relay attribution |
| `node_id` | string? | 32-byte persistent identity hash (hex) |
| `tip_hash` | string? | 32-byte hash of the sender's tip header (hex) |
| `capabilities` | string[]? | Capability strings (e.g. `"uptime-v1"`) |
| `marketplace_feed` | string? | Optional URL of the sender's offer feed |
| `external_endpoint` | string? | Self-advertised dialable endpoint in `"<ip>:<port>"` form. **New on `testing-codes-before-merging` (v1.9.19 scheduled).** Backwards compatible via `#[serde(default)]`. |

### `external_endpoint` semantics

When set and globally routable, the receiver SHOULD use this string to
record the sender's dialable address in PeerDirectory (the address later
gossiped to other peers via `GetPeers` / `Peers` messages). When unset,
or when the value fails routability validation, the receiver MUST fall
back to the TCP source IP and the `port` field.

Routability validation is identical on every node and rejects:

- Loopback (127.0.0.0/8, `::1`)
- RFC1918 private (10/8, 172.16/12, 192.168/16)
- RFC6598 CGNAT (100.64.0.0/10)
- Link-local (169.254/16, fe80::/10)
- Unspecified (0.0.0.0, `::`)
- Broadcast (255.255.255.255)
- Multicast
- RFC5737 documentation (192.0.2/24, 198.51.100/24, 203.0.113/24)
- IPv6 (the directory currently stores IPv4-only multiaddrs)
- Port 0

Nodes set their own `external_endpoint` via the
`IRIUM_EXTERNAL_ENDPOINT=<ip>:<port>` environment variable (or
`external_endpoint` in the node config JSON). Operators behind CGNAT
should pair this with port-forwarding at the carrier level (or accept
outbound-only operation if no inbound is reachable).

Reference implementation in `src/p2p.rs::dialable_multiaddr_from_advertised`.

---

## Notes

- All amounts throughout the API are in satoshis. To convert: `satoshis / 100_000_000 = IRM`.
- Block hashes and transaction IDs are hex strings, lowercase.
- Timestamps are Unix epoch seconds (integer or float).
- The default RPC port is configurable via the `IRIUM_RPC_PORT` environment variable. The node does not hardcode any port.
