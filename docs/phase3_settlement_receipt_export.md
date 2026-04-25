# Settlement Receipt Export

## Overview

Settlement receipts are human-readable (text or HTML) exports of the derived
agreement statement. They are informational only; the canonical source of
truth for any agreement is the agreement hash plus on-chain / RPC state.

## Commands

### agreement-receipt

Print or export a settlement receipt to stdout or a file.

    irium-wallet agreement-receipt <agreement.json|bundle.json|id|hash>         [--format text|html|json]         [--out <file>]         [--rpc <url>]         [--agreement-signature <sig.json>]         [--bundle-signature <sig.json>]

- Default format: text
- --format text: formatted plaintext receipt with ASCII section headers
- --format html: standalone HTML page with table layout
- --format json: raw AgreementStatement JSON (same as agreement-statement --json)
- --out: write output to file (text/json prints to stdout too; html prints path only)

### agreement-statement-export (extended)

    irium-wallet agreement-statement-export <agreement.json|...> --out <file>         [--format json|text|html]         [--agreement-signature <sig.json>]         [--bundle-signature <sig.json>]

- Default format: json (unchanged from prior versions)
- --format text: exports rendered receipt text
- --format html: exports standalone HTML receipt
- --json flag sets format to json (backward compatibility)

## Receipt fields

Both text and HTML receipts include:

- Agreement ID and hash (canonical identifier)
- Template type
- Payer and payee addresses
- Commercial terms: total amount, milestones, release path, refund path, deadlines
- Observed activity: funding / release / refund observed flags, linked txids
- Settlement outcome: derived status label, funded / released / refunded amounts
- Authenticity summary (if signatures were supplied)
- Generated-at timestamp
- Canonical notice (informational trust boundary statement)

## Verification note

Receipts are derived from on-chain observations plus supplied agreement data.
They do not constitute settlement enforcement. To verify a receipt:

1. Confirm the agreement_hash matches the canonical agreement JSON
2. Re-run agreement-statement or agreement-receipt against a live node
3. Check linked txids on-chain

## Examples

    # Print text receipt to terminal
    irium-wallet agreement-receipt agreement.json

    # Save HTML receipt to file
    irium-wallet agreement-receipt agreement.json --format html --out receipt.html

    # Export statement JSON (backward compatible)
    irium-wallet agreement-statement-export agreement.json --out statement.json

    # Export text receipt via statement-export
    irium-wallet agreement-statement-export agreement.json --format text --out receipt.txt
