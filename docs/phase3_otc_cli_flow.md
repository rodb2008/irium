# Phase 3 OTC CLI Flow

## Overview

This document describes the simplified guided OTC (over-the-counter) flow introduced in the `otc-create`, `otc-attest`, `otc-settle`, and `otc-status` commands.

## Old vs New Flow

### Old flow (10+ commands, manual wiring)

```
# Step 1: Create agreement object
irium-wallet agreement-create-otc \
  --agreement-id otc-$(date +%s) \
  --creation-time $(date +%s) \
  --buyer "buyer|Buyer|<buyer_addr>|buyer" \
  --seller "seller|Seller|<seller_addr>|seller" \
  --amount 1.0 \
  --asset-reference IRM \
  --payment-reference bank-transfer \
  --refund-timeout 144 \
  --secret-hash $(openssl rand -hex 32) \
  --document-hash $(openssl rand -hex 32) \
  --out agreement.json

# Step 2: Compute canonical hash
irium-wallet agreement-hash agreement.json

# Step 3: Build OTC policy
irium-wallet policy-build-otc \
  --policy-id pol-1 \
  --agreement-hash <hash> \
  --attestor "attestor:$(irium-wallet ...) " \
  --release-proof-type otc_release \
  --refund-deadline-height 144

# Step 4: Store policy on node
irium-wallet agreement-policy-set \
  --policy pol.json

# Step 5: Create signed proof
irium-wallet agreement-proof-create \
  --agreement-hash <hash> \
  --proof-type otc_release \
  --attested-by <address> \
  --address <address> \
  --evidence-summary "payment confirmed"

# Step 6: Submit proof to node
irium-wallet agreement-proof-submit \
  --proof proof.json

# Step 7: Evaluate policy
irium-wallet agreement-policy-evaluate \
  --agreement agreement.json

# Step 8: Build settlement transaction
irium-wallet agreement-build-settlement agreement.json

# Step 9: Check overall settle status
irium-wallet agreement-settle-status agreement.json
```

### New flow (4 commands, guided)

```
# Step 1: Create agreement (auto-generates IDs, hashes, saves locally)
irium-wallet otc-create \
  --seller <seller_address> \
  --buyer <buyer_address> \
  --amount 1.0 \
  --asset IRM \
  --payment-method bank-transfer \
  --timeout 144

# Step 2: Attestor submits proof
irium-wallet otc-attest \
  --agreement <agreement_hash_or_path> \
  --message "payment confirmed" \
  --address <attestor_address>

# Step 3: Evaluate and build settlement
irium-wallet otc-settle \
  --agreement <agreement_hash_or_path>

# Step 4 (any time): Check full status
irium-wallet otc-status \
  --agreement <agreement_hash_or_path>
```

---

## Command Reference

### `otc-create`

Creates an OTC agreement locally and saves it to `~/.irium/agreements/raw/<hash>.json`.

**Required flags:**
- `--seller <address>` - Irium address of the seller
- `--buyer <address>` - Irium address of the buyer
- `--amount <IRM>` - Amount to trade (e.g. `1.0`, `0.5`)
- `--asset <string>` - Asset identifier (e.g. `IRM`)
- `--payment-method <string>` - Off-chain payment method (e.g. `bank-transfer`)
- `--timeout <blocks>` - Refund timeout in blocks (e.g. `144`)

**Optional flags:**
- `--agreement-id <id>` - Custom agreement ID (default: `otc-<timestamp>`)
- `--out <path>` - Also write agreement JSON to this path
- `--json` - Machine-readable JSON output

**Output:**
```
agreement_id    otc-1745600000
agreement_hash  a3f2...d8c1
saved_path      /home/user/.irium/agreements/raw/a3f2...d8c1.json

next_step  irium-wallet otc-attest --agreement a3f2...d8c1 --message "<payment confirmed>" --address <attestor_address>
```

**What it auto-generates:**
- `agreement_id` from current Unix timestamp
- `creation_time` from current Unix timestamp
- `secret_hash` and `document_hash` from deterministic SHA-256 of the agreement ID + timestamp

---

### `otc-attest`

Creates a signed proof and submits it to the node via RPC. Used by the attestor (buyer, seller, or third party) to confirm that the off-chain payment has taken place.

**Required flags:**
- `--agreement <hash_or_path>` - Agreement hash or path to agreement JSON file
- `--message <string>` - Evidence summary (e.g. `"payment confirmed"`)
- `--address <address>` - Signing address (must exist in wallet)

**Optional flags:**
- `--proof-type <type>` - Proof type (default: `otc_release`)
- `--rpc <url>` - Node RPC URL (default: from env or `http://localhost:8080`)
- `--json` - Machine-readable JSON output

**Output:**
```
proof_id        prf-a1b2c3d4e5f6a7b8
agreement_hash  a3f2...d8c1
accepted        true
duplicate       false
message         proof accepted
tip_height      18240
expires_at_height none
expired         false
status          active

next_step  irium-wallet otc-settle --agreement a3f2...d8c1
```

---

### `otc-settle`

Evaluates the policy and builds the settlement transaction. Use after attestation.

**Required flags:**
- `--agreement <hash_or_path>` - Agreement hash or path to agreement JSON file

**Optional flags:**
- `--rpc <url>` - Node RPC URL
- `--json` - Machine-readable JSON output

**Output:**
```
=== policy evaluation ===
agreement_hash  a3f2...d8c1
policy_found    true
outcome         satisfied
...

=== settlement actions ===
agreement_hash  a3f2...d8c1
release_eligible true
action_count    1
action[0] release recipient=<seller_addr> bps=10000 executable=true executable_after=now

next_step  Agreement is satisfied. Execute the settlement transaction to release funds.
```

**Outcome values:**
- `satisfied` - Release eligible; settlement can proceed
- `timeout` - Refund eligible; timeout has passed
- `unsatisfied` - Neither eligible; waiting for attestation

---

### `otc-status`

Shows the complete current state of an OTC agreement: hash, policy, proofs submitted, evaluation result, settlement actions, and deadline.

**Required flags:**
- `--agreement <hash_or_path>` - Agreement hash or path to agreement JSON file

**Optional flags:**
- `--rpc <url>` - Node RPC URL

**Output:**
```
=== agreement ===
agreement_id    otc-1745600000
agreement_hash  a3f2...d8c1
amount          1.00000000 IRM
asset           IRM
payment_method  bank-transfer

=== policy ===
agreement_hash  a3f2...d8c1
found           true
...

=== proofs (1) ===
  proof_id=prf-a1b2...  type=otc_release  attested_by=Qattest...  time=18239

=== evaluation ===
...

=== settlement actions ===
...

=== deadline ===
refund_deadline_height 18384
ready_to_settle        true
```

---

## Example: Full OTC Flow

```sh
# Both parties agree on terms off-chain. Seller initiates.

# 1. Create agreement
irium-wallet otc-create \
  --seller Qseller1abc...xyz \
  --buyer  Qbuyer1abc...xyz \
  --amount 5.0 \
  --asset  IRM \
  --payment-method USDT-TRC20 \
  --timeout 288

# Output shows agreement_hash = abc123...
# Agreement saved to ~/.irium/agreements/raw/abc123....json

# 2. Buyer confirms payment off-chain, then attests
irium-wallet otc-attest \
  --agreement abc123... \
  --message "USDT payment TxID: 0x...sent" \
  --address Qbuyer1abc...xyz

# 3. Check status (both parties can do this)
irium-wallet otc-status \
  --agreement abc123...

# 4. Settle when ready
irium-wallet otc-settle \
  --agreement abc123...
```

---

## Design Notes

- `otc-create` is purely local; it requires no network connection.
- `otc-attest`, `otc-settle`, and `otc-status` require a running node accessible via RPC.
- The existing low-level commands (`agreement-create-otc`, `policy-build-otc`, `agreement-proof-create`, etc.) remain available and unchanged for advanced use.
- No consensus changes, LWMA changes, HTLCv1 changes, or activation heights are involved in this UX layer.