# Settlement Integration Guide

This guide covers the Irium settlement system for developers integrating agreement-based escrow into their own applications.

---

## Overview

The Irium settlement system provides HTLC-based escrow. Funds are locked on-chain in a Hash Time-Locked Contract controlled by a 2-of-2 condition: either the recipient presents the preimage of a known secret hash (release), or the funder reclaims after a block height timeout (refund).

On top of the base HTLC, Irium adds a structured agreement layer with proof submission and policy evaluation, so that release conditions can be verified programmatically before funds are moved.

---

## Agreement Types

The on-the-wire `template_type` field uses the snake_case enum values defined in
`src/settlement.rs::AgreementTemplateType`:

| `template_type` value | Use case |
|------|----------|
| `simple_release_refund` | Two parties, one amount, one secret hash. General purpose. |
| `otc_settlement` | Buyer/seller trade. Includes asset reference and payment reference. |
| `refundable_deposit` | Payer/payee deposit. Includes purpose reference and refund summary. |
| `milestone_settlement` | Multiple milestones, each with its own amount, timeout height, and per-milestone secret hash. |
| `merchant_delayed_settlement` | Merchant-side settlement with a delayed payout deadline. |
| `contractor_milestone` | Contractor-style milestone agreement with per-leg release authorization. |

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
  --buyer  "buyer|Buyer|QBuyerAddress...|buyer" \
  --seller "seller|Seller|QSellerAddress...|seller" \
  --amount 1.0 \
  --asset-reference "50 USDT" \
  --payment-reference "SEPA transfer ref #12345" \
  --secret-hash a3f1b2c3d4e5f6071234567890abcdef1234567890abcdef1234567890abcdef \
  --refund-timeout 20500 \
  --document-hash fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210 \
  --out otc-trade-001.json
```

The `--buyer` and `--seller` flags expect a pipe-delimited `party_id|display_name|address|role(optional)` spec (parsed by `parse_party_spec` in `src/bin/irium-wallet.rs`). The `--secret-hash` and `--document-hash` values must each be exactly 64 hex characters (32 bytes); the parser rejects anything else.

The `--secret-hash` is the SHA256 of the secret preimage. The secret itself is kept private by the party who will trigger release.

The `--document-hash` is the SHA256 of any off-chain agreement document (terms, receipts, etc.). It binds the on-chain record to the off-chain agreement.

The output `otc-trade-001.json` contains the full agreement structure.

---

### Step 2: Compute the agreement hash

```bash
irium-wallet agreement-hash otc-trade-001.json
```

Output: a 32-byte hex hash. This hash uniquely identifies the agreement and is used in all subsequent API calls.

You can also compute it via the RPC API. The endpoint expects an `AgreementRequest` body — the agreement JSON wrapped in `{"agreement": ...}`:

```bash
curl -X POST http://localhost:38300/rpc/computeagreementhash \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $IRIUM_RPC_TOKEN" \
  -d "$(jq -n --argfile agr otc-trade-001.json '{agreement: $agr}')"
```

Response:
```json
{"agreement_hash": "a1b2c3d4...32bytehex..."}
```

The `Authorization` header is required whenever `IRIUM_RPC_TOKEN` is set on the node — `/rpc/computeagreementhash` and all settlement endpoints are protected.

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

Via RPC (note: takes the full agreement, not a hash, per `AgreementRequest`):
```bash
curl -X POST http://localhost:38300/rpc/agreementstatus \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $IRIUM_RPC_TOKEN" \
  -d "$(jq -n --argfile agr otc-trade-001.json '{agreement: $agr}')"
```

The response includes the current lifecycle state plus proof-finality fields:

```json
{
  "agreement_hash": "...",
  "lifecycle": {
    "state": "funded",
    "funding": { /* derived from chain */ },
    "milestones": [ /* per-milestone status, if any */ ]
  },
  "proof_depth": null,
  "proof_final": false,
  "release_eligible": false
}
```

`lifecycle.state` is one of the `AgreementLifecycleState` snake_case values: `draft`, `proposed`, `funded`, `partially_released`, `released`, `refunded`, `expired`, `cancelled`, `disputed_metadata_only`.

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

Via RPC directly. The endpoint expects a `SubmitProofRequest` body, so wrap the proof JSON in `{"proof": ...}`:
```bash
curl -X POST http://localhost:38300/rpc/submitproof \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $IRIUM_RPC_TOKEN" \
  -d "$(jq -n --argfile p proof.json '{proof: $p}')"
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

This is the canonical `AgreementObject` shape from `src/settlement.rs`. The `template_type` enum is serialized as snake_case; `parties` is an array (not a map); `payer` and `payee` are `party_id` references into the `parties` array (not addresses); there is **no** top-level `secret_hash` or `refund_timeout` field — those live inside `release_conditions[].secret_hash_hex` and `refund_conditions[].timeout_height` / `deadlines.refund_deadline` respectively. Signatures are tracked separately via `AgreementSignatureEnvelope` and are not part of the `AgreementObject`.

```json
{
  "agreement_id": "otc-trade-001",
  "version": 1,
  "schema_id": "irium.phase1.canonical.v1",
  "template_type": "otc_settlement",
  "parties": [
    {"party_id": "buyer",  "display_name": "Buyer",  "address": "QBuyerAddress...",  "role": "buyer"},
    {"party_id": "seller", "display_name": "Seller", "address": "QSellerAddress...", "role": "seller"}
  ],
  "payer": "seller",
  "payee": "buyer",
  "total_amount": 100000000,
  "network_marker": "IRIUM",
  "creation_time": 1777624133,
  "deadlines": {
    "settlement_deadline": null,
    "refund_deadline": 20500,
    "dispute_window": null
  },
  "release_conditions": [
    {
      "mode": "secret_preimage",
      "secret_hash_hex": "a3f1b2c3d4e5f6071234567890abcdef1234567890abcdef1234567890abcdef",
      "release_authorizer": "seller",
      "notes": "OTC release path: seller authorizes payout to buyer after receiving off-chain payment"
    }
  ],
  "refund_conditions": [
    {
      "refund_address": "QSellerAddress...",
      "timeout_height": 20500,
      "notes": "Refund returns escrowed IRM to seller on HTLC timeout when no release is authorized"
    }
  ],
  "milestones": [],
  "asset_reference": "50 USDT",
  "payment_reference": "SEPA transfer ref #12345",
  "release_summary": "HTLC-backed OTC release path",
  "refund_summary": "Timeout refund path for the OTC funding leg",
  "document_hash": "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210",
  "disputed_metadata_only": false
}
```

Note on OTC roles: the canonical `build_otc_agreement` function maps `payer = seller` (the party that locks IRM in the HTLC) and `payee = buyer` (the party that receives IRM after delivering off-chain payment). This is the opposite of a fiat-side mental model where the buyer "pays first" — on-chain, the seller's IRM is what gets escrowed.

### Agreement hash response

```json
{"agreement_hash": "a1b2c3d4e5f6078901234567890abcdef1234567890abcdef1234567890abcd"}
```

The `agreement_hash` value is exactly 64 hex characters (32 bytes).

### Agreement status response

The `AgreementStatusResponse` struct in `src/bin/iriumd.rs` defines the exact shape. Lifecycle information (including funding-tx visibility, milestone status, and the derived state) lives nested under `lifecycle`; proof-finality fields are top-level so callers can gate release on `release_eligible` without parsing `lifecycle`.

```json
{
  "agreement_hash": "a1b2c3d4...",
  "lifecycle": {
    "state": "funded",
    "funding": { /* funding-tx record derived from chain */ },
    "milestones": [ /* per-milestone status, milestone agreements only */ ]
  },
  "proof_depth": null,
  "proof_final": false,
  "release_eligible": false
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

The `AgreementSpendEligibilityResponse` struct returns rich context including the expected secret hash for verification, the funding leg details, and a `reasons[]` array (plural) of machine-readable codes — not a single `reason` string. Both eligibility endpoints (`/rpc/agreementreleaseeligibility` and `/rpc/agreementrefundeligibility`) take an `AgreementSpendRequest` body that includes the agreement object **and** a `funding_txid` so the node can locate the specific HTLC output being spent.

```json
{
  "agreement_hash": "a1b2c3d4...",
  "agreement_id": "otc-trade-001",
  "funding_txid": "cb7d25dc...",
  "htlc_vout": 0,
  "anchor_vout": 1,
  "role": "OtcSettlement",
  "milestone_id": null,
  "amount": 100000000,
  "branch": "release",
  "htlc_backed": true,
  "funded": true,
  "unspent": true,
  "preimage_required": true,
  "timeout_height": 20500,
  "timeout_reached": false,
  "destination_address": "QBuyerAddress...",
  "expected_hash": "a3f1b2c3d4e5f6071234567890abcdef1234567890abcdef1234567890abcdef",
  "recipient_address": "QBuyerAddress...",
  "refund_address": "QSellerAddress...",
  "eligible": true,
  "reasons": ["policy_satisfied:delivery_confirmed"],
  "trust_model_note": "..."
}
```

### Refund eligibility response (before timeout)

```json
{
  "agreement_hash": "a1b2c3d4...",
  "branch": "refund",
  "funded": true,
  "unspent": true,
  "timeout_height": 20500,
  "timeout_reached": false,
  "destination_address": "QSellerAddress...",
  "refund_address": "QSellerAddress...",
  "eligible": false,
  "reasons": ["timeout_not_reached:current=20400,target=20500"]
}
```

---

## Proof Lifecycle

Every submitted proof carries an internal lifecycle state that drives both
the explorer UI and the policy evaluator. The state is derived deterministically
from the proof itself, the current chain tip, and (for `satisfied`) the
stored release policy — so every node reaches the same conclusion.

| Status | When it applies | Meaning |
|--------|-----------------|---------|
| `active` | Proof has no expiry set, OR `tip_height < expires_at_height` | Proof is current and counts toward policy evaluation. |
| `expired` | Proof has `expires_at_height` set AND `tip_height >= expires_at_height` | Proof no longer counts; the attestor must resubmit if the agreement is still active. Returned by `GET /explorer/proofs` and `POST /rpc/listproofs` as a derived field. |
| `satisfied` | Per-agreement classification used by the outcome engine when the policy's release conditions are met by the current set of active proofs | Agreement can move to release; emitted as `agreement.satisfied` on the WebSocket stream. |

Submitting a proof against an already-`expired` agreement-status row is
accepted but has no effect — the agreement has already moved past the proof's
useful window. Use `POST /rpc/listproofs` or `GET /explorer/proofs?agreement_hash=…`
to inspect the current set.

### Attestor Thresholds

A policy can require **N-of-M attestor signatures** rather than a single
attestor. Set `attestors[]` to the list of accepted pubkeys/addresses and
`threshold` to the minimum count required. The evaluator counts only
**unique** attestor identities across the active proof set, so duplicate
proofs from the same attestor count as one. See `phase2_proof_automation.md`
for the full evaluation semantics and edge cases.

### Holdback / Retention

A policy can also declare a top-level **holdback** — a fraction of the
escrowed amount (in basis points, `holdback_bps`) that stays locked even after
the base release condition is met. The holdback releases on its own deadline
or proof requirement. See `phase2_proof_automation.md` for the JSON shape,
the `holdback_outcome` values (`pending` / `held` / `released`), and the
`agreementstatus` response fields (`holdback_present`, `holdback_released`,
`holdback_bps`, `holdback_reason`, etc.). Milestone agreements support
per-milestone holdbacks under each milestone's evaluation block.

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

## Packing and Distributing Agreements

Once an agreement is funded and a policy is stored, the parties usually need to
hand the complete state — agreement document, policy, signatures, funding-tx
record, and any already-submitted proofs — to a counterparty or attestor.
`agreement-pack` and `agreement-unpack` make this a single round-trip.

### Pack an agreement

```bash
irium-wallet agreement-pack \
  --agreement otc-trade-001 \
  --out otc-trade-001.pack.json \
  --rpc http://localhost:38300
```

Output JSON shape (top-level keys):

```json
{
  "version": 1,
  "agreement_hash": "a1b2c3d4...",
  "agreement": { /* full agreement JSON */ },
  "policy": { /* stored release policy or null */ },
  "signatures": [ /* every signature attached so far */ ],
  "funding": {
    "txid": "cb7d25dc...",
    "height": 20300,
    "confirmations": 4
  },
  "proofs": [ /* every submitted proof */ ],
  "exported_at": 1779100000,
  "exported_from": "127.0.0.1:38300"
}
```

Pulling all four pieces from the node in a single call guarantees the pack is
consistent with the chain at one specific tip — there's no race window where
(say) the policy moved on but the proofs reference an older policy.

### Unpack and verify

The receiving party verifies the document hash, agreement hash, all embedded
signatures, and the on-chain status before importing anything to their local
wallet:

```bash
irium-wallet agreement-unpack \
  --file otc-trade-001.pack.json \
  --rpc http://localhost:38300
```

If any signature is invalid, the pack's agreement_hash doesn't match the
sender's agreement JSON, or the on-chain status contradicts the pack's
`funding` block, the command fails with a non-zero exit and no local state is
modified.

This pairs well with the OTC offer flow: seller packs immediately after
funding → buyer unpacks → buyer verifies the funding tx independently before
delivering the off-chain side of the trade.

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
| Funding tx not yet confirmed | `agreementstatus` returns `lifecycle.state: "draft"` or `"proposed"` and `release_eligible: false` |
| Release attempted before policy satisfied | `agreementreleaseeligibility` returns `eligible: false` with codes in `reasons[]` (e.g. `["policy_not_satisfied"]`) |
| Refund attempted before timeout | `agreementrefundeligibility` returns `eligible: false` with codes in `reasons[]` (e.g. `["timeout_not_reached:current=N,target=M"]`) |
| Proof submitted after release | Proof accepted but has no effect on eligibility |
| Wrong secret provided | Release transaction will fail validation at the node |
| Agreement hash mismatch | Node rejects the request with a 400 error |

---

## Milestone Agreements

Milestone agreements release funds in stages. Each milestone has:
- An amount
- A proof type requirement
- A timeout height

Creating a milestone agreement. Real CLI uses `--payer` / `--payee` (pipe-delimited party specs, same as OTC `--buyer` / `--seller`), one `--milestone` flag per milestone (pipe-delimited `id|title|amount_irm|timeout_height|secret_hash_hex|deliverable_hash(optional)`), and `--refund-deadline` (not `--refund-timeout`). There is no top-level `--secret-hash` for milestone agreements — each milestone carries its own:

```bash
irium-wallet agreement-create-milestone \
  --agreement-id milestone-001 \
  --creation-time $(date +%s) \
  --payer "payer|Client|QClientAddress...|client" \
  --payee "payee|Contractor|QContractorAddress...|contractor" \
  --milestone "m1|Design phase|0.5|20800|1111111111111111111111111111111111111111111111111111111111111111" \
  --milestone "m2|Implementation|1.0|20900|2222222222222222222222222222222222222222222222222222222222222222" \
  --refund-deadline 21000 \
  --document-hash fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210 \
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


---

## Proof Finality and Reorg Protection

### What is Proof Finality Depth?

When a proof is submitted, it is included in a block and that block enters the chain.
However, any block can theoretically be reorganized away if a longer competing chain
appears. Until a block is buried deep enough under subsequent blocks, it is not
considered final.

Irium uses a configurable **proof finality depth** (default: 6 blocks) to protect
settlement agreements from ambiguous states caused by reorgs.

A proof is only considered final — and release eligibility is only granted — after the
block containing the proof is at least `PROOF_FINALITY_DEPTH` blocks deep.

### Configuring Finality Depth

Set the `IRIUM_PROOF_FINALITY_DEPTH` environment variable before starting iriumd:

```bash
# Use a shorter depth for testing (faster confirmation)
IRIUM_PROOF_FINALITY_DEPTH=2 ./iriumd

# Use a deeper finality for high-value agreements (more conservative)
IRIUM_PROOF_FINALITY_DEPTH=12 ./iriumd
```

The default of 6 blocks matches the conventional finality threshold for SHA-256d PoW
chains. For most commercial agreements, 6 blocks is sufficient.

### Agreement Status Fields

The `agreement-status` RPC now returns three finality fields:

```json
{
  "agreement_hash": "...",
  "lifecycle": { ... },
  "proof_depth": 4,
  "proof_final": false,
  "release_eligible": false
}
```

| Field | Type | Description |
|---|---|---|
| `proof_depth` | `number \| null` | How many blocks deep the proof is. `null` if no proof submitted yet. |
| `proof_final` | `bool` | `true` when `proof_depth >= PROOF_FINALITY_DEPTH`. |
| `release_eligible` | `bool` | `true` when `proof_final` is `true` and the agreement is in a releasable state (Funded or PartiallyReleased). |

**Agreement parties should wait for `release_eligible: true` before considering a
settlement complete.** Do not act on `proof_final: false` as the proof may be rolled
back by a chain reorg.

### Progression Example

With `PROOF_FINALITY_DEPTH=6`:

| Blocks after proof | proof_depth | proof_final | release_eligible |
|---|---|---|---|
| 0 (just submitted) | 0 | false | false |
| 1 | 1 | false | false |
| 2 | 2 | false | false |
| 3 | 3 | false | false |
| 4 | 4 | false | false |
| 5 | 5 | false | false |
| 6 | 6 | true | true |

### Reorg Recovery

If a chain reorganization occurs and the block containing a submitted proof is
reorganized away, iriumd detects this automatically.

When a reorg is detected:
1. Any proof whose submission block is no longer in the canonical chain is re-evaluated.
2. If the proof no longer appears in the chain, the agreement's `proof_depth` resets.
3. A `agreement.proof_reorged` WebSocket event is emitted for each affected agreement.
4. The seller should resubmit the proof once the network stabilizes.

The `agreement.proof_reorged` event payload:

```json
{
  "event": "agreement.proof_reorged",
  "agreement_hash": "abc123...",
  "reorged_at_height": 20491,
  "new_tip_height": 20490
}
```

### WebSocket Subscription for Finality Events

To monitor proof finality progression in real time:

```json
{
  "action": "subscribe",
  "events": ["agreement.proof_submitted", "agreement.satisfied", "agreement.proof_reorged"],
  "filter": { "agreement_hash": "your_agreement_hash_here" }
}
```

Use `agreement.satisfied` as the final confirmation that a settlement is complete and
release-eligible. If `agreement.proof_reorged` arrives, resubmit the proof and wait
again.

## Private Agreements (Off-Chain Storage)

All agreement content is visible locally by default. Business users negotiating
sensitive contracts can keep the full agreement content private by using the
`--private` flag. Only the agreement hash is anchored on-chain; the content stays in
`~/.irium/private-agreements/` on the local machine.

### Creating a Private Agreement

Add `--private` to any `agreement-create-*` command:

```sh
irium-wallet agreement-create-otc \
  --agreement-id "order-2026-001" \
  --buyer "buyer-id|Buyer Name|QBuyerAddr..." \
  --seller "seller-id|Seller Name|QSellerAddr..." \
  --amount 5.0 \
  --asset-reference "Confidential goods" \
  --payment-reference "Wire REF-001" \
  --secret-hash <32bytehex> \
  --refund-timeout 30000 \
  --document-hash <32bytehex> \
  --private
```

Output:

```
[private] agreement stored: /home/irium/.irium/private-agreements/7d52a7fe...json
```

The full agreement JSON is stored locally. The RPC endpoint returns 404 for this hash
because the content was never broadcast to the network.

### Selective Disclosure — Sharing with a Counterparty

To share the agreement content with a specific recipient, encrypt it using their
secp256k1 public key (ECIES with AES-256-GCM):

```sh
irium-wallet agreement-share <agreement_hash> <recipient_pubkey_hex> [--out blob.json]
```

Example:

```sh
irium-wallet agreement-share \
  7d52a7fe80f5c676d52a4ae1234617a0252418ddff1bcea02e80f0294c15716d \
  03e918af472e63de044c983df9f09bae57d4c78a70998d5d5fded408672886f868 \
  --out shared-agreement.json
```

This produces a self-describing encrypted blob:

```json
{
  "version": 1,
  "scheme": "ecies-secp256k1-aes256gcm",
  "ephemeral_pubkey": "03c5fc129c...",
  "nonce": "9aa2f08a7595c65b15c669d4",
  "ciphertext": "16182935cd..."
}
```

Send this blob to the recipient by any channel (email, Signal, etc.). The blob contains
no plaintext — only the recipient private key can decrypt it.

### Decrypting a Received Blob

The recipient decrypts using their wallet:

```sh
irium-wallet agreement-decrypt shared-agreement.json \
  --wallet ~/.irium/wallet.json \
  [--store-private] [--json]
```

- Without `--store-private`: prints the agreement to stdout.
- With `--store-private`: stores it in `~/.irium/private-agreements/` for later use.

The command tries every key in the wallet until one succeeds. If no key matches,
it exits with an error.

### Proof Submission for Private Agreements

No change is needed for proof submission. The proof references the agreement hash
which is already on-chain. The full agreement content is not required for proof
validation.

```sh
irium-wallet agreement-proof-submit --proof proof.json [--rpc <url>]
```

### Network Privacy

When `--private` is used, the agreement content is never transmitted over the network.
The only on-chain anchor is the 32-byte hash. The P2P gossip layer never sees the
full agreement terms.

To verify: iriumd gossip logs show only proof and offer messages. Agreement content
for private agreements does not appear in any gossip log entry.

### Encryption Scheme

The ECIES scheme used is:

1. Generate a random ephemeral secp256k1 key pair.
2. ECDH: shared secret = ephemeral_secret x recipient_pubkey (x-coordinate).
3. Derive AES key: SHA256(shared_secret_x_bytes).
4. Encrypt plaintext with AES-256-GCM using a random 12-byte nonce.
5. Output: JSON blob with ephemeral_pubkey, nonce, ciphertext.

Decryption reverses step 2 using the recipient private key:
shared_secret = recipient_secret x ephemeral_pubkey.

The shared secret is identical in both directions (ECDH property), so the same
AES key is derived and the ciphertext can be decrypted.


---

# Dispute and Resolver System (Group B, Stages 3.0–3.4.1)

The dispute system layers on top of the base agreement escrow. Either party can raise a dispute when the off-chain side of an OTC, Milestone, or Contractor agreement breaks down. A nominated resolver — verified as a recent miner — then decides release-or-refund, with a fee paid out of the disputed escrow at spend time.

**Scope:** disputes apply to `otc_settlement`, `milestone_settlement`, and `contractor_milestone` agreements only. `refundable_deposit`, `simple_release_refund`, and `merchant_delayed_settlement` reject resolver fields at validation time.

---

## Resolver fields on the agreement

The agreement creator names up to two resolvers and locks their fees:

```json
{
  "primary_resolver":         "Q...",
  "fallback_resolver":        "Q...",
  "primary_resolver_fee":     1000000,
  "fallback_resolver_fee":    500000
}
```

- Both addresses must be valid P2PKH (`Q…`).
- Fees are in satoshis, must be `> 0`, and the combined sum must be `< total_amount`.
- `fallback_resolver` requires `primary_resolver` to also be set.
- The address must have appeared in a coinbase output within the last 2 016 blocks at registration time (miner-recency check enforced by `/rpc/registerresolver`).
- All four fields are optional. An agreement without resolvers can still be raised as a dispute, but no resolver can act on it.

---

## End-to-end flow

```
   ┌── buyer ──┐                                  ┌── seller ──┐
   │ off-chain │                                  │ off-chain  │
   │ payment   │                                  │ delivery   │
   └─────┬─────┘                                  └─────┬──────┘
         │                                              │
         │ (a) raise dispute                            │
         ▼                                              │
   ┌─────────────────────────────────────┐              │
   │   POST /rpc/raisedispute            │              │
   │   - signs DisputeRaise with party   │              │
   │     key                             │              │
   │   - iriumd anchors OP_RETURN tx     │              │
   │   - HTLC frozen via eligibility hook│              │
   └─────────────────┬───────────────────┘              │
                     │ P2P broadcast                    │
                     ▼                                  │
   ┌─────────────────────────────────────┐              │
   │   other party drains inbox, applies │◄─────────────┘
   │   to local disputes_index           │
   └─────────────────┬───────────────────┘
                     │ (b) optional: respond
                     ▼
   ┌─────────────────────────────────────┐
   │   POST /rpc/disputeevidence         │
   │   - either party submits evidence   │
   │   - anchored on-chain               │
   └─────────────────┬───────────────────┘
                     │ (c) resolve within 288 blocks
                     ▼
   ┌─────────────────────────────────────┐
   │   POST /rpc/resolvedispute          │
   │   - primary or fallback resolver    │
   │   - outcome: release | refund       │
   │   - HTLC unfreezes for the matching │
   │     branch only                     │
   └─────────────────┬───────────────────┘
                     │ (d) winner spends
                     ▼
   ┌─────────────────────────────────────┐
   │   POST /rpc/buildagreementrelease   │
   │     (or buildagreementrefund)       │
   │   - tx pays winner (value - fee)    │
   │   - tx pays resolver (fee)          │
   └─────────────────────────────────────┘
```

If the primary resolver does not anchor a resolution within 288 blocks of `raise_anchored_at_height`, the dispute auto-escalates to the fallback resolver. If both resolvers fail, the parties can co-sign a `/rpc/reresolveagreement` nomination of a new pair.

---

## Canonical types

All dispute payloads are signed by their originator with a secp256k1 `AgreementSignatureEnvelope`. The signature is over `Sha256(canonical_bytes_with_signature_field_cleared)`. The signer's pubkey must derive to the same address that appears in `envelope.signer_address`.

| Type | Schema id | Signed by | Required fields |
|---|---|---|---|
| `DisputeRaise` | `irium.phase3.dispute_raise.v1` | raising party | `agreement_hash`, `raising_party`, `raised_at_height`, `raised_at_unix`, `reason`, `initial_evidence_hash` |
| `DisputeEvidence` | `irium.phase3.dispute_evidence.v1` | submitter party | `agreement_hash`, `submitter_party`, `submitted_at_height`, `evidence_type` ∈ {`payment_proof`, `delivery_proof`, `communication_proof`}, `evidence_payload`, `evidence_hash` |
| `DisputeResolution` | `irium.phase3.dispute_resolution.v1` | resolver | `agreement_hash`, `resolver_address`, `resolver_role` ∈ {`primary`, `fallback`}, `outcome` ∈ {`release`, `refund`}, `resolved_at_height`, `message` |
| `DisputeReResolverNomination` | `irium.phase3.dispute_reresolve.v1` | BOTH parties | `agreement_hash`, `new_primary_resolver`, `new_fallback_resolver?`, `nominated_at_height`, two signatures |
| `ResolverRegistration` | `irium.phase3.resolver_registration.v1` | resolver | `resolver_address`, `registered_at_height`, optional `display_name`/`bio`/`fee_bps_self_quoted` |

---

## Anchor roles

Each dispute action emits an OP_RETURN anchor in a small follow-on tx funded by the iriumd wallet. The anchor payload is `agr1:<agreement_hash>:<role_code>`:

| Role | Short code |
|---|---|
| `DisputeRaise` | `e` |
| `DisputeEvidence` | `v` |
| `DisputeResolve` | `x` |
| `ResolverRegister` | `y` |

Cap: the OP_RETURN data fits within 75 bytes (5 prefix + 64 hash + 1 colon + 1 code = 71 bytes).

---

## iriumd RPC endpoints

All `POST` endpoints require `IRIUM_RPC_TOKEN` via bearer-auth header. `GET /resolvers/list` is public.

| Endpoint | Method | Description |
|---|---|---|
| `/rpc/raisedispute` | POST | Validates + verifies sig + anchors + inserts; rejects deposit template or duplicate open dispute |
| `/rpc/disputeevidence` | POST | Appends evidence record to open dispute; anchors |
| `/rpc/resolvedispute` | POST | Verifies resolver_address against agreement or reresolve nomination; records resolution; anchors |
| `/rpc/registerresolver` | POST | Miner-recency check (last 2 016 blocks); inserts/replaces; anchors |
| `/rpc/reresolveagreement` | POST | Co-signed by both parties; rejects if dispute already resolved; resets escalation |
| `/rpc/disputestate?agreement_hash=X` | GET | Returns current DisputeState JSON with `found:bool` flag |
| `/resolvers/list?limit=N&cursor=X` | GET (public) | Paginated list of registered resolvers, sorted by `registered_at_height` desc |

### Eligibility hook

`/rpc/agreementreleaseeligibility` and `/rpc/agreementrefundeligibility` (and their build counterparts) consult `disputes_index` after the chain-side check:

- Open dispute → `eligible: false`, reason `"dispute_open"`.
- Resolved dispute with outcome `"release"` → release branch passes; refund returns `"dispute_resolution_blocks_branch"`.
- Resolved dispute with outcome `"refund"` → mirror image.

---

## Wallet CLI

All signing-required commands take `--key <hex|wif>`. The key derives both the signer's address (P2PKH, base58) and the secp256k1 SEC1 public key embedded in the envelope. The wallet also adds the bearer `Authorization` header from `IRIUM_RPC_TOKEN` automatically.

```bash
# Raise a dispute
irium-wallet agreement-dispute-raise \
  --agreement /path/to/agreement.json \
  --raising-party buyer \
  --reason "seller refused release after fiat payment" \
  --evidence-file /path/to/proof.pdf \
  --key <hex-or-wif>

# Respond with evidence
irium-wallet agreement-dispute-respond \
  --agreement /path/to/agreement.json \
  --submitter-party seller \
  --evidence-file /path/to/delivery-receipt.pdf \
  --evidence-type delivery_proof \
  --message "shipping receipt attached" \
  --key <hex-or-wif>

# Resolver decides
irium-wallet agreement-dispute-resolve \
  --agreement /path/to/agreement.json \
  --outcome release \
  --resolver-role primary \
  --message "buyer-supplied bank wire confirmation matches seller's amount" \
  --key <resolver-hex-or-wif>

# Inspect current state
irium-wallet agreement-dispute-show \
  --agreement /path/to/agreement.json \
  [--json]

# Co-sign a fresh resolver pair (both parties needed)
irium-wallet agreement-dispute-reresolve \
  --agreement /path/to/agreement.json \
  --new-resolver Q...new-primary... \
  --new-fallback Q...new-fallback... \
  --key-a <buyer-hex-or-wif> \
  --key-b <seller-hex-or-wif>

# Resolver opts in
irium-wallet resolver-register \
  --display-name "Pacific Arbitration" \
  --bio "OTC settlement specialist" \
  --fee-bps 50 \
  --key <resolver-hex-or-wif>

# Browse the resolver feed
irium-wallet resolver-list [--limit N] [--cursor Q...]
```

---

## P2P propagation

When a dispute action succeeds locally, iriumd best-effort broadcasts the canonical payload to all connected peers:

| Action | MessageType | Payload struct |
|---|---|---|
| Raise | `21 DisputeRaisedNotification` | `DisputeRaisedNotificationPayload` |
| Evidence | `22 DisputeEvidenceNotification` | `DisputeEvidenceNotificationPayload` |
| Resolve | `23 DisputeResolvedNotification` | `DisputeResolvedNotificationPayload` |
| Escalate | `24 DisputeEscalatedNotification` | `DisputeEscalatedNotificationPayload` |

Receivers drop the JSON into per-message-type inboxes. A 5 s drain task in iriumd consumes each inbox and applies the change to the local `disputes_index` after re-verifying the signature. The on-chain OP_RETURN anchor remains the durable record; the in-memory + on-disk index is each node's operational cache.

---

## Reputation impact

A resolved dispute affects local reputation tracking (`wallet.rs` outcomes.json):

- Losing party gets `default_count` incremented.
- Winning party gets `successful_trades_count` incremented.

Currently the wallet records this when the local user observes the resolution; on-chain reputation anchoring is Group H work and not part of Stage 3.

---

## Trust model summary

| Property | Enforcement |
|---|---|
| Resolver fee output value | Wallet-side at spend tx build time (chain enforces tx integrity but not split policy) |
| Resolver identity | secp256k1 signature + agreement's `primary_resolver`/`fallback_resolver` field or co-signed reresolve nomination |
| Miner-recency for resolvers | iriumd at `/rpc/registerresolver` and `/rpc/reresolveagreement` (last 2 016 blocks of coinbase outputs) |
| HTLC freeze during open dispute | Wallet-side (eligibility advisory + build refusal). Sophisticated parties can spend the HTLC via raw tools after timeout; the chain does not enforce dispute-aware locking. |
| Dispute audit trail | OP_RETURN anchor tx for each action with role code `e`/`v`/`x`/`y` |

Future work tagged in commit messages includes on-chain reputation anchoring (Group H), HTLCv2 with a dispute-lock branch, and dispute-evidence storage indexed by chain-anchored evidence-hash.
