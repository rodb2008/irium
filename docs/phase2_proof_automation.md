# Phase 2: Proof-Based Objective Automation

## Status

MVP implemented and merged to `phase2-proof-automation` branch.
Commits: `081161e`, `5950acc`, `b8d2294`.

This document covers only what is currently implemented.
Items not yet implemented are marked explicitly.

---

## Purpose and scope

Phase 1 settlement established a structured agreement layer (HTLCv1-backed, off-chain
coordination). Phase 2 adds a policy evaluation layer: a caller submits an agreement,
a proof policy, and zero or more signed proofs. The node evaluates whether the policy
conditions are satisfied and returns a structured eligibility result.

All Phase 2 logic is application-layer only. No consensus rules, block validation,
mining, or on-chain transaction construction is changed. Policy evaluation is stateless
and read-only at the node — the node does not store policies or proofs.

---

## ProofPolicy JSON shape

A proof policy binds a set of requirements and timeout rules to a specific agreement.

```json
{
  "policy_id": "pol-001",
  "schema_id": "irium.phase2.proof_policy.v1",
  "agreement_hash": "<64-char hex SHA-256 of canonical agreement JSON>",
  "required_proofs": [
    {
      "requirement_id": "req-001",
      "proof_type": "delivery_confirmation",
      "required_by": null,
      "required_attestor_ids": ["attestor-a"],
      "resolution": "release",
      "milestone_id": null
    }
  ],
  "no_response_rules": [
    {
      "rule_id": "rule-refund-500",
      "deadline_height": 500,
      "trigger": "funded_and_no_release",
      "resolution": "refund",
      "milestone_id": null,
      "notes": null
    }
  ],
  "attestors": [
    {
      "attestor_id": "attestor-a",
      "pubkey_hex": "<uncompressed secp256k1 SEC1 hex>",
      "display_name": "Trusted Escrow Service",
      "domain": null
    }
  ],
  "notes": null,
  "milestones": [
    { "milestone_id": "ms-delivery", "label": "Delivery confirmation" },
    { "milestone_id": "ms-inspection", "label": "Inspection sign-off" }
  ],
  "holdback": {
    "holdback_bps": 1000,
    "release_requirement_id": "req-holdback",
    "deadline_height": 50000
  }
}
```

### Field notes

- `schema_id` must be `"irium.phase2.proof_policy.v1"` (constant `PROOF_POLICY_SCHEMA_ID`).
- `agreement_hash` must match the SHA-256 of the canonical JSON of the agreement passed
  in the same request. Mismatch returns a 400 error.
- `required_attestor_ids` lists attestor IDs from the `attestors` array.
- `resolution` values: `"release"`, `"refund"`, `"milestone_release"`.
- `trigger` values: `"funded_and_no_release"`, `"disputed_and_no_response"`.
- `pubkey_hex` must be the uncompressed secp256k1 public key as hex (65 bytes, 130 chars).

### Not yet implemented

- `required_by` (block height deadline on a ProofRequirement) is stored but not evaluated.
- `resolution: "refund"` on a `ProofRequirement` is stored but not evaluated. Proof-triggered
  refunds are only reachable via `no_response_rules` in the current implementation.
- `milestone_id` scoping: requirements and rules with a `milestone_id` are now
  evaluated independently per milestone when the policy declares a `milestones` array.
  See [Milestone / tranche-based evaluation](#milestone--tranche-based-evaluation) below.

---

## SettlementProof JSON shape

A settlement proof is a signed attestation that a real-world condition has been met.

```json
{
  "proof_id": "prf-001",
  "schema_id": "irium.phase2.settlement_proof.v1",
  "proof_type": "delivery_confirmation",
  "agreement_hash": "<64-char hex SHA-256 of canonical agreement JSON>",
  "milestone_id": null,
  "attested_by": "attestor-a",
  "attestation_time": 1700000000,
  "evidence_hash": null,
  "evidence_summary": "Goods delivered and signed for.",
  "signature": {
    "signature_type": "secp256k1_ecdsa_sha256",
    "pubkey_hex": "<uncompressed secp256k1 SEC1 hex>",
    "signature_hex": "<64-byte compact ECDSA signature as hex>",
    "payload_hash": "<64-char hex SHA-256 of the proof payload>"
  }
}
```

### Payload hash computation

The payload that is hashed and signed covers all proof fields except the `signature`
envelope itself. It is computed as:

1. Construct a JSON object with exactly these keys:
   `proof_id`, `schema_id`, `proof_type`, `agreement_hash`, `milestone_id`,
   `attested_by`, `attestation_time`, `evidence_hash`, `evidence_summary`.
2. Sort keys lexicographically (recursive, same as agreement canonical hash).
3. Serialize to compact JSON (no whitespace).
4. SHA-256 hash the UTF-8 bytes.

The resulting 32-byte digest is the payload hash. The ECDSA signature is over this
digest using `sign_prehash`. The hex-encoded hash goes into `signature.payload_hash`;
the 64-byte compact signature goes into `signature.signature_hex`.

The public function `settlement_proof_payload_bytes(proof)` in `src/settlement.rs`
returns the canonical payload bytes for external signing tools.

### Field notes

- `schema_id` must be `"irium.phase2.settlement_proof.v1"` (constant `SETTLEMENT_PROOF_SCHEMA_ID`).
- `attested_by` must match an `attestor_id` in the policy's `attestors` list.
- `pubkey_hex` in the signature envelope must match the `pubkey_hex` registered for
  that attestor in the policy. Mismatch causes verification failure.
- `attestation_time` is a Unix timestamp (u64). No range validation is enforced.
- `signature_type` must be `"secp256k1_ecdsa_sha256"` (constant `AGREEMENT_SIGNATURE_TYPE_SECP256K1`).

---

## POST /rpc/checkpolicy

Evaluates a proof policy against a set of submitted proofs at the current chain tip.

### Authentication

Requires the same `Authorization: Bearer <IRIUM_RPC_TOKEN>` header as all other
`/rpc/` endpoints. Rate-limited by the standard node rate limiter.

### Request

```
POST /rpc/checkpolicy
Content-Type: application/json
Authorization: Bearer <token>
```

```json
{
  "agreement": { ... },
  "policy": { ... },
  "proofs": [ ... ]
}
```

- `agreement`: a full `AgreementObject` (same shape as used by Phase 1 RPC endpoints).
- `policy`: a `ProofPolicy` object (see above). `policy.agreement_hash` must match the
  SHA-256 of the canonical agreement.
- `proofs`: array of `SettlementProof` objects. May be empty — useful for testing
  no-response rule deadlines without submitting proofs.

### Response (200 OK)

```json
{
  "agreement_hash": "<64-char hex>",
  "policy_id": "pol-001",
  "tip_height": 1500,
  "proof_count": 1,
  "expired_proof_count": 0,
  "matched_proof_count": 1,
  "matched_proof_ids": ["prf-001"],
  "release_eligible": true,
  "refund_eligible": false,
  "reason": "all release requirements satisfied by verified proofs",
  "evaluated_rules": [
    "proof 'prf-001' verified ok"
  ]
}
```

- `agreement_hash`: SHA-256 of the canonical agreement, computed by the node.
- `tip_height`: chain tip height at the moment of evaluation.
- `proof_count`: active (non-expired) proofs considered for evaluation.
- `expired_proof_count`: proofs filtered out as expired before evaluation.
- `matched_proof_count`: proofs that passed signature verification and matched
  the policy attestor list.
- `matched_proof_ids`: IDs of those matched proofs.
- `release_eligible` / `refund_eligible`: at most one will be true per response.
- `outcome`: deterministic classification — `satisfied`, `timeout`, or `unsatisfied`. Shown by `agreement-policy-evaluate` immediately after `tip_height`.
- `reason`: human-readable explanation of the outcome.
- `evaluated_rules`: ordered list of strings describing each step taken, including
  verified proofs, rejected proofs, and triggered or pending deadline rules.

### Error responses

- `400 Bad Request` with body `agreement_hash_failed:<detail>` — agreement failed
  to hash (malformed agreement object).
- `400 Bad Request` with body `policy_eval_failed:<detail>` — policy evaluation
  failed, most commonly because `policy.agreement_hash` does not match the supplied
  agreement. The detail contains both hashes.
- `401 Unauthorized` — missing or invalid RPC token.
- `429 Too Many Requests` — rate limit exceeded.

### Evaluation semantics

1. The node computes the canonical SHA-256 of the agreement and validates it against
   `policy.agreement_hash`. Mismatch returns 400 immediately.
2. No-response rules are evaluated first, in order. The first rule whose
   `deadline_height <= tip_height` fires and the result is returned immediately.
   Remaining rules and proofs are not evaluated after a deadline fires.
3. If no deadline fires, each submitted proof is verified:
   - `attested_by` must be in the policy `attestors` list with matching `pubkey_hex`.
   - The ECDSA signature over the payload hash must verify.
   - Invalid proofs are logged in `evaluated_rules` as `proof '<id>' rejected: <reason>`
     but do not abort the call.
4. After proof verification, the node checks whether all `required_proofs` with
   `resolution: "release"` or `resolution: "milestone_release"` are satisfied by at
   least one verified proof matching on `proof_type` and `attested_by`.
5. If all release requirements are satisfied, `release_eligible: true` is returned.
6. Otherwise both flags are false and the reason says no condition was met.

---

## irium-wallet agreement-policy-check

```
irium-wallet agreement-policy-check \
  --agreement <agreement.json|-> \
  --policy <policy.json|-> \
  [--proof <proof.json>]... \
  [--rpc <url>] \
  [--json]
```

### Arguments

| Flag | Required | Description |
|---|---|---|
| `--agreement <path\|->` | yes | Path to agreement JSON file, or `-` for stdin |
| `--policy <path\|->` | yes | Path to proof policy JSON file, or `-` for stdin |
| `--proof <path\|->` | no, repeatable | Path to a settlement proof JSON file. May be specified multiple times |
| `--rpc <url>` | no | Node RPC base URL. Defaults to `IRIUM_NODE_RPC` env var or `http://127.0.0.1:8338` |
| `--json` | no | Output raw JSON response instead of human-readable summary |

### Default output (human-readable)

```
agreement_hash <64-char hex>
policy_id pol-001
tip_height 1500
release_eligible true
refund_eligible false
reason all release requirements satisfied by verified proofs
evaluated_rules
  proof 'prf-001' verified ok
```

### Exit codes

- `0`: at least one of `release_eligible` or `refund_eligible` is true.
- `1`: neither flag is true, or the RPC call failed, or input parsing failed.

### RPC token

If `IRIUM_RPC_TOKEN` is set in the environment, it is sent as `Authorization: Bearer`
on the request. If the node requires authentication, set this variable.

---

---

## POST /rpc/submitproof

Submits a settlement proof for storage at the node. The node validates the proof's
cryptographic signature and stores it if valid. Attestor authorization (policy
membership) is **not** checked at submission time — policy evaluation happens at
`/rpc/checkpolicy`.

### Authentication

Requires `Authorization: Bearer <IRIUM_RPC_TOKEN>`. Rate-limited.

### Request

```
POST /rpc/submitproof
Content-Type: application/json
Authorization: Bearer <token>
```

```json
{
  "proof": { ... }
}
```

- `proof`: a full `SettlementProof` object (see SettlementProof JSON shape above).

### Response (200 OK)

```json
{
  "proof_id": "prf-001",
  "agreement_hash": "<64-char hex>",
  "accepted": true,
  "duplicate": false,
  "message": "proof accepted",
  "tip_height": 1042,
  "expires_at_height": 2000,
  "expired": false,
  "status": "active"
}
```

- `accepted`: true if the proof was newly stored.
- `duplicate`: true if a proof with the same `proof_id` was already present.
- If `accepted` is false and `duplicate` is false, an error occurred (returned as 400).
- `tip_height`: chain tip height at the moment the proof was submitted.
- `expires_at_height`: value of `expires_at_height` from the submitted proof, or `null` if the proof has no expiry.
- `expired`: `true` when `tip_height >= expires_at_height` at submit time; always `false` when `expires_at_height` is `null`. Expiry does **not** affect acceptance — an expired proof is still stored.
- `status`: `"active"` or `"expired"`. Consistent with per-proof `status` in `/rpc/listproofs`. Derived from `expired`: if `expired` is true then `status` is `"expired"`, otherwise `"active"`.

### Error responses

- `400 Bad Request` — proof schema_id mismatch, invalid signature, or empty
  `proof_id`/`agreement_hash`.
- `401 Unauthorized` — missing or invalid RPC token.
- `429 Too Many Requests` — rate limit exceeded.

### Storage semantics

- Proofs are keyed by `proof_id`. Submitting the same `proof_id` twice returns
  `duplicate: true` without modifying stored state.
- Proofs are persisted to `<state_dir>/proofs.json` immediately on acceptance.
- The node does not evict proofs. Storage is unbounded.
- Proofs are not validated against any agreement or policy at submission time.
  Use `/rpc/checkpolicy` with `proofs` from `/rpc/listproofs` to evaluate eligibility.

---

## POST /rpc/getproof

Retrieves a single settlement proof by its `proof_id`.

### Authentication

Requires `Authorization: Bearer <IRIUM_RPC_TOKEN>`. Rate-limited.

### Request

```json
{
  "proof_id": "prf-001"
}
```

### Response (200 OK)

```json
{
  "proof_id": "prf-001",
  "found": true,
  "tip_height": 19200,
  "proof": { "proof_id": "prf-001", "agreement_hash": "...", "expires_at_height": null, "..." : "..." },
  "expires_at_height": null,
  "expired": false,
  "status": "active"
}
```

When not found:

```json
{
  "proof_id": "prf-unknown",
  "found": false,
  "tip_height": 19200,
  "proof": null,
  "expires_at_height": null,
  "expired": false,
  "status": ""
}
```

### Fields

- `found`: `true` if the proof exists in the store; `false` if not found.
- `proof`: full `SettlementProof` object when `found=true`; `null` otherwise.
- `tip_height`: chain tip height at query time.
- `expires_at_height`: echoed from the proof; `null` if no expiry or `found=false`.
- `expired`: `true` when `tip_height >= expires_at_height`; always `false` when `expires_at_height` is null or `found=false`.
- `status`: `"active"` or `"expired"` when `found=true`, consistent with per-proof `status` in `/rpc/listproofs`. Empty string when `found=false`.
- Invariant: `(status == "expired") == expired` always holds.

### Error responses

- `401 Unauthorized` — missing or invalid RPC token.
- `429 Too Many Requests` — rate limit exceeded.
- Not found is signalled by `found: false` in the response body (not a 404).

---

## POST /rpc/listproofs

Returns all stored proofs for a given agreement hash.

**Ordering guarantee:** Proofs are always returned sorted by `attestation_time` ascending,
with `proof_id` ascending as a stable tie-breaker. This rule applies to global queries
(`agreement_hash` omitted), agreement-scoped queries, and `active_only` filtered queries.

### Authentication

Requires `Authorization: Bearer <IRIUM_RPC_TOKEN>`. Rate-limited.

### Request

```json
{
  "agreement_hash": "<64-char hex>",
  "active_only": false,
  "offset": 0,
  "limit": 50
}
```

### Response (200 OK)

```json
{
  "agreement_hash": "<64-char hex>",
  "tip_height": 19200,
  "active_only": false,
  "total_count": 25,
  "returned_count": 5,
  "has_more": true,
  "offset": 10,
  "limit": 5,
  "proofs": [
    { "proof_id": "prf-011", "expires_at_height": null, "status": "active", "..." : "..." }
  ]
}
```

- `tip_height`: chain tip height at query time.
- `active_only`: echoes the request filter.
- `total_count`: total proofs matching all filters before pagination. Equals `returned_count`
  when no pagination is applied.
- `returned_count`: number of proofs returned in this page. Always equals `proofs.len()`.
- `has_more`: `true` when more proofs remain after this page
  (`total_count > offset + returned_count`); `false` on the last page or when all results
  fit in one page. Clients should use this field to drive cursor-style pagination.
- `offset`: echoed from the request (0 when omitted).
- `limit`: echoed from the request (`null` when omitted).
- `proofs`: array of proof objects sorted **by `attestation_time` ascending, then `proof_id`
  ascending** as a stable tie-breaker. This order is consistent across global queries,
  agreement-scoped queries, and `active_only` filtered queries. Each proof includes:
  - All `SettlementProof` fields.
  - `status`: `"active"` if the proof is not expired at `tip_height`; `"expired"` if `tip_height >= expires_at_height`.
- Returns an empty `proofs` array and `returned_count: 0` if offset exceeds available proofs.
  `has_more` will be `false` in this case.
- `limit` and `offset` are optional. When omitted, all matching proofs are returned from offset 0
  and `has_more` will always be `false`.

---

## irium-wallet agreement-proof-submit

```
irium-wallet agreement-proof-submit \
  --proof <proof.json|-> \
  [--rpc <url>] \
  [--json]
```

### Arguments

| Flag | Required | Description |
|---|---|---|
| `--proof <path\|->` | yes | Path to settlement proof JSON, or `-` for stdin |
| `--rpc <url>` | no | Node RPC base URL |
| `--json` | no | Output raw JSON response |

### Default output

```
proof_id prf-001
agreement_hash <64-char hex>
accepted true
duplicate false
message proof accepted
tip_height 1042
expires_at_height 2000
expired false
status active
```

`expires_at_height` is shown as `none` when the proof carries no expiry. `expired` reflects
whether `tip_height >= expires_at_height` at submit time; an expired proof is still accepted
for storage. `status` shows `active` or `expired` and is consistent with the `status` field
in `agreement-proof-list` output. `status` is omitted if the node does not return it (older nodes).

### Exit codes

- `0`: proof was accepted or duplicate.
- `1`: proof was rejected, RPC call failed, or input parsing failed.

---

## irium-wallet agreement-proof-get

Retrieves a single settlement proof by `proof_id` from the node.

```
irium-wallet agreement-proof-get \n  --proof-id <id> \n  [--rpc <url>] \n  [--json]
```

### Arguments

| Flag | Required | Description |
|---|---|---|
| `--proof-id <id>` | yes | The `proof_id` of the proof to retrieve |
| `--rpc <url>` | no | Node RPC base URL |
| `--json` | no | Output raw JSON response |

### Default output (found)

```
proof_id prf-001
found true
tip_height 19200
agreement_hash <64-char hex>
proof_type delivery_confirmation
attested_by attestor-a
expires_at_height none
expired false
status active
```

### Default output (not found)

```
proof_id prf-unknown
not_found true
```

### Exit codes

- `0`: proof was found.
- `1`: proof not found, RPC call failed, or input parsing failed.

---

## irium-wallet agreement-proof-list

Lists settlement proofs stored on the node. When `--agreement-hash` is omitted all
proofs in the store are returned (global listing). When provided, only proofs for
that agreement are returned. `--active-only` excludes proofs that have expired at
the current chain tip.

```
irium-wallet agreement-proof-list \
  [--agreement-hash <hex>] \
  [--active-only] \
  [--rpc <url>] \
  [--json]
```

### Arguments

| Flag | Required | Description |
|---|---|---|
| `--agreement-hash <hex>` | no | SHA-256 hex of the agreement to filter by. Omit to list all proofs. |
| `--active-only` | no | Return only proofs that are not expired at the current chain tip. |
| `--rpc <url>` | no | Node RPC base URL |
| `--json` | no | Output raw JSON response |

### Default output (filtered)

```
agreement_hash <64-char hex>
count 1
  agreement_hash=<64-char hex> proof_id=prf-001 attested_by=attestor-a proof_type=delivery_confirmation expires_at_height=none status=active
```

### Default output (global — no filter)

```
agreement_hash * (all)
count 3
  agreement_hash=<hex-a> proof_id=prf-001 attested_by=attestor-a proof_type=delivery_confirmation expires_at_height=none status=active
  agreement_hash=<hex-b> proof_id=prf-002 attested_by=attestor-b proof_type=payment expires_at_height=5000 expired=false status=active
  agreement_hash=<hex-c> proof_id=prf-003 attested_by=attestor-c proof_type=milestone expires_at_height=100 expired=true status=expired
```

### With `--active-only`

```
filter active_only true
agreement_hash * (all)
count 2
  agreement_hash=<hex-a> proof_id=prf-001 attested_by=attestor-a proof_type=delivery_confirmation expires_at_height=none
  agreement_hash=<hex-b> proof_id=prf-002 attested_by=attestor-b proof_type=payment expires_at_height=5000 expired=false
```

Proofs with `tip_height >= expires_at_height` are omitted. The `filter active_only true` header
confirms the filter is active. `--active-only` and `--agreement-hash` may be combined.

### RPC body

Sends `POST /rpc/listproofs` with body:
- `{}` — all proofs, no filter
- `{ "agreement_hash": "<hex>" }` — filter by agreement
- `{ "active_only": true }` — only non-expired proofs
- `{ "agreement_hash": "<hex>", "active_only": true }` — both filters combined

The response includes `"active_only": true/false` echoing the filter.

### Exit codes

- `0` always (listing an empty set is not an error).




## irium-wallet agreement-proof-create

Creates and signs a `SettlementProof` using a key from the local wallet. The output
is a signed proof JSON ready to submit via `agreement-proof-submit`.

```
irium-wallet agreement-proof-create \
  --agreement-hash <hex> \
  --proof-type <string> \
  --attested-by <attestor-id> \
  --address <wallet-address> \
  [--expires-at-height <n>] \
  [--milestone-id <id>] \
  [--evidence-summary <text>] \
  [--evidence-hash <hex>] \
  [--proof-id <id>] \
  [--timestamp <unix-seconds>] \
  [--out <path>] \
  [--json]
```

### Arguments

| Flag | Required | Description |
|---|---|---|
| `--agreement-hash <hex>` | yes | SHA-256 hex of the agreement this proof attests to |
| `--proof-type <string>` | yes | Proof type label matching a `ProofRequirement.proof_type` in the policy |
| `--attested-by <id>` | yes | Attestor ID to embed in the proof; must match an entry in the policy `attestors` list |
| `--address <addr>` | yes | Wallet address whose private key signs the proof |
| `--expires-at-height <n>` | no | Block height at which this proof becomes inactive for stored evaluation. Omit for no expiry. |
| `--milestone-id <id>` | no | Milestone scope for milestone-specific proofs |
| `--evidence-summary <text>` | no | Free-text description of the supporting evidence |
| `--evidence-hash <hex>` | no | Hex hash of an external evidence artifact |
| `--proof-id <id>` | no | Explicit proof ID. Defaults to `prf-<16-char hex>` derived from proof_type, agreement_hash, and timestamp |
| `--timestamp <unix>` | no | Attestation time as Unix seconds. Defaults to current time |
| `--out <path>` | no | Write the proof JSON to this file path in addition to stdout |
| `--json` | no | Also print the full proof JSON to stdout (always printed when `--out` is not given) |

### Signing flow

1. The wallet key for `--address` is loaded from the local wallet file
   (`IRIUM_WALLET_FILE` env var or `~/.irium/wallet.json`).
2. The proof payload is computed by `settlement_proof_payload_bytes`: a canonical
   JSON of all proof fields except the `signature` envelope, sorted lexicographically
   and SHA-256 hashed.
3. The 32-byte digest is signed with `secp256k1_ecdsa_sha256` using the wallet key.
4. The resulting `SettlementProof` JSON is printed to stdout (and written to `--out`
   if specified).

### Default output (when `--out` is given without `--json`)

```
proof_id prf-<16-char hex>
schema_id irium.phase2.settlement_proof.v1
proof_type delivery_confirmation
agreement_hash <64-char hex>
attested_by attestor-a
attestation_time 1700000000
expires_at_height none
payload_hash <64-char hex>
pubkey_hex <compressed secp256k1 public key hex>
```

### Output when `--out` is not given

The full proof JSON is printed to stdout regardless of `--json`. This allows piping:

```
irium-wallet agreement-proof-create \
  --agreement-hash <hex> --proof-type delivery_confirmation \
  --attested-by attestor-a --address <addr> > proof.json
```

### Notes on pubkey_hex

The wallet stores compressed secp256k1 keys (33 bytes, 66 hex chars). The
`pubkey_hex` in the generated proof uses this compressed format. The node
verification code (`VerifyingKey::from_sec1_bytes`) accepts both compressed
and uncompressed keys, so no conversion is needed. However, the `ApprovedAttestor`
entry in the `ProofPolicy` must use the same `pubkey_hex` value as the generated
proof for verification to succeed.

### Typical workflow

```sh
# 1. Get the wallet address that will act as attestor
irium-wallet list-addresses

# 2. Create the proof
irium-wallet agreement-proof-create \
  --agreement-hash <64-char hex> \
  --proof-type delivery_confirmation \
  --attested-by attestor-a \
  --address <addr> \
  --evidence-summary "Goods delivered and signed for." \
  --out proof.json

# 3. Submit to the node
irium-wallet agreement-proof-submit --proof proof.json

# 4. Check policy eligibility
irium-wallet agreement-policy-check \
  --agreement agreement.json \
  --policy policy.json \
  --proof proof.json
```

### Exit codes

- `0`: proof was created and written successfully.
- `1`: wallet key not found, signing failed, serialization failed, or file write error.


## agreement-policy-set

Stores a `ProofPolicy` on the node, associating it with the `agreement_hash` embedded
in the policy. Each `agreement_hash` holds at most one active policy. Attempting to store
a different `policy_id` for the same hash is rejected unless `--replace` is given.

```
irium-wallet agreement-policy-set \
  --policy <policy.json|-> \
  [--rpc <url>] \
  [--json]
```

| Flag | Required | Description |
|---|---|---|
| `--policy <path\|->` | yes | Path to a `ProofPolicy` JSON file, or `-` to read from stdin |
| `--rpc <url>` | no | Node RPC base URL. Defaults to `IRIUM_RPC_URL` or `http://127.0.0.1:38300` |
| `--json` | no | Print the full response JSON to stdout |
| `--replace` | no | Overwrite an existing policy for the same agreement hash |
| `--expires-at-height <n>` | no | Block height at which this policy expires. Omit for no expiry. |

### Storage behavior

The node persists all policies to `$IRIUM_DATA_DIR/state/policies.json` (default
`~/.irium/state/policies.json`). One policy is stored per `agreement_hash`. The four possible outcomes are:

| Scenario | result |
|---|---|
| Fresh `agreement_hash` | accepted (`status accepted`) |
| Same `policy_id` again | silent no-op (`status rejected`, no change) |
| Different `policy_id`, no `--replace` | rejected (`status rejected`) with message naming the existing policy |
| Different `policy_id` with `--replace` | overwrites previous (`status replaced`) |

### Default output

```
policy_id pol-<id>
agreement_hash <64-char hex>
status accepted
```

### Node RPC

`POST /rpc/storepolicy` — body: `{ "policy": <ProofPolicy>, "replace": false }`.
Response: `{ policy_id, agreement_hash, accepted, updated, message }`.

### Policy expiry

A policy may carry an optional `expires_at_height` field (a `u64` block height).

- If `expires_at_height` is absent or `null`, the policy never expires.
- If `tip_height >= expires_at_height`, the policy is considered expired.
- Expiry is evaluated at query time — no automatic deletion occurs.
- `/rpc/evaluatepolicy` returns `expired: true` and skips proof evaluation for expired policies.
- `/rpc/checkpolicy` (manual check) does **not** enforce expiry; the caller supplies the policy directly.
- `/rpc/getpolicy` and `/rpc/listpolicies` include `expires_at_height` and `expired` in their responses.

Pass `--expires-at-height <n>` to `agreement-policy-set` to set the expiry height when storing.

---

## Proof expiry

A stored proof may carry an optional `expires_at_height` field (a `u64` block height).

- If `expires_at_height` is absent or `null`, the proof never expires.
- If `tip_height >= expires_at_height`, the proof is treated as inactive for stored evaluation.
- Expiry is evaluated at query time — no automatic deletion of expired proofs occurs.
- `/rpc/evaluatepolicy` skips expired stored proofs; each skipped proof is noted in
  `evaluated_rules` with the message `"proof '<id>' skipped: expired at height <H> (tip <T)"`.
- `/rpc/checkpolicy` (manual check) does **not** enforce proof expiry; the caller supplies
  proofs directly and is responsible for filtering.
- `/rpc/listproofs` includes `expires_at_height` per proof and a top-level `tip_height`.
  Each proof entry also carries a derived `status` field: `"active"` when not expired,
  `"expired"` when `tip_height >= expires_at_height`.
- The `irium-wallet agreement-proof-list` output shows `expires_at_height=<N> expired=<bool> status=<active|expired>`
  (or `expires_at_height=none`) per proof line.

Pass `--expires-at-height <n>` to `agreement-proof-create` to set the expiry height when
creating a proof. The field is stored as plain metadata and is **not** included in the
signature payload — the signature covers only the proof content fields, not its TTL.

---

## agreement-policy-get

Retrieves the stored `ProofPolicy` for a given agreement hash from the node.

```
irium-wallet agreement-policy-get \
  --agreement-hash <hex> \
  [--rpc <url>] \
  [--json]
```

| Flag | Required | Description |
|---|---|---|
| `--agreement-hash <hex>` | yes | SHA-256 hex of the agreement whose policy to fetch |
| `--rpc <url>` | no | Node RPC base URL |
| `--json` | no | Print the full response JSON (including the policy object) to stdout |

### Default output (when found)

```
policy_id pol-<id>
agreement_hash <64-char hex>
required_proofs <count>
attestors <count>
found true
```

Exits with code `1` when no policy is stored for the requested hash.

### Node RPC

`POST /rpc/getpolicy` — body: `{ "agreement_hash": "<hex>" }`.
Response: `{ agreement_hash, found, policy }` where `policy` is `null` when not found.

---


## agreement-policy-evaluate

Evaluates an agreement against its stored policy and stored proofs using a single
command, without supplying policy JSON or proof JSON manually. The node looks up both
artifacts by the `agreement_hash` derived from the supplied agreement.

```
irium-wallet agreement-policy-evaluate \
  --agreement <agreement.json|-> \
  [--rpc <url>] \
  [--json]
```

| Flag | Required | Description |
|---|---|---|
| `--agreement <path\|->` | yes | Path to an `AgreementObject` JSON file, or `-` to read from stdin |
| `--rpc <url>` | no | Node RPC base URL. Defaults to `IRIUM_RPC_URL` or `http://127.0.0.1:38300` |
| `--json` | no | Print the full response JSON to stdout |

### Evaluation flow

1. The wallet sends the agreement object to `/rpc/evaluatepolicy`.
2. The node computes `agreement_hash` from the object.
3. The node fetches the stored `ProofPolicy` for that hash. If none is found, it
   returns `policy_found: false` with `release_eligible: false`.
4. The node fetches all stored `SettlementProof` entries for that hash.
5. The node runs `evaluate_policy(agreement, stored_policy, stored_proofs, tip_height)`
   and returns the result.

The agreement-hash binding check introduced in `e2853a3` is enforced at the store
lookup level: proofs stored for a different agreement hash are never fetched.

### Default output (policy found, eligible)

```
agreement_hash <64-char hex>
policy_id pol-<id>
policy_found true
tip_height <n>
proof_count <n>
matched_proof_count <n>
matched_proof_ids <id1>, <id2>
release_eligible true
refund_eligible false
reason all release requirements satisfied by verified proofs
evaluated_rules
  proof '<id>' verified ok
```

### Default output (policy not found)

```
agreement_hash <64-char hex>
policy_id none
policy_found false
tip_height <n>
proof_count 0
release_eligible false
refund_eligible false
reason no policy stored for this agreement
```

Exits with code `1` when neither `release_eligible` nor `refund_eligible` is true
(covers both not-found and not-satisfied cases).

### Node RPC

`POST /rpc/evaluatepolicy` — body: `{ "agreement": <AgreementObject> }`.
Response:
```json
{
  "outcome": "satisfied",
  "agreement_hash": "<hex>",
  "policy_found": true,
  "policy_id": "<id>",
  "tip_height": <n>,
  "proof_count": <n>,
  "expired_proof_count": <n>,
  "matched_proof_count": <n>,
  "matched_proof_ids": ["<id>"],
  "expired": false,
  "release_eligible": true,
  "refund_eligible": false,
  "reason": "<string>",
  "evaluated_rules": ["..."],
  "milestone_results": [
    {
      "milestone_id": "<id>",
      "label": "<string or null>",
      "outcome": "satisfied",
      "release_eligible": true,
      "refund_eligible": false,
      "matched_proof_ids": ["<id>"],
      "reason": "<string>"
    }
  ],
  "completed_milestone_count": <n>,
  "total_milestone_count": <n>,
  "holdback": {
    "holdback_present": true,
    "holdback_released": false,
    "holdback_bps": 1000,
    "immediate_release_bps": 9000,
    "holdback_outcome": "held",
    "holdback_reason": "<string>"
  },
  "threshold_results": [
    {
      "requirement_id": "<id>",
      "threshold_required": <n>,
      "approved_attestor_count": <n>,
      "matched_attestor_ids": ["<id>"],
      "threshold_satisfied": true
    }
  ]
}
```
`policy_id` is `null` when no policy is stored.
`milestone_results` is an empty array when no milestones are declared.
`holdback` is `null` when no holdback is configured; present only when the base
condition is `satisfied`.
`threshold_results` is an empty array when no requirements have an explicit `threshold` field.

#### `outcome` field

The `outcome` field provides a single deterministic classification of each evaluation.
It is objective — derived purely from proof signatures, policy rules, and `tip_height`.

| Value | Meaning |
|---|---|
| `"satisfied"` | All required proofs are present and signature-verified. `release_eligible` will be `true`. |
| `"timeout"` | A `no_response_rule` deadline or a refund `required_by` deadline has elapsed before release was achieved. Proofs are missing or insufficient. |
| `"unsatisfied"` | Neither condition is met — proofs are missing, expired, or signature-invalid, and no deadline has elapsed. |

**Rules:**
- `"satisfied"` is returned in Step 2 of evaluation, before any deadline check. If proofs satisfy release, no-response rules are suppressed entirely.
- `"timeout"` fires when either a `no_response_rule.deadline_height` is reached (Step 3) or a refund requirement's `required_by` deadline is reached (Step 4).
- `"unsatisfied"` is the fallthrough when no condition has triggered (Step 5).
- A policy with no stored policy returns `"unsatisfied"` (`policy_found: false`).
- An expired policy returns `"unsatisfied"` (`expired: true`).

**Examples:**

```json
// Proof submitted and verified:
{ "outcome": "satisfied", "release_eligible": true, "refund_eligible": false }

// No-response deadline elapsed, no proof submitted:
{ "outcome": "timeout", "release_eligible": false, "refund_eligible": true }

// No proof submitted, no deadline elapsed:
{ "outcome": "unsatisfied", "release_eligible": false, "refund_eligible": false }

// Only expired proofs remain (active proof_count = 0), no deadline:
{ "outcome": "unsatisfied", "proof_count": 0, "expired_proof_count": 1 }
```

#### Milestone / tranche-based evaluation

A `ProofPolicy` may declare one or more **milestones** via the `milestones` array.
When milestones are declared:

- Each milestone is identified by a `milestone_id` string.
- `ProofRequirement` entries with a matching `milestone_id` field belong to that milestone.
- `NoResponseRule` entries with a matching `milestone_id` field belong to that milestone.
- Each milestone is evaluated **independently** using the same 5-step logic.
- The overall `outcome` is the **aggregate** of all milestone outcomes:
  - All milestones `satisfied` → overall `"satisfied"`.
  - Any milestone `timeout` (and not all satisfied) → overall `"timeout"`.
  - Otherwise → overall `"unsatisfied"`.
- `completed_milestone_count` and `total_milestone_count` track partial progress.

**Backward compatibility:** policies without a `milestones` array are evaluated
using the traditional flat-requirements path. `milestone_results` will be `[]` and
both counts will be `0`.

#### Approved-attestor threshold

A `ProofRequirement` may declare an optional `threshold` field (integer >= 1) requiring
that at least that many **distinct** approved attestors submit matching verified proofs
before the requirement is considered satisfied.

| Field | Type | Description |
|---|---|---|
| `threshold` | integer\|null | Minimum distinct approved attestors needed. Defaults to 1 (absent = single-attestor). |

**Semantics:**
- Only attestors in `required_attestor_ids` count toward the threshold.
- Multiple proofs from the same attestor count as **1** distinct attestor.
- Expired proofs (filtered at RPC layer) do not count.
- Unapproved attestors (not in `policy.attestors`) are rejected at signature-verification and do not count.
- `threshold: null` or absent behaves identically to `threshold: 1`.

**Output:** `threshold_results` array in the evaluate response, one entry per requirement
with an explicit `threshold`. Empty when no threshold requirements exist.

**Example — 2-of-3 delivery confirmation:**

```json
{
  "requirement_id": "req-delivery",
  "proof_type": "delivery_confirmation",
  "required_attestor_ids": ["att-1", "att-2", "att-3"],
  "resolution": "release",
  "threshold": 2
}
```

After att-1 and att-2 each submit a verified proof, the requirement is satisfied.
The response includes:

```json
"threshold_results": [{
  "requirement_id": "req-delivery",
  "threshold_required": 2,
  "approved_attestor_count": 2,
  "matched_attestor_ids": ["att-1", "att-2"],
  "threshold_satisfied": true
}]
```

**Validation at store time:**
- `threshold` must be >= 1.
- `threshold` must not exceed `required_attestor_ids.len()`.
- `required_attestor_ids` must be non-empty when `threshold` is set.

#### Holdback / retention release

A `ProofPolicy` or `PolicyMilestone` may declare an optional `holdback` that retains
a portion of the settlement amount until a secondary condition is met.

| Field | Type | Description |
|---|---|---|
| `holdback_bps` | integer | Basis points to hold back (1–9999). |
| `release_requirement_id` | string\|null | ID of a `ProofRequirement` whose satisfaction releases the holdback. |
| `deadline_height` | integer\|null | Block height at or after which the holdback auto-releases. |

At least one of `release_requirement_id` or `deadline_height` must be supplied.
Proof-condition release takes priority over deadline release.

**Holdback outcome vocabulary**

| Value | Meaning |
|---|---|
| `pending` | Base condition not yet satisfied; holdback not yet active. |
| `held` | Base satisfied; holdback condition not yet met. |
| `released` | Holdback released (by proof or deadline). |

`immediate_release_bps` is `10000 - holdback_bps` when `held`, and `10000` when `released`.

**Scope rules:**
- `policy.holdback` applies in the non-milestone evaluation path only.
- `milestone.holdback` applies per-milestone within the milestone evaluation path.
- When holdback is absent, `holdback` in the response is `null`.

**Example policy with two milestones:**

```json
{
  "policy_id": "pol-tranche",
  "agreement_hash": "<hex>",
  "required_proofs": [
    { "requirement_id": "req-ms-a", "proof_type": "delivery_confirmation",
      "required_attestor_ids": ["att-1"], "resolution": "milestone_release",
      "milestone_id": "ms-a" },
    { "requirement_id": "req-ms-b", "proof_type": "inspection_report",
      "required_attestor_ids": ["att-1"], "resolution": "milestone_release",
      "milestone_id": "ms-b" }
  ],
  "milestones": [
    { "milestone_id": "ms-a", "label": "Delivery" },
    { "milestone_id": "ms-b", "label": "Inspection" }
  ],
  "attestors": [{ "attestor_id": "att-1", "pubkey_hex": "<hex>" }]
}
```

**Partial completion response (ms-a done, ms-b pending):**

```json
{ "outcome": "unsatisfied", "completed_milestone_count": 1, "total_milestone_count": 2,
  "milestone_results": [
    { "milestone_id": "ms-a", "label": "Delivery", "outcome": "satisfied", "release_eligible": true },
    { "milestone_id": "ms-b", "label": "Inspection", "outcome": "unsatisfied", "release_eligible": false }
  ]
}
```

### Relationship to agreement-policy-check

`agreement-policy-check` accepts explicit policy and proof JSON and is the right tool
when the operator wants to evaluate hypothetical or offline-constructed artifacts.
`agreement-policy-evaluate` is the convenience path for on-node artifacts — it uses
whatever the node has persisted, nothing more.

---


## agreement-policy-list

Lists all stored policies on the node. Useful for operators who need to discover
what policies are registered without knowing specific agreement hashes.

```
irium-wallet agreement-policy-list \
  [--active-only] \
  [--rpc <url>] \
  [--json]
```

| Flag | Required | Description |
|---|---|---|
| `--active-only` | no | Return only policies that are not expired at the current tip height |
| `--rpc <url>` | no | Node RPC base URL. Defaults to `IRIUM_RPC_URL` or `http://127.0.0.1:38300` |
| `--json` | no | Print the full response JSON to stdout |

### Default output

Without `--active-only`:

```
count <n>
  agreement_hash <hex> policy_id <id> required_proofs <n> attestors <n> expires_at_height none
  agreement_hash <hex> policy_id <id> required_proofs <n> attestors <n> expires_at_height <N> expired false
  agreement_hash <hex> policy_id <id> required_proofs <n> attestors <n> expires_at_height <N> expired true
  ...
```

With `--active-only`:

```
filter active_only true
count <n>
  agreement_hash <hex> policy_id <id> required_proofs <n> attestors <n> expires_at_height none
  agreement_hash <hex> policy_id <id> required_proofs <n> attestors <n> expires_at_height <N> expired false
  ...
```

Expired policies are omitted when `--active-only` is set. The `filter active_only true` header
is printed so operators can distinguish a filtered result from an empty store.
Use `agreement-policy-get --agreement-hash <hex>` to retrieve the full policy JSON.

### Node RPC

`POST /rpc/listpolicies` — body: `{ "active_only": false }`.
Set `"active_only": true` to filter out expired policies.
Response:
```json
{
  "count": 2,
  "active_only": false,
  "policies": [
    {
      "agreement_hash": "<hex>",
      "policy_id": "<id>",
      "required_proofs": 1,
      "attestors": 1,
      "expires_at_height": null,
      "expired": false
    }
  ]
}
```

---


## Evaluation semantics

Policy evaluation (`evaluate_policy` / `/rpc/evaluatepolicy` / `agreement-policy-check`)
follows a deterministic, ordered sequence:

### 1. Agreement-hash binding

The `agreement_hash` in the stored policy must match the hash derived from the
supplied `AgreementObject`. If it does not, evaluation fails immediately with an error.

### 2. Proof verification

All supplied proofs are verified before any deadline or rule checks:

- Proofs whose `agreement_hash` does not match the policy's hash are rejected with a
  mismatch note in `evaluated_rules`.
- Proofs that fail signature or attestor-approval checks are rejected with the rejection
  reason in `evaluated_rules`.
- Proofs that pass both checks are added to the satisfied set.

### 3. Release requirements

If **all** release requirements (`resolution: release` or `milestone_release`) are satisfied
by verified proofs, the result is `release_eligible: true`. No-response rules are suppressed
when release is already achieved — a `funded_and_no_release` rule will not override a valid
release.

### 4. No-response rules

If release has not been achieved, no-response rules are evaluated in order. The first rule
whose `deadline_height <= tip_height` fires immediately and determines the result.

- `funded_and_no_release` — fires when the deadline is reached and release has not been
  granted. In Phase 2 this is the only trigger condition checked.
- `disputed_and_no_response` — treated identically to `funded_and_no_release` in Phase 2
  (fires at deadline when release is not met).

The trigger label appears in `evaluated_rules` for observability.

### 5. Refund-requirement deadlines (`required_by`)

For each proof requirement with `resolution: refund` and a `required_by` height set,
if `tip_height >= required_by` and no verified proof satisfies the requirement, the result
is `refund_eligible: true`.

If the refund requirement **is** satisfied by a verified proof (even past its deadline),
the result is recorded in `evaluated_rules` but does not trigger a refund.

### 6. Release-requirement deadline recording

If a release requirement has `required_by` set and its deadline has passed with no
satisfying proof, this is recorded in `evaluated_rules` as a missed deadline. It does not
prevent release if the proof later arrives — `required_by` on a release requirement is an
informational deadline, not a hard acceptance cutoff.

### Summary table

| Condition | Result |
|---|---|
| All release requirements satisfied by verified proofs | `release_eligible: true` |
| No-response rule fires (release not met, deadline passed) | rule resolution applied |
| Refund requirement `required_by` passed, no proof | `refund_eligible: true` |
| Refund requirement `required_by` passed, proof present | `evaluated_rules` note only |
| Release requirement `required_by` passed, proof present | `release_eligible: true` |
| None of the above | not eligible |

---

## Current limitations

The following items are defined in the type layer but not yet evaluated or exposed:

- **Proof-triggered refunds**: `ProofRequirement.resolution = "refund"` is stored and
  serialized but `evaluate_policy` does not process it. Refund eligibility is only
  reachable via `no_response_rules`.
- **`required_by` deadline on ProofRequirement**: the field exists and round-trips
  through JSON but is not checked during evaluation.
- **Milestone scoping**: `milestone_id` on requirements and rules is stored but
  evaluation does not filter or scope by milestone.
- **Policy persistence**: implemented via `PolicyStore` (`state/policies.json`) / `/rpc/storepolicy` / `/rpc/getpolicy` / `/rpc/evaluatepolicy` / `/rpc/listpolicies`.
- **Proof persistence**: implemented via `ProofStore` (`state/proofs.json`) / `/rpc/submitproof` / `/rpc/listproofs`.
- **Attestor registry**: there is no persistent on-node attestor registry. Attestors
  are defined inline in each `ProofPolicy` object.
- **Explorer routes**: no `/agreement/policy*` routes exist in `irium-explorer` yet.
- **`AGREEMENT_SCHEMA_ID_V2`**: the constant `"irium.phase2.canonical.v1"` is defined
  but no code validates it against incoming agreement objects. Phase 2 agreements
  are validated by the existing Phase 1 `AgreementObject::validate()` path.

---

## Application-layer note

Phase 2 policy evaluation is entirely off-chain application logic. The node reads the
current chain tip height for deadline evaluation but does not write any state, submit
transactions, or alter consensus rules. No blocks are produced or validated differently
as a result of any Phase 2 operation. Phase 1 HTLC spend eligibility (secret preimage,
timeout refund) is unchanged.

---

## Typed proof payload (structured proof objects)

### Overview

`SettlementProof` carries an optional `typed_payload` field that allows provers to attach
structured, normalized metadata to any proof without affecting its cryptographic signature.
This is a pure application-layer extension — existing proofs without the field deserialize
correctly (`typed_payload` defaults to `None`), and the field is intentionally excluded from
`settlement_proof_payload_bytes()` so that all previously-signed proofs remain valid.

### Schema

```json
{
  "proof_kind": "delivery_confirmation",   // required, non-empty string
  "content_hash": "aabbcc...(64 hex)",     // optional, must be 64 lowercase hex chars
  "reference_id": "TRK-12345",             // optional, free-form reference string
  "attributes": { "carrier": "DHL" }       // optional, JSON object only
}
```

| Field | Type | Required | Constraints |
|---|---|---|---|
| `proof_kind` | string | yes | non-empty after trim |
| `content_hash` | string | no | exactly 64 lowercase hex chars; if set alongside `evidence_hash`, must match |
| `reference_id` | string | no | free-form, any UTF-8 string |
| `attributes` | object | no | must be a JSON object (not array/scalar) |

### Backward compatibility

- Proofs stored without `typed_payload` deserialize with `typed_payload = None` — no migration needed.
- `settlement_proof_payload_bytes()` does **not** include `typed_payload` in the signed payload.
  Adding or removing this field after signing does not invalidate the signature.
- Policy evaluation (`evaluate_policy`) ignores `typed_payload` entirely — matching is done on
  `proof_type`, `attested_by`, and `milestone_id` as before.

### Validation (at submit time)

`ProofStore::submit()` calls `validate_typed_proof_payload()` before signature verification
when `typed_payload` is `Some`. The following are rejected with a `BAD_REQUEST` response:

- `proof_kind` is empty or whitespace-only
- `content_hash` is not exactly 64 lowercase hex characters
- `content_hash` is set alongside `evidence_hash` but they do not match
- `attributes` is present but is not a JSON object

### CLI usage (irium-wallet)

```sh
irium-wallet agreement-proof-create \
  --agreement-hash <hex> \
  --proof-type delivery_confirmation \
  --attested-by my-attestor \
  --address irium1abc... \
  --proof-kind delivery_confirmation \
  --reference-id TRK-99887766
```

The `--proof-kind` and `--reference-id` flags populate `typed_payload.proof_kind` and
`typed_payload.reference_id` respectively. `content_hash` and `attributes` can be set
programmatically via the JSON submission path (`/rpc/submitproof`).

### Display

- `agreement-proof-get` renders `proof_kind` and `reference_id` lines when present.
- `agreement-proof-list` appends ` proof_kind=<value>` to each proof line when present.
- JSON output from `/rpc/listproofs` and `/rpc/getproof` includes the full `typed_payload`
  object (serialized inline on `SettlementProof` via `#[serde(default)]`).

### Example flow

```sh
# Submit a typed proof
irium-wallet agreement-proof-create \
  --agreement-hash $(cat agreement.hash) \
  --proof-type delivery_confirmation \
  --attested-by logistics-oracle \
  --address irium1... \
  --proof-kind shipment_delivered \
  --reference-id BILL-OF-LADING-2026-001

# List proofs — typed proofs show proof_kind
irium-wallet agreement-proof-list --agreement-hash $(cat agreement.hash)
# => proof_id=prf-xxx  ...  proof_kind=shipment_delivered

# Get proof details
irium-wallet agreement-proof-get --proof-id prf-xxx
# =>  proof_kind shipment_delivered
# =>  reference_id BILL-OF-LADING-2026-001
```

### Security and trust boundary

**`typed_payload` is unsigned.** The fields `proof_kind`, `content_hash`, `reference_id`, and
`attributes` are NOT included in `settlement_proof_payload_bytes()` and are therefore NOT
covered by the attestor's cryptographic signature. This means:

- `proof_kind` cannot be used as attestation evidence. It is normalization metadata only.
- A proof where `proof_kind = "fraud_report"` but `proof_type = "delivery_confirmation"` is
  valid — the contradiction is permitted. Policy evaluation uses the **signed** `proof_type`.
- `reference_id` is an opaque external pointer; its correctness cannot be verified on-chain.
- `attributes` is arbitrary JSON; treat it as unverified supplementary information.

**Display markers:** The wallet CLI marks these fields as `[metadata]` in human-readable output
(e.g., `proof_kind shipment_delivered [metadata]`) to distinguish them from attested fields
like `proof_type` and `attested_by`.

**Invariant for future developers:** `typed_payload` fields must never be used in
`req_satisfied_threshold()`, `evaluate_holdback()`, or `evaluate_policy()` as matching criteria.
Doing so would allow unsigned data to influence security-sensitive release/refund decisions.
This constraint is enforced by a regression test (`typed_payload_proof_kind_contradiction_does_not_affect_policy`)
and by SAFETY INVARIANT comments in the source code.

**Signed fields** (any addition here requires a schema version bump and breaks all prior signatures):
`proof_id`, `schema_id`, `proof_type`, `agreement_hash`, `milestone_id`, `attested_by`,
`attestation_time`, `evidence_hash`, `evidence_summary`

**Excluded from signature** (safe to evolve without breaking existing proofs):
`expires_at_height`, `typed_payload` (and all its subfields)

---

## Commercial Policy Templates

Phase 2 ships three policy template builder functions in `settlement.rs` that let integrators
construct correct `ProofPolicy` objects using existing evaluation primitives without writing
policy JSON by hand. Templates enforce invariants, return descriptive errors on bad input, and
produce policies that are fully compatible with `evaluate_policy()`.

### `contractor_milestone_template`

**Use when:** a contractor must deliver multiple discrete work items, each with its own holdback
and optional deadline. A timeout rule fires a `Refund` if no proof arrives by the deadline.

**Signature:**
```rust
pub fn contractor_milestone_template(
    policy_id: &str,
    agreement_hash: &str,
    attestors: &[TemplateAttestor],
    milestones: &[MilestoneSpec],
    notes: Option<String>,
) -> Result<ProofPolicy, String>
```

**Behaviour:**
- Each `MilestoneSpec` produces a `ProofRequirement` with id `req-{milestone_id}`,
  resolution `MilestoneRelease`, and the milestone's `proof_type`.
- If `deadline_height` is set on a milestone, a `PolicyRule` with id `rule-{milestone_id}` is
  added: trigger `FundedAndNoRelease`, deadline `deadline_height`, resolution `Refund`.
- If `holdback_bps` and `holdback_release_height` are both set, a `PolicyHoldback` is added to
  the requirement with `deadline_height = holdback_release_height`.
- Rejects: empty attestors, empty milestones, duplicate milestone ids, `holdback_bps` set
  without `holdback_release_height`, `holdback_bps` > 10000.

**Example JSON** (two milestones, first with a 30-day holdback):
```json
{
  "schema_id": "irium.phase2.proof_policy.v1",
  "policy_id": "pol-construction-001",
  "agreement_hash": "aabbcc...",
  "approved_attestors": [
    { "attestor_id": "att-inspector", "pubkey_hex": "03abc...", "display_name": "Site Inspector" }
  ],
  "requirements": [
    {
      "requirement_id": "req-foundation",
      "proof_type": "foundation_complete",
      "resolution": "MilestoneRelease",
      "milestone_id": "foundation",
      "holdback": {
        "holdback_bps": 1000,
        "deadline_height": 750000
      }
    },
    {
      "requirement_id": "req-framing",
      "proof_type": "framing_complete",
      "resolution": "MilestoneRelease",
      "milestone_id": "framing"
    }
  ],
  "rules": [
    {
      "rule_id": "rule-framing",
      "trigger": "FundedAndNoRelease",
      "deadline_height": 800000,
      "resolution": "Refund"
    }
  ]
}
```

---

### `preorder_deposit_template`

**Use when:** a buyer deposits funds for a pre-ordered item. Funds release on a delivery proof;
a timeout rule refunds the buyer if no delivery proof arrives before a deadline.

**Signature:**
```rust
pub fn preorder_deposit_template(
    policy_id: &str,
    agreement_hash: &str,
    attestors: &[TemplateAttestor],
    delivery_proof_type: &str,
    refund_deadline_height: u64,
    holdback_bps: Option<u32>,
    holdback_release_height: Option<u64>,
    notes: Option<String>,
) -> Result<ProofPolicy, String>
```

**Behaviour:**
- Produces a single requirement `req-delivery` with resolution `Release`.
- Produces a single rule `rule-timeout-refund` with trigger `FundedAndNoRelease`,
  deadline `refund_deadline_height`, resolution `Refund`.
- If `holdback_bps` is set, a top-level `policy.holdback` is added (requires
  `holdback_release_height`).
- Rejects: empty attestors, `holdback_bps` set without `holdback_release_height`,
  `holdback_bps` > 10000.

**Example JSON** (5% holdback, 60-day refund window):
```json
{
  "schema_id": "irium.phase2.proof_policy.v1",
  "policy_id": "pol-preorder-42",
  "agreement_hash": "aabbcc...",
  "approved_attestors": [
    { "attestor_id": "att-warehouse", "pubkey_hex": "03def...", "display_name": "Warehouse" }
  ],
  "requirements": [
    {
      "requirement_id": "req-delivery",
      "proof_type": "shipment_delivered",
      "resolution": "Release"
    }
  ],
  "rules": [
    {
      "rule_id": "rule-timeout-refund",
      "trigger": "FundedAndNoRelease",
      "deadline_height": 850000,
      "resolution": "Refund"
    }
  ],
  "holdback": {
    "holdback_bps": 500,
    "deadline_height": 870000
  }
}
```

---

### `basic_otc_escrow_template`

**Use when:** two parties want a simple OTC escrow — funds release when one (or more) trusted
attestors submit a matching proof. A timeout refund fires if no release proof arrives.

**Signature:**
```rust
pub fn basic_otc_escrow_template(
    policy_id: &str,
    agreement_hash: &str,
    attestors: &[TemplateAttestor],
    release_proof_type: &str,
    refund_deadline_height: u64,
    threshold: Option<u32>,
    notes: Option<String>,
) -> Result<ProofPolicy, String>
```

**Behaviour:**
- Produces a single requirement `req-release` with resolution `Release`.
- If `threshold` is `None` or `Some(1)`, no `threshold` field is set on the requirement
  (single-attestor backward-compatible path).
- If `threshold` is `Some(n)` where `n > 1`, the requirement's `threshold` field is set to `n`
  and `required_attestor_ids` is populated from all attestors.
- Produces a single rule `rule-timeout-refund` with trigger `FundedAndNoRelease`,
  deadline `refund_deadline_height`, resolution `Refund`.
- Rejects: empty attestors, `threshold` > number of attestors.

**Example JSON** (2-of-3 multi-sig escrow):
```json
{
  "schema_id": "irium.phase2.proof_policy.v1",
  "policy_id": "pol-otc-77",
  "agreement_hash": "aabbcc...",
  "approved_attestors": [
    { "attestor_id": "att-a", "pubkey_hex": "03aaa...", "display_name": "Arbitrator A" },
    { "attestor_id": "att-b", "pubkey_hex": "03bbb...", "display_name": "Arbitrator B" },
    { "attestor_id": "att-c", "pubkey_hex": "03ccc...", "display_name": "Arbitrator C" }
  ],
  "requirements": [
    {
      "requirement_id": "req-release",
      "proof_type": "otc_trade_confirmed",
      "resolution": "Release",
      "threshold": 2,
      "required_attestor_ids": ["att-a", "att-b", "att-c"]
    }
  ],
  "rules": [
    {
      "rule_id": "rule-timeout-refund",
      "trigger": "FundedAndNoRelease",
      "deadline_height": 900000,
      "resolution": "Refund"
    }
  ]
}
```

---

### Input types

```rust
pub struct TemplateAttestor {
    pub attestor_id: String,
    pub pubkey_hex: String,
    pub display_name: Option<String>,
}

pub struct MilestoneSpec {
    pub milestone_id: String,
    pub label: Option<String>,
    pub proof_type: String,
    pub deadline_height: Option<u64>,
    pub holdback_bps: Option<u32>,
    pub holdback_release_height: Option<u64>,
}
```

### Serialising a template to JSON

```rust
pub fn policy_template_to_json(policy: &ProofPolicy) -> Result<String, String>
```

Wraps `serde_json::to_string_pretty`. Use this to write a policy to disk or transmit it
to `store-policy` RPC before a funded agreement starts.

### Design constraints

Templates compose only existing `ProofPolicy` primitives. They:

- Do **not** add new evaluation logic or consensus rules.
- Do **not** introduce new struct fields on `ProofPolicy`, `ProofRequirement`, or `PolicyRule`.
- Do **not** modify `evaluate_policy()` or any holdback/threshold evaluator.
- Are tested against `evaluate_policy()` directly so that template-generated policies behave
  identically to hand-crafted ones with the same structure.
