# Settlement Integration Guide

This guide covers the Irium settlement system for developers integrating agreement-based escrow into their own applications.

---

## Overview

The Irium settlement system provides HTLC-based escrow. Funds are locked on-chain in a Hash Time-Locked Contract controlled by a 2-of-2 condition: either the recipient presents the preimage of a known secret hash (release), or the funder reclaims after a block height timeout (refund).

On top of the base HTLC, Irium adds a structured agreement layer with proof submission and policy evaluation, so that release conditions can be verified programmatically before funds are moved.

---

## Agreement Types

| Type | Use case |
|------|----------|
| `simple-settlement` | Two parties, one amount, one secret hash. General purpose. |
| `otc` | Buyer/seller trade. Includes asset reference and payment reference. |
| `deposit` | Payer/payee. Includes purpose reference and refund summary. |
| `milestone` | Multiple milestones, each with a separate amount and timeout height. |

---

## Agreement Lifecycle

```
1. Create agreement JSON
        |
        v
2. Compute agreement hash (deterministic, based on all fields)
        |
        v
3. Fund the agreement (on-chain HTLC transaction)
        |
        v
4. Submit proof (attester signs off that condition is met)
        |
        v
5a. Release (recipient unlocks with secret preimage)
   OR
5b. Refund (funder reclaims after refund-timeout height)
```

---

## Step-by-Step: Full OTC Agreement Lifecycle

### Step 1: Create the agreement

```bash
irium-wallet agreement-create-otc \
  --agreement-id otc-trade-001 \
  --creation-time $(date +%s) \
  --buyer addr=QBuyerAddress... \
  --seller addr=QSellerAddress... \
  --amount 1.0 \
  --asset-reference "50 USDT" \
  --payment-reference "SEPA transfer ref #12345" \
  --secret-hash a3f1b2c3d4e5f6071234567890abcdef1234567890abcdef1234567890abcdef12 \
  --refund-timeout 20500 \
  --document-hash fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210fe \
  --out otc-trade-001.json
```

The `--secret-hash` is the SHA256 of the secret preimage. The secret itself is kept private by the party who will trigger release.

The `--document-hash` is the SHA256 of any off-chain agreement document (terms, receipts, etc.). It binds the on-chain record to the off-chain agreement.

The output `otc-trade-001.json` contains the full agreement structure.

---

### Step 2: Compute the agreement hash

```bash
irium-wallet agreement-hash otc-trade-001.json
```

Output: a 32-byte hex hash. This hash uniquely identifies the agreement and is used in all subsequent API calls.

You can also compute it via the RPC API:

```bash
curl -X POST http://localhost:38300/rpc/computeagreementhash \
  -H "Content-Type: application/json" \
  -d @otc-trade-001.json
```

Response:
```json
{"agreement_hash": "a1b2c3d4...32bytehex..."}
```

---

### Step 3: Fund the agreement

The seller funds the escrow by broadcasting the HTLC funding transaction.

```bash
irium-wallet agreement-fund otc-trade-001.json \
  --broadcast \
  --rpc http://localhost:38300
```

This builds and broadcasts a transaction that locks `amount` IRM into an HTLC output. The output can only be spent by:
- The recipient presenting the secret preimage before `refund-timeout`, or
- The funder reclaiming after `refund-timeout`.

To build without broadcasting (for inspection or offline signing):
```bash
irium-wallet agreement-fund otc-trade-001.json --rpc http://localhost:38300
```

---

### Step 4: Check agreement status

```bash
irium-wallet agreement-status otc-trade-001.json --rpc http://localhost:38300
```

Via RPC:
```bash
curl -X POST http://localhost:38300/rpc/agreementstatus \
  -H "Content-Type: application/json" \
  -d '{"agreement_hash": "a1b2c3d4...32bytehex..."}'
```

Status values include: `pending`, `funded`, `released`, `refunded`.

---

### Step 5: Submit a proof

Once the condition is met (e.g. payment received, goods delivered), an attester submits a proof:

```bash
irium-wallet agreement-proof-create \
  --agreement-hash a1b2c3d4...32bytehex... \
  --proof-type delivery_confirmed \
  --attested-by attestor-id \
  --address QAttestorAddress... \
  --evidence-summary "Payment of 50 USDT confirmed. Bank ref #12345." \
  --out proof.json

irium-wallet agreement-proof-submit --proof proof.json --rpc http://localhost:38300
```

Via RPC directly:
```bash
curl -X POST http://localhost:38300/rpc/submitproof \
  -H "Content-Type: application/json" \
  -d @proof.json
```

---

### Step 6a: Release funds to recipient

Once proof is submitted and the policy is satisfied:

```bash
# Check release eligibility first
irium-wallet agreement-release-eligibility otc-trade-001.json --rpc http://localhost:38300

# Release
irium-wallet agreement-release otc-trade-001.json \
  --secret a3f1b2c3d4e5f6071234567890abcdef1234567890abcdef1234567890abcdef12 \
  --broadcast \
  --rpc http://localhost:38300
```

The `--secret` is the preimage of the `secret-hash` set at agreement creation. The transaction unlocks the HTLC and sends funds to the recipient.

---

### Step 6b: Refund after timeout

If the counterparty does not perform and the refund timeout block height is reached:

```bash
# Check refund eligibility
irium-wallet agreement-refund-eligibility otc-trade-001.json --rpc http://localhost:38300

# Refund
irium-wallet agreement-refund otc-trade-001.json \
  --broadcast \
  --rpc http://localhost:38300
```

---

## Real JSON Examples at Each Step

### Agreement JSON structure (output of `agreement-create-otc`)

```json
{
  "agreement_id": "otc-trade-001",
  "agreement_type": "otc",
  "creation_time": 1777624133,
  "parties": {
    "buyer": {"addr": "QBuyerAddress..."},
    "seller": {"addr": "QSellerAddress..."}
  },
  "amount_satoshis": 100000000,
  "asset_reference": "50 USDT",
  "payment_reference": "SEPA transfer ref #12345",
  "secret_hash": "a3f1b2c3d4e5f6071234567890abcdef1234567890abcdef1234567890abcdef12",
  "refund_timeout": 20500,
  "document_hash": "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210fe",
  "signatures": {}
}
```

### Agreement hash response

```json
{"agreement_hash": "a1b2c3d4e5f6078901234567890abcdef1234567890abcdef1234567890abcdef1"}
```

### Agreement status response

```json
{
  "agreement_hash": "a1b2c3d4...",
  "status": "funded",
  "funding_txid": "cb7d25dc615df7e64726c171b18f401c916133f9335ed5153e3e14312b001b12",
  "funding_height": 20300,
  "refund_timeout": 20500,
  "amount_satoshis": 100000000
}
```

### Proof JSON structure

```json
{
  "proof_id": "proof-otc-trade-001-001",
  "agreement_hash": "a1b2c3d4...",
  "proof_type": "delivery_confirmed",
  "attested_by": "attestor-id",
  "attester_address": "QAttestorAddress...",
  "evidence_summary": "Payment of 50 USDT confirmed. Bank ref #12345.",
  "evidence_hash": null,
  "created_at": 1777630000,
  "signature": "..."
}
```

### Release eligibility response

```json
{
  "eligible": true,
  "reason": "Policy satisfied: delivery_confirmed proof submitted by required attestor."
}
```

### Refund eligibility response (before timeout)

```json
{
  "eligible": false,
  "reason": "Refund timeout not reached. Current height: 20400, timeout: 20500."
}
```

---

## Using Policies

Policies define conditions that must be met before release is permitted. A policy links an agreement to required proof types and attestors.

```bash
# Build an OTC policy
irium-wallet policy-build-otc \
  --policy-id policy-001 \
  --agreement-hash a1b2c3d4... \
  --attestor attestor-id:QAttestorAddress... \
  --release-proof-type delivery_confirmed

# Store the policy on the node
irium-wallet agreement-policy-set --policy policy.json --rpc http://localhost:38300

# Evaluate whether proofs satisfy the policy
irium-wallet agreement-policy-evaluate --agreement a1b2c3d4... --rpc http://localhost:38300
```

---

## RPC Endpoints Reference

All settlement endpoints accept JSON bodies and return JSON responses.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/rpc/createagreement` | POST | Register agreement on node |
| `/rpc/computeagreementhash` | POST | Compute agreement hash |
| `/rpc/inspectagreement` | POST | Parse and inspect agreement fields |
| `/rpc/fundagreement` | POST | Build funding transaction |
| `/rpc/agreementstatus` | POST | Current on-chain status |
| `/rpc/agreementtimeline` | POST | Full event timeline |
| `/rpc/agreementaudit` | POST | Full audit record |
| `/rpc/agreementreleaseeligibility` | POST | Check release conditions |
| `/rpc/agreementrefundeligibility` | POST | Check refund conditions |
| `/rpc/buildagreementrelease` | POST | Build release transaction |
| `/rpc/buildagreementrefund` | POST | Build refund transaction |
| `/rpc/listproofs` | POST | List proofs for an agreement |
| `/rpc/getproof` | POST | Get single proof by ID |
| `/rpc/submitproof` | POST | Submit a proof |
| `/rpc/storepolicy` | POST | Store a release policy |
| `/rpc/getpolicy` | POST | Get stored policy |
| `/rpc/evaluatepolicy` | POST | Evaluate policy against proofs |
| `/rpc/listagreementtxs` | POST | List on-chain transactions for agreement |
| `/rpc/agreementmilestones` | POST | Milestone status (milestone type only) |
| `/rpc/verifyagreementlink` | POST | Verify bundle-agreement-tx link |

---

## Error Handling

| Scenario | Expected behaviour |
|----------|--------------------|
| Funding tx not yet confirmed | `agreementstatus` returns `pending` |
| Release attempted before policy satisfied | `agreementreleaseeligibility` returns `eligible: false` with reason |
| Refund attempted before timeout | `agreementrefundeligibility` returns `eligible: false` with reason |
| Proof submitted after release | Proof accepted but has no effect on eligibility |
| Wrong secret provided | Release transaction will fail validation at the node |
| Agreement hash mismatch | Node rejects the request with a 400 error |

---

## Milestone Agreements

Milestone agreements release funds in stages. Each milestone has:
- An amount
- A proof type requirement
- A timeout height

Creating a milestone agreement:
```bash
irium-wallet agreement-create-milestone \
  --agreement-id milestone-001 \
  --creation-time $(date +%s) \
  --party-a addr=QClientAddress... \
  --party-b addr=QContractorAddress... \
  --secret-hash a3f1...32bytehex... \
  --refund-timeout 21000 \
  --document-hash fedcba...32bytehex... \
  --out milestone-001.json
```

Check milestone status:
```bash
irium-wallet agreement-milestones milestone-001.json --rpc http://localhost:38300
```


---

## Attestor Bonding

Attestors can register an on-chain economic bond to signal accountability. Agreements
that reference unbonded attestors are accepted but produce a warning to counterparties.

### Registering a Bond

```bash
irium-wallet attestor-register --bond 10 --from QMyAttestorAddress...
```

This publishes a transaction with an OP_RETURN output:
```
bond1:<pkh_hex_40>:<atoms_decimal>
```

The bond amount is declared publicly on-chain. The IRM remains in the attestor's
wallet but is publicly committed. Any counterparty can verify the bond is active.

### Viewing Bond Status

```bash
# All bonds on this node
irium-wallet attestor-bond-status

# Bond for a specific address
irium-wallet attestor-bond-status --address QMyAttestorAddress...

# JSON output
irium-wallet attestor-bond-status --json
```

Bonds are also shown inline when running `irium-wallet attestor-list`:
```
alice-attestor (02abc...) — Alice [bond: 10 IRM active]
bob-attestor (03def...) — Bob [bond: none — unbonded]
```

When building a policy (`policy-build-otc`, `policy-build-contractor`,
`policy-build-preorder`), a warning is printed to stderr if any specified
attestor has no active bond:
```
warn  attestor 02def... has no registered bond — agreement counterparties have no economic protection
```

### Slashing

If an attestor submits two contradicting proofs for the same agreement (one claiming
satisfied, another claiming unsatisfied), any party can record a slash:

```bash
irium-wallet attestor-slash \
  --attestor QAttestorAddress... \
  --proof1 proof-id-1 \
  --proof2 proof-id-2 \
  --agreement <agreement_hash>
```

This publishes a `slash1:<pkh>:<agreement_hash>` OP_RETURN and updates the local
bond store. The slash count appears in `attestor-list` and `attestor-bond-status`
output.

### Withdrawal Cooldown

The minimum cooldown before withdrawal is 1000 blocks, measured from:
- The registration height, if the attestor has never attested; or
- The most recent attestation height, if the attestor has signed proofs

```bash
# Withdraw after cooldown
irium-wallet attestor-withdraw-bond --from QMyAttestorAddress...
```

This publishes `bond1w:<pkh>` on-chain and marks the bond as withdrawn locally.

See `docs/ATTESTOR-GUIDE.md` for the full attestor bonding reference.
