# Marketplace Getting Started

This guide takes a first-time buyer from zero to a completed agreement in six commands.
No IP addresses, no feed URLs, and no manual peer configuration required.

## How discovery works

Every Irium node advertises its marketplace feed URL during the P2P handshake.
When you connect to any peer, their feed URL is saved automatically to
~/.irium/discovered_feeds.json. The wallet reads this file during sync so you
find offers from sellers across the network without knowing their addresses in advance.

## Buyer flow (six commands)

### Step 1 — Start your node and connect to the network

Run iriumd and leave it running. It connects to peers and exchanges feed URLs automatically.

### Step 2 — Sync offers from all discovered feeds

    irium-wallet marketplace-sync

This fetches offers from every feed the P2P layer has discovered plus any feeds you added
manually. Imported offers are stored locally.

### Step 3 — Browse available offers

    irium-wallet offer-list --source remote

Add --trusted-only to hide sellers with a HIGH risk signal.
Add --sort score to rank by reputation score.
Add --payment <method> to filter by payment method.

### Step 4 — Inspect an offer

    irium-wallet offer-inspect <offer-id>

Shows seller address, amount, payment instructions, and the seller reputation summary.

### Step 5 — Take the offer

    irium-wallet offer-take <offer-id>

This creates an agreement and locks funds in escrow. A policy is created automatically.

### Step 6 — Release funds after proof

    irium-wallet agreement-release <agreement-hash>

Once the seller submits proof and you verify it, release the escrowed funds.

## Seller flow

### Publish an offer

    irium-wallet offer-create --seller <your-address> --amount <irm> --payment-method <text> --timeout <height>

### Advertise your feed

Set the IRIUM_MARKETPLACE_FEED_URL environment variable to your node public feed URL
before starting iriumd. Peers will automatically learn your feed URL during handshake.

    IRIUM_MARKETPLACE_FEED_URL=http://<your-public-ip>:<p2p-port>/offers/feed iriumd

No central registration needed. The URL propagates to all peers you connect to, and from
there to their peers.

### Check open offers and active agreements

    irium-wallet offer-list
    irium-wallet agreement-list

### Submit proof when work is complete

    irium-wallet proof-submit --agreement <hash> --type <proof-type> --payload <data>

## P2P-discovered vs manually configured feeds

Manually configured feeds live in ~/.irium/feeds.json and are added with:

    irium-wallet feed-add <url>

P2P-discovered feeds live in ~/.irium/discovered_feeds.json and are received
automatically from peers during handshake.

Both sources are used by offer-feed-sync and marketplace-sync. You can see what
has been discovered with:

    irium-wallet offer-feed-discover

## Trust and reputation

Every seller has a reputation score derived from on-chain agreement outcomes.
The score feeds into --sort score ordering and the risk signal shown in offer-list.

- low risk: no defaults on record
- moderate risk: up to 20% default rate
- high risk: more than 20% default rate

Use --trusted-only to hide HIGH risk sellers from offer-list output.

View a seller full reputation data:

    irium-wallet reputation-show <seller-pubkey-or-address>
