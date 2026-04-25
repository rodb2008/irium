# Phase 3 Commercial Templates

## Overview

The template system lets participants create valid agreements and policies from
a short list of named parameters, without constructing the full agreement object
by hand. Each template encodes a commercial pattern (OTC, deposit, milestone)
with sensible defaults and mandatory-field validation.

## Available templates

| Template ID          | Type                 | Description                                              |
|----------------------|----------------------|----------------------------------------------------------|
| `otc-basic`          | `otc_settlement`     | Peer-to-peer OTC trade with timeout refund               |
| `deposit-protection` | `refundable_deposit` | Deposit held in escrow; refunded on timeout              |
| `milestone-payment`  | `milestone_settlement` | Single-milestone staged payment released on attestation |

## CLI commands

### template-list

List all available templates.

```
irium-wallet template-list
irium-wallet template-list --json
```

### template-show

Show required and optional fields for a template.

```
irium-wallet template-show --template otc-basic
irium-wallet template-show --template deposit-protection --json
```

### agreement-create-from-template

Create an agreement from a named template. Auto-generates agreement ID,
creation time, secret hash, and document hash.

```
irium-wallet agreement-create-from-template \
  --template otc-basic \
  --seller <address> \
  --buyer  <address> \
  --amount <IRM value, e.g. 5.0> \
  --timeout <refund deadline block height>

irium-wallet agreement-create-from-template \
  --template deposit-protection \
  --payer  <address> \
  --payee  <address> \
  --amount <IRM value> \
  --timeout <block height> \
  [--purpose "Service deposit"] \
  [--attestor <address>]

irium-wallet agreement-create-from-template \
  --template milestone-payment \
  --payer  <address> \
  --payee  <address> \
  --amount <IRM value> \
  --timeout <block height> \
  [--milestone-title "Delivery complete"]
```

Common optional flags for all templates:

| Flag             | Description                              |
|------------------|------------------------------------------|
| `--agreement-id` | Override auto-generated agreement ID     |
| `--out <path>`   | Write agreement JSON to file             |
| `--json`         | Output result as JSON                    |

## Template details

### otc-basic

Required: `--seller`, `--buyer`, `--amount`, `--timeout`

Calls `build_otc_agreement` internally. Defaults `--asset` to `IRM` and
`--payment-method` to `off-chain`. The secret hash and document hash are
derived deterministically from the agreement ID and creation timestamp.

### deposit-protection

Required: `--payer`, `--payee`, `--amount`, `--timeout`

Calls `build_deposit_agreement`. Purpose defaults to `"Deposit protection"`.
If `--attestor` is supplied, the refund summary names the attestor address.

### milestone-payment

Required: `--payer`, `--payee`, `--amount`, `--timeout`

Calls `build_milestone_agreement` with a single auto-generated milestone.
The milestone title defaults to `"Milestone 1"`. The full amount maps to
that single milestone. Multi-milestone schedules require `agreement-create-milestone`
directly.

## How templates reduce complexity

Without templates, creating an OTC agreement requires manually supplying:
`--agreement-id`, `--creation-time`, `--buyer`, `--seller`, `--amount`,
`--asset-reference`, `--payment-reference`, `--refund-timeout`, `--secret-hash`,
`--document-hash`.

With `agreement-create-from-template --template otc-basic` only four fields
are required. All derived values are generated automatically, and the
resulting agreement is saved to the local store and printed to stdout.

## Relationship to existing commands

Templates are a thin orchestration layer over the existing low-level builders:

- `agreement-create-from-template --template otc-basic`
  wraps `agreement-create-otc` with auto-derived hashes and IDs.
- `agreement-create-from-template --template deposit-protection`
  wraps `agreement-create-deposit`.
- `agreement-create-from-template --template milestone-payment`
  wraps `agreement-create-milestone` (single milestone).

The underlying `build_otc_agreement`, `build_deposit_agreement`, and
`build_milestone_agreement` functions in `settlement.rs` are unchanged.
