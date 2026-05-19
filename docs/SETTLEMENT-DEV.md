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
