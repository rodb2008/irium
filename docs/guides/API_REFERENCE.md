# Irium API Reference

## Node RPC (default 38300)
Base URL: `https://127.0.0.1:38300` when TLS is enabled via `IRIUM_TLS_CERT` + `IRIUM_TLS_KEY`.

### GET /status
Node height and peer summary.

### GET /peers
Connected peer list.

### GET /metrics
Prometheus-style metrics.

### GET /rpc/balance?address=<base58>
Spendable balance + mined block count for the address.

### GET /rpc/utxos?address=<base58>
List UTXOs for an address (includes `is_coinbase` + `height`).

### GET /rpc/utxo?txid=<hex>&index=<n>
Look up a specific UTXO.

### GET /rpc/getblocktemplate
Mining template (requires RPC auth).

### GET /rpc/block?height=<n>
Block JSON for a height.

### POST /rpc/submit_tx
Submit a raw transaction (requires RPC auth).

### POST /rpc/submit_block
Submit a raw block (requires RPC auth).

## Explorer API (default 38310)
### GET /status
### GET /peers
### GET /metrics
### GET /block/:height
### GET /utxo?txid=<hex>&index=<n>

## Wallet API (default 38320)
### GET /status
### GET /balance?address=<base58>
### GET /utxos?address=<base58>
### POST /submit_tx

Set `IRIUM_WALLET_API_TOKEN` to protect `submit_tx`.
Optional TLS: `IRIUM_WALLET_API_TLS_CERT` + `IRIUM_WALLET_API_TLS_KEY`.
