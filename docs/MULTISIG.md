# Multisig on Irium

Irium supports M-of-N multi-signature addresses and agreements using the MPSOv1 output type. Multisig lets multiple parties jointly control funds, requiring M of N designated keys to release or refund a locked output.

## When to use multisig

- **Joint custody**: two people share control of a wallet; both must agree to spend
- **2-of-2 OTC escrow**: buyer and seller both lock funds; release requires both co-signatures
- **2-of-3 with arbitrator**: any 2 of 3 participants (buyer, seller, arbitrator) can release funds if one party is unresponsive

## Creating a multisig address

```bash
# 2-of-2: both keys required to spend
irium-wallet multisig-create --m 2 --pubkeys <pubkey1_hex> <pubkey2_hex>

# 2-of-3: any 2 of 3 keys required
irium-wallet multisig-create --m 2 --pubkeys <pk1> <pk2> <pk3>
```

Pubkeys must be 33-byte compressed secp256k1 public keys in hex. Get a wallet's pubkey with:

```bash
irium-wallet new-address --show-pubkey
```

The address encodes M, N, and all pubkeys in Base58Check with version byte `0x28`. Multisig addresses begin with a different character than regular Q-addresses.

## Sending funds to a multisig address

Use `multisig-send` from your regular wallet address:

```bash
irium-wallet multisig-send 1.0 \
  --from QYourRegularAddress \
  --to <multisig-addr> \
  --timeout-height 21000
```

`--timeout-height` is the block height after which the refund path becomes active. Set it to `current_height + expected_blocks`. Use `irium-wallet balance` to see the current height.

Optional:
- `--agreement-hash <hex>` — links the multisig output to a settlement agreement hash (32 bytes in hex)
- `--fee <amount>` — override the fee
- `--rpc <url>` — specify a different node

## Spending from a multisig address (claim path)

Spending before the timeout requires M valid signatures from the claim pubkeys.

### Step 1: Build the unsigned spend transaction

Get the txid and vout of the MPSO output you want to spend (from `irium-wallet list-unspent <multisig-addr>`), then get the MPSO script_pubkey from the transaction output.

```bash
irium-wallet multisig-spend-build \
  --txid <funding_txid> \
  --vout 0 \
  --value <output_value_satoshis> \
  --to <destination_p2pkh_addr> \
  --script-pubkey <mpso_script_hex> \
  --fee 500 > partial.json
```

This creates a `partial.json` file with the unsigned tx and input metadata.

### Step 2: Each signer signs the partial tx

Each co-signer runs:

```bash
irium-wallet multisig-sign partial.json --wallet <their_address> > partial_signed_by_alice.json
```

If there are two signers, each produces their own signed partial:

```bash
# Alice signs
irium-wallet multisig-sign partial.json --wallet <alice_addr> > alice.json

# Bob signs (on his own machine)
irium-wallet multisig-sign partial.json --wallet <bob_addr> > bob.json
```

### Step 3: Combine the partial signatures

```bash
irium-wallet multisig-combine alice.json bob.json
```

Outputs the fully-signed transaction hex. If M signatures are present, the output is ready to broadcast.

### Step 4: Broadcast

```bash
irium-wallet multisig-broadcast <txhex>
```

Or pipe from combine:

```bash
irium-wallet multisig-combine alice.json bob.json | irium-wallet multisig-broadcast -
```

The txid is printed on success.

## Refund path (after timeout)

After `timeout_height` blocks have passed, the refund path opens. The same M-of-N keys can refund the output to any destination using `--path refund`:

```bash
# Build refund spend
irium-wallet multisig-spend-build \
  --txid <txid> --vout 0 --value <sats> \
  --to <refund_addr> \
  --script-pubkey <mpso_script_hex> \
  --path refund > refund_partial.json

# Each signer signs the refund
irium-wallet multisig-sign refund_partial.json --wallet <alice_addr> > refund_alice.json
irium-wallet multisig-sign refund_partial.json --wallet <bob_addr> > refund_bob.json

# Combine and broadcast
irium-wallet multisig-combine refund_alice.json refund_bob.json | irium-wallet multisig-broadcast -
```

The refund tx is only valid if the current block height is >= `timeout_height`.

## 2-of-2 OTC escrow flow

This is the recommended pattern for trustless OTC trades.

### Setup

```bash
# Both parties share their pubkeys out-of-band
ALICE_PK=<alice_compressed_pubkey_hex>
BOB_PK=<bob_compressed_pubkey_hex>

# Either party creates the multisig address
irium-wallet multisig-create --m 2 --pubkeys $ALICE_PK $BOB_PK
# → Multisig address (2-of-2): <multisig-addr>
```

### Funding

Alice (the buyer) funds the escrow:

```bash
irium-wallet multisig-send 0.5 \
  --from <alice_regular_addr> \
  --to <multisig-addr> \
  --timeout-height <agreed_deadline>
```

### Release (happy path)

When the trade completes, both parties co-sign a release to Bob:

```bash
# Build the release tx
irium-wallet multisig-spend-build \
  --txid <funding_txid> --vout 0 --value <amount_satoshis> \
  --to <bob_addr> --script-pubkey <mpso_script_hex> > release.json

# Alice signs
irium-wallet multisig-sign release.json --wallet <alice_addr> > release_alice.json
# Bob signs
irium-wallet multisig-sign release.json --wallet <bob_addr> > release_bob.json

# Combine and broadcast
irium-wallet multisig-combine release_alice.json release_bob.json | irium-wallet multisig-broadcast -
```

### Timeout refund (dispute path)

If the trade fails and the timeout passes, both sign a refund back to Alice:

```bash
irium-wallet multisig-spend-build \
  --txid <funding_txid> --vout 0 --value <amount_satoshis> \
  --to <alice_addr> --script-pubkey <mpso_script_hex> --path refund > refund.json

irium-wallet multisig-sign refund.json --wallet <alice_addr> > refund_alice.json
irium-wallet multisig-sign refund.json --wallet <bob_addr> > refund_bob.json
irium-wallet multisig-combine refund_alice.json refund_bob.json | irium-wallet multisig-broadcast -
```

## Getting the MPSO script_pubkey

After sending to a multisig address, you need the `script_pubkey` hex of the output for subsequent spend operations. Retrieve it from the node:

```bash
# List UTXOs for the multisig address
irium-wallet list-unspent <multisig-addr> --rpc http://localhost:38300
# The script_pubkey field in the response is what you need
```

Or reconstruct it from the address (the address encodes all parameters):

```bash
# The multisig-spend-build command does this automatically when given --script-pubkey
# You can also derive it using the multisig-create output's script components
```

## Security considerations

- **Coordinate off-chain**: the partial JSON files (`partial.json`, `alice.json`, `bob.json`) contain the unsigned transaction. Anyone who intercepts them cannot spend funds (they lack the private keys), but keep them confidential.
- **Verify the tx before signing**: check the `tx_hex` in the partial JSON independently using a hex decoder to confirm the destination address and amount before signing.
- **Set a realistic timeout**: if the timeout is too short, the refund path opens before both parties finish; if too long, funds are locked for too long in a dispute.
- **One timeout for all paths**: both the claim deadline and the refund opening share the same `timeout_height`. Before this height, only the claim path is valid. At or after this height, only the refund path is valid.
- **Nonce safety**: each `multisig-sign` call produces a fresh signature. Never reuse a partial JSON from a different transaction when signing.
