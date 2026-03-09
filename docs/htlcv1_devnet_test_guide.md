# HTLCv1 Devnet/Testnet Guide

Safety:
- HTLCv1 is off by default.
- Keep mainnet activation disabled.
- Use activation height only on local/devnet/testnet.

Start iriumd with activation:

export IRIUM_HTLCV1_ACTIVATION_HEIGHT=5
export IRIUM_NODE_HOST=127.0.0.1
export IRIUM_NODE_PORT=38300
./target/release/iriumd

Suggested test heights:
- 5 for fast local checks
- 100 for explicit pre-activation checks

Create secret/hash:

SECRET_HEX=<32-byte-hex>
SECRET_HASH_HEX=<sha256(secret bytes)>

Example RPC flow:
1) createhtlc
POST /rpc/createhtlc
{
  "amount": "1.00000000",
  "recipient_address": "<recipient>",
  "refund_address": "<refund>",
  "secret_hash_hex": "<64-hex>",
  "timeout_height": 200,
  "fee_per_byte": 1,
  "broadcast": true
}

Expected:
- before activation: HTTP 400
- after activation: returns txid/raw_tx_hex/htlc_vout

2) Mine/confirm funding tx
3) inspecthtlc
GET /rpc/inspecthtlc?txid=<funding_txid>&vout=0
Expected funded: exists=true, unspent=true

4) claimhtlc
POST /rpc/claimhtlc
{
  "funding_txid": "<funding_txid>",
  "vout": 0,
  "destination_address": "<recipient>",
  "secret_hex": "<secret>",
  "fee_per_byte": 1,
  "broadcast": true
}

Expected:
- valid secret: accepted path
- wrong secret: HTTP 400

5) refundhtlc
POST /rpc/refundhtlc
{
  "funding_txid": "<funding_txid>",
  "vout": 0,
  "destination_address": "<refund>",
  "fee_per_byte": 1,
  "broadcast": true
}

Expected:
- before timeout: HTTP 400
- at/after timeout: accepted path

Mainnet warning:
Do not set IRIUM_HTLCV1_ACTIVATION_HEIGHT on production mainnet nodes until coordinated rollout is approved.
