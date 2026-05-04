# Irium Quickstart

This guide takes you from zero to a completed settlement. No blockchain experience needed.

**What you need:** an internet connection and a Linux or macOS computer. Windows users can follow the same steps inside WSL.

**What you will have at the end:** a working Irium wallet, your own IRM address, and a complete understanding of how to trade on the Irium marketplace.

---

## Step 1 — Install

Run this command in your terminal:

```bash
curl -fsSL https://raw.githubusercontent.com/iriumlabs/irium/main/install.sh | bash
```

This installs four programs into `/usr/local/bin`:

- `iriumd` — the full node (connects to the network and keeps it running)
- `irium-wallet` — the wallet and marketplace tool you will use for everything
- `irium-miner` — CPU miner (optional)
- `irium-miner-gpu` — GPU miner (optional)

**Confirm it worked:**

```bash
irium-wallet --version
```

If you see usage output, the install succeeded.

---

## Step 2 — Create a wallet

```bash
irium-wallet create-wallet --bip32
```

**What you will see:**

```
BIP32 wallet created
mnemonic: pass cycle ill pistol glad chapter normal nice shuffle inherit census beef wool solution page fossil rain theory prepare blood field frog hybrid print
derivation path: m/44'/1'/0'/0/0
IMPORTANT: write down your mnemonic -- it cannot be recovered
address: Pzt49NFBU9N15J4GuxNGitxdRtcHwyLGcj
wallet /home/yourname/.irium/wallet.json
```

**Write down your 24-word mnemonic right now.** This is the only backup of your wallet. Anyone who has these words can access your IRM. Store them offline — paper, not a screenshot.

The `--bip32` flag creates a standard hierarchical deterministic wallet. This is the recommended type because it is compatible with hardware wallets and future wallet apps.

Your wallet file is saved at `~/.irium/wallet.json`. Keep this file safe.

---

## Step 3 — Get your address

Generate a new IRM address to receive funds:

```bash
irium-wallet new-address
```

Then list all your addresses:

```bash
irium-wallet list-addresses
```

**What you will see:**

```
Pzt49NFBU9N15J4GuxNGitxdRtcHwyLGcj
Q2mADqCt3fHgvLZUMfdSn12JiKQ284qPEX
```

Your addresses start with `P` or `Q`. Both are valid IRM addresses — the first letter varies based on your specific key. Share any of these addresses to receive IRM.

---

## Step 4 — Check the network

Start the full node in a separate terminal window (leave it running):

```bash
iriumd
```

The node connects to the Irium network automatically using two official seed nodes. Once it starts syncing, check its status:

```bash
curl http://127.0.0.1:38300/status
```

You will see the current block height and peer count. The node is synced when `height` matches the network tip. Network status is also visible at [iriumlabs.org](https://www.iriumlabs.org).

Check your balance:

```bash
irium-wallet balance <YOUR_ADDRESS> --rpc http://127.0.0.1:38300
```

**What you will see:**

```
balance 0 IRM blocks mined 0
```

A new address always starts at zero. To receive IRM, share your address with the sender or buy from a marketplace offer.

---

## Step 5 — Browse the marketplace

Sync the offer feed to discover what is available:

```bash
irium-wallet offer-feed-sync --rpc http://127.0.0.1:38300
```

**What you will see:**

```
source   http://207.244.247.86:38300/offers/feed
total    13
imported 0
skipped  13 (already in local store)
```

Then list open offers:

```bash
irium-wallet offer-list --status open
```

**What you will see:**

```
Total: 2 offers

[1] aggr-test-001
    amount:   750000000 IRM
    seller:   Q9KxBRfrnb6v9Vb8vuHjwkZaxj3ZRhJWpg
    payment:  bank_transfer
    source:   remote:http://207.244.247.86:38300/offers/feed
    status:   open
    reputation: 11 agreements

[2] offer-1777297990
    amount:   1 IRM
    seller:   Q9KxBRfrnb6v9Vb8vuHjwkZaxj3ZRhJWpg
    payment:  rpc-check
    status:   open
    reputation: 11 agreements
```

Each offer shows the seller address, amount, payment method, and their reputation based on completed agreements.

---

## Step 6 — Create an offer (seller path)

If you have IRM to sell, create a sell offer:

```bash
irium-wallet offer-create \
  --seller <YOUR_ADDRESS> \
  --amount 0.5 \
  --payment-method "bank_transfer" \
  --timeout 21000 \
  --price-note "Software licence — delivered as download link"
```

Replace `<YOUR_ADDRESS>` with one of your addresses from Step 3. The `--timeout` is the block height deadline (current height plus blocks to wait; ~10 minutes per block).

**What you will see:**

```
offer_id         offer-1777888495
status           open
seller           Pzt49NFBU9N15J4GuxNGitxdRtcHwyLGcj
amount_irm       0.50000000 IRM
payment_method   bank_transfer
timeout_height   21000

saved_path      /home/yourname/.irium/offers/offer-offer-1777888495.json

next_step  export and share offer: irium-wallet offer-export --offer offer-1777888495 --out offer.json
```

Export and share the offer file with your buyer:

```bash
irium-wallet offer-export --offer offer-1777888495 --out offer.json
```

Send `offer.json` to the buyer through any channel (email, Telegram, etc.). The buyer imports it with:

```bash
irium-wallet offer-import --file offer.json
```

---

## Step 7 — Take an offer (buyer path)

When you have found an offer to accept, take it with your buyer address:

```bash
irium-wallet offer-take \
  --offer <OFFER_ID> \
  --buyer <YOUR_ADDRESS> \
  --rpc http://127.0.0.1:38300
```

**What you will see:**

```
=== Offer Taken ===

offer_id        offer-1777888495
agreement_id    offer-offer-1777888495-1777888517
agreement_hash  96dfc2a96630e6d6f9b49b404c69ad19bc4f8175055aeb33f098dd681be11f2e
seller          Pzt49NFBU9N15J4GuxNGitxdRtcHwyLGcj
buyer           Q2mADqCt3fHgvLZUMfdSn12JiKQ284qPEX
amount_irm      0.50000000 IRM

=== Next steps ===
1. Export this agreement for seller:
   irium-wallet agreement-pack --agreement 96dfc2a... --out agreement-pkg.json
2. Make external payment via bank_transfer
3. Seller confirms delivery and submits proof
```

The `agreement_hash` is your unique trade identifier. Keep it.

Package the agreement and share it with the seller:

```bash
irium-wallet agreement-pack \
  --agreement <AGREEMENT_HASH> \
  --out agreement-pkg.json \
  --rpc http://127.0.0.1:38300
```

The seller unpacks it on their side:

```bash
irium-wallet agreement-unpack --file agreement-pkg.json
```

The buyer then sends the agreed IRM amount to fund the escrow:

```bash
irium-wallet agreement-fund <AGREEMENT_HASH> --rpc http://127.0.0.1:38300
```

This locks the IRM on-chain. Neither party can access it until the agreement resolves.

---

## Step 8 — Submit proof and release

Once off-chain delivery or payment is complete, the seller submits an attestation:

```bash
irium-wallet otc-attest \
  --agreement <AGREEMENT_HASH> \
  --message "payment confirmed via bank transfer" \
  --address <SELLER_ADDRESS> \
  --rpc http://127.0.0.1:38300
```

**What you will see:**

```
proof_id prf-3c98200364ac8efc
agreement_hash 96dfc2a96630e6d6f9b49b404c69ad19bc4f8175055aeb33f098dd681be11f2e
accepted true
message proof accepted
tip_height 20489
status active
```

Check the full agreement status at any time:

```bash
irium-wallet otc-status \
  --agreement <AGREEMENT_HASH> \
  --rpc http://127.0.0.1:38300
```

Once the proof is buried 6 blocks deep (about 1 hour), the agreement becomes release-eligible. The seller releases the escrowed IRM to complete the settlement:

```bash
irium-wallet agreement-release <AGREEMENT_HASH> \
  --broadcast \
  --rpc http://127.0.0.1:38300
```

The IRM moves from escrow to the seller's address on-chain, automatically and without any third party.

---

## What's next

| Resource | What it covers |
|----------|---------------|
| [docs/WALLET-CLI.md](docs/WALLET-CLI.md) | Every wallet command with flags and examples |
| [docs/SETTLEMENT-DEV.md](docs/SETTLEMENT-DEV.md) | Agreement types, policy templates, proof formats |
| [docs/API.md](docs/API.md) | REST API for building apps on Irium |
| [docs/WEBSOCKET.md](docs/WEBSOCKET.md) | Real-time event streaming for UIs |
| [docs/WHITEPAPER.md](docs/WHITEPAPER.md) | Full protocol specification |
| [Telegram](https://t.me/iriumlabs) | Community — ask questions, find counterparties |
