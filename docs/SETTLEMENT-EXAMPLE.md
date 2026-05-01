# Irium Settlement Walkthrough: Getting Paid Safely as a Freelancer

## The Scenario

Alice is a freelance developer. Bob wants to hire her to build a website for 50 IRM.

They have never worked together before. Alice does not know if Bob will pay after she delivers. Bob does not know if Alice will deliver after he pays. Neither wants to go first.

Irium settlement solves this with on-chain escrow. Bob locks 50 IRM into a contract before Alice does any work. Alice knows the money is there and cannot be moved by Bob until the agreement resolves. If Alice delivers and the proof is accepted, she gets paid. If the deadline passes without delivery, Bob gets his money back automatically.

No bank. No lawyer. No trust required.

---

## Step 1 — Alice and Bob agree on terms off-chain

Before anything goes on-chain, they agree on:

- The amount: 50 IRM
- What counts as delivery: a working website at an agreed URL
- A deadline (expressed as a block height, approximately 1 block per 10 minutes)
- A short description of the work in a document they both sign

They write their agreement into a text file (`terms.txt`). This file will be hashed and committed on-chain so neither party can claim the terms were different later.

---

## Step 2 — Generate a secret and create the agreement

Bob generates a one-time secret. This is a random number that will unlock the funds when it is revealed:

```bash
# Generate a random 32-byte secret (Bob keeps this private until satisfied)
SECRET=$(openssl rand -hex 32)

# Compute the hash of the secret (this goes into the agreement — not the secret itself)
SECRET_HASH=$(printf '%s' "$SECRET" | xxd -r -p | sha256sum | awk '{print $1}')

echo "Secret hash: $SECRET_HASH"
```

Bob hashes the terms document to bind it to the on-chain record:

```bash
DOCUMENT_HASH=$(sha256sum terms.txt | awk '{print $1}')
```

Bob creates the agreement JSON:

```bash
irium-wallet agreement-create-simple-settlement \
  --agreement-id website-project-001 \
  --creation-time $(date +%s) \
  --party-a "id=alice,name=Alice,role=freelancer" \
  --party-b "id=bob,name=Bob,role=client" \
  --amount 50 \
  --secret-hash $SECRET_HASH \
  --refund-timeout 21500 \
  --document-hash $DOCUMENT_HASH \
  --release-summary "Alice delivers completed website by block 21500" \
  --refund-summary "Bob reclaims if website not delivered by block 21500" \
  --out website-project-001.json
```

The `--refund-timeout 21500` means: if the secret has not been revealed by block 21500 (roughly 1 week from now if the current height is ~20300), Bob can reclaim his 50 IRM. No manual action from a third party. The chain enforces it.

---

## Step 3 — Share the agreement with Alice

Bob sends Alice the agreement JSON file. Alice inspects it:

```bash
irium-wallet agreement-inspect website-project-001.json
```

Alice checks:
- Her name and role are correct
- The amount is 50 IRM
- The release summary matches what they agreed
- The refund timeout gives her enough time to complete the work
- The document hash matches the terms they discussed

If Alice is satisfied, she confirms. If she wants changes, she tells Bob before anything is funded.

---

## Step 4 — Bob funds the escrow (money goes on-chain)

Once both parties agree on the terms, Bob locks 50 IRM into the escrow contract:

```bash
irium-wallet agreement-fund website-project-001.json \
  --broadcast \
  --rpc http://localhost:38300
```

Output:
```
Funding transaction broadcast: txid=a3f1b2c3d4e5...
Agreement: website-project-001
Escrow amount: 50.000 IRM
Refund timeout: block 21500
Status: FUNDED — awaiting delivery
```

Alice can verify the funding independently:

```bash
irium-wallet agreement-timeline website-project-001.json \
  --rpc http://localhost:38300
```

The 50 IRM is now locked. Bob cannot take it back before block 21500. Alice knows with certainty that the money exists and is waiting for her.

---

## Step 5 — Alice does the work

Alice builds the website. When it is ready, she tells Bob. Bob reviews the delivery.

---

## Step 6 — Bob accepts and reveals the secret

If Bob is satisfied with the work, he gives Alice the secret:

```bash
# Bob sends Alice this value (privately, e.g. via Telegram)
echo "My secret: $SECRET"
```

Alice uses the secret to check release eligibility and collect the funds:

```bash
irium-wallet agreement-release-eligibility website-project-001.json \
  --secret $SECRET \
  --destination <ALICE_ADDRESS> \
  --rpc http://localhost:38300
```

If eligible, Alice broadcasts the release transaction:

```bash
# The release-eligibility command shows the signed transaction
# Alice broadcasts it directly
```

The 50 IRM moves from the escrow output to Alice's address. The agreement is settled.

---

## What if Bob does not accept or disappears?

### Scenario A — Bob withholds the secret despite good delivery

Alice submitted the work. Bob will not respond. This is a dispute.

In this case, Alice can submit a proof to the Irium network. A designated attestor reviews the evidence (screenshots, code repository, delivery confirmation) and if the proof meets the policy, the attestor publishes the release secret.

```bash
# Alice submits proof to the RPC
curl -X POST http://localhost:38300/rpc/submitproof \
  -H "Content-Type: application/json" \
  -d @proof-of-delivery.json
```

The proof mechanism is designed for objective, verifiable evidence. For a website: a working URL screenshot, a git commit hash, a signed delivery statement. The proof policy is defined in the agreement upfront so there is no ambiguity.

### Scenario B — Bob never responds and the timeout expires

Block 21500 passes. Bob has neither accepted nor disputed. The escrow automatically becomes refundable to Bob:

```bash
irium-wallet agreement-refund-eligibility website-project-001.json \
  --destination <BOB_ADDRESS> \
  --rpc http://localhost:38300
```

Bob gets his 50 IRM back. Alice gets nothing but has wasted no time on funded work — she saw the escrow and could make an informed decision about whether to start.

---

## What if Alice does not deliver?

Alice took the job and the escrow was funded. She delivers nothing.

Block 21500 passes. Bob checks refund eligibility and reclaims his 50 IRM automatically. No court required. No arbitration. The chain enforces the timeout.

```bash
irium-wallet agreement-refund-eligibility website-project-001.json \
  --destination <BOB_ADDRESS> \
  --rpc http://localhost:38300
```

---

## Summary

| Situation | Outcome |
|-----------|---------|
| Alice delivers, Bob accepts | Alice receives 50 IRM |
| Alice delivers, Bob withholds secret | Alice can submit proof; attestor can release funds |
| Alice does not deliver | Bob reclaims 50 IRM after timeout, automatically |
| Bob never responds after timeout | Bob reclaims 50 IRM after timeout, automatically |
| Bob tries to take funds back early | Impossible — HTLC contract prevents it |

The escrow is enforced by the chain itself. There is no Irium Labs server that can be hacked, bribed, or shut down to change the outcome. The rules are in the transaction and the transaction is on every node in the network.

---

## Costs

- One funding transaction: ~0.001 IRM in fees (varies with network congestion)
- One release or refund transaction: ~0.001 IRM in fees
- Total cost for the whole agreement: approximately 0.002 IRM

At current network activity, this is negligible.

---

## Getting Started

```bash
# Install the wallet
git clone https://github.com/iriumlabs/irium.git
cd irium
cargo build --release --bin irium-wallet

# Set up a wallet
./target/release/irium-wallet init
./target/release/irium-wallet new-address
```

Full wallet command reference: [docs/WALLET-CLI.md](WALLET-CLI.md)

API reference for integration: [docs/API.md](API.md)

Settlement integration guide for developers: [docs/SETTLEMENT-DEV.md](SETTLEMENT-DEV.md)

Questions: [t.me/iriumlabs](https://t.me/iriumlabs)
