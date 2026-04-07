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
  "notes": null
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
- `milestone_id` scoping on requirements and rules is stored but not used in evaluation logic.

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
- `release_eligible` / `refund_eligible`: at most one will be true per response.
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
  "message": "proof accepted"
}
```

- `accepted`: true if the proof was newly stored.
- `duplicate`: true if a proof with the same `proof_id` was already present.
- If `accepted` is false and `duplicate` is false, an error occurred (returned as 400).

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

## POST /rpc/listproofs

Returns all stored proofs for a given agreement hash.

### Authentication

Requires `Authorization: Bearer <IRIUM_RPC_TOKEN>`. Rate-limited.

### Request

```json
{
  "agreement_hash": "<64-char hex>"
}
```

### Response (200 OK)

```json
{
  "agreement_hash": "<64-char hex>",
  "count": 1,
  "proofs": [
    { ... }
  ]
}
```

- `proofs`: array of `SettlementProof` objects sorted by `proof_id`.
- Returns an empty array if no proofs are stored for the given hash.

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
```

### Exit codes

- `0`: proof was accepted or duplicate.
- `1`: proof was rejected, RPC call failed, or input parsing failed.

---

## irium-wallet agreement-proof-list

```
irium-wallet agreement-proof-list \
  --agreement-hash <hex> \
  [--rpc <url>] \
  [--json]
```

### Arguments

| Flag | Required | Description |
|---|---|---|
| `--agreement-hash <hex>` | yes | SHA-256 hex of the agreement to query |
| `--rpc <url>` | no | Node RPC base URL |
| `--json` | no | Output raw JSON response |

### Default output

```
agreement_hash <64-char hex>
count 1
  proof_id=prf-001 attested_by=attestor-a proof_type=delivery_confirmation
```

### Exit codes

- `0` always (listing an empty set is not an error).



## Current limitations

The following items are defined in the type layer but not yet evaluated or exposed:

- **Proof-triggered refunds**: `ProofRequirement.resolution = "refund"` is stored and
  serialized but `evaluate_policy` does not process it. Refund eligibility is only
  reachable via `no_response_rules`.
- **`required_by` deadline on ProofRequirement**: the field exists and round-trips
  through JSON but is not checked during evaluation.
- **Milestone scoping**: `milestone_id` on requirements and rules is stored but
  evaluation does not filter or scope by milestone.
- **Proof persistence**: implemented via  and . ~~The node does not store proofs.~~
- **Attestor registry**: there is no persistent on-node attestor registry. Attestors
  are defined inline in each `ProofPolicy` object.
- **Explorer routes**: no `/agreement/policy*` routes exist in `irium-explorer` yet.
- **`AGREEMENT_SCHEMA_ID_V2`**: the constant `"irium.phase2.canonical.v1"` is defined
  but no code validates it against incoming agreement objects. Phase 2 agreements
  are validated by the existing Phase 1 `AgreementObject::validate()` path.
- **Proof creation tooling**: there is no `irium-wallet` command to create or sign
  a `SettlementProof`. Proofs must be constructed and signed externally using the
  canonical payload bytes from `settlement_proof_payload_bytes`.

---

## Application-layer note

Phase 2 policy evaluation is entirely off-chain application logic. The node reads the
current chain tip height for deadline evaluation but does not write any state, submit
transactions, or alter consensus rules. No blocks are produced or validated differently
as a result of any Phase 2 operation. Phase 1 HTLC spend eligibility (secret preimage,
timeout refund) is unchanged.
