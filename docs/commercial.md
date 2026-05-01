# Irium Commercial User Guide

## Overview

Irium supports merchant payments, contractor flows, escrow-on-delivery, and recurring
settlements -- all on-chain with no central server. Every endpoint is configurable;
no IP addresses or ports are hardcoded in any binary.

## Seller Dashboard

```bash
irium-wallet seller-status [--address <addr>] [--rpc <url>]
```

Shows: active offers, active agreements, completed trades, and reputation summary.

## Buyer Dashboard

```bash
irium-wallet buyer-status [--address <addr>] [--rpc <url>]
```

Shows: active agreements, proofs submitted, trade history.

## Invoice / Payment Link

Generate a shareable, self-contained invoice (no server required):

```bash
irium-wallet invoice-generate \
  --recipient <your-address> \
  --amount 10.5 \
  --reference "Order #1234" \
  --out invoice.json
```

The recipient shares `invoice.json`. The payer imports it:

```bash
irium-wallet invoice-import --file invoice.json
```

The import verifies the integrity checksum and prints payment instructions.
The payer then sends with `irium-wallet send`.

## Contractor Payment Flow (Milestone-Based)

### Step 1 -- Create a milestone agreement

```bash
irium-wallet agreement-template contractor_milestone \
  --payer-address <payer> \
  --payee-address <payee> \
  --out agreement.json
```

### Step 2 -- Build a proof policy for each milestone

```bash
irium-wallet policy-build-contractor \
  --policy-id pol-c1 \
  --agreement-hash <hash> \
  --attestor attestor-1:<pubkey> \
  --milestone ms-1:software_delivery \
  --milestone ms-2:service_completion \
  --rpc <url>
```

### Step 3 -- Submit proofs per milestone

```bash
irium-wallet agreement-proof-create \
  --agreement-hash <hash> \
  --proof-type software_delivery \
  --milestone-id ms-1 \
  --attested-by <id> \
  --address <addr> \
  --out ms1_proof.json

irium-wallet agreement-proof-submit --proof ms1_proof.json --rpc <url>
```

### Step 4 -- Evaluate and release milestone funds

```bash
irium-wallet agreement-policy-evaluate --agreement <hash> --rpc <url>
```

## Business Settlement Templates

```bash
# Escrow-on-delivery: funds locked until delivery proof received
irium-wallet agreement-template escrow_on_delivery

# Recurring payments: periodic settlement structure
irium-wallet agreement-template recurring_payment

# Partial release: incremental fund release over milestones
irium-wallet agreement-template partial_release
```

## API Endpoints

All ports are configurable via environment variables. No hardcoded defaults in
production paths:

| Variable            | Purpose                    |
|---------------------|----------------------------|
| `IRIUM_NODE_PORT`   | RPC and feed port          |
| `IRIUM_STATUS_PORT` | Lightweight status port    |
| `IRIUM_P2P_BIND`    | P2P bind address           |
| `IRIUM_RPC_TOKEN`   | API authentication token   |

Key endpoints (all require `Authorization: Bearer <token>` except `/status` and
`/offers/feed`):

| Method | Path                    | Description                        |
|--------|-------------------------|------------------------------------|
| GET    | `/status`               | Node height, peers, sync state     |
| GET    | `/offers/feed`          | Public offer feed                  |
| POST   | `/rpc/balance`          | Address balance                    |
| POST   | `/rpc/submitproof`      | Submit settlement proof            |
| POST   | `/rpc/listproofs`       | List proofs by agreement           |
| POST   | `/rpc/agreementstatus`  | Agreement lifecycle state          |
| POST   | `/rpc/evaluatepolicy`   | Check release eligibility          |
| POST   | `/rpc/buildsettlementtx`| Build settlement transaction       |

## SDK

The Python SDK stub in `sdk/irium_client.py` demonstrates the full cycle.
The `IriumClient` class accepts `base_url` and `token` at runtime -- never
hardcoded. Set `IRIUM_NODE_URL` and `IRIUM_RPC_TOKEN` environment variables
before running.

```bash
export IRIUM_NODE_URL=http://<your-node>:<port>
export IRIUM_RPC_TOKEN=<your-token>
python3 sdk/irium_client.py
```
