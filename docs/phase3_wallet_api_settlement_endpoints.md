# Phase 3: Settlement / Agreement / Policy / Proof Endpoints

Added to irium-wallet-api in the next-upgrade-planning-clean branch.

## New Routes (13 total)

### Agreement
- POST /agreement/create           -> proxy iriumd /rpc/createagreement
- POST /agreement/create/otc       -> LOCAL: build OTC AgreementObject via settlement lib
- POST /agreement/hash             -> proxy iriumd /rpc/computeagreementhash
- POST /agreement/settle-status    -> proxy iriumd /rpc/agreementstatus

### Policy
- POST /policy/build/otc           -> proxy iriumd /rpc/buildotctemplate
- POST /policy/set                 -> proxy iriumd /rpc/storepolicy
- POST /policy/get                 -> proxy iriumd /rpc/getpolicy
- POST /policy/evaluate            -> proxy iriumd /rpc/evaluatepolicy

### Proof
- POST /proof/create               -> LOCAL: build + sign SettlementProof (k256/SHA-256)
- POST /proof/submit               -> proxy iriumd /rpc/submitproof
- POST /proof/list                 -> proxy iriumd /rpc/listproofs
- POST /proof/get                  -> proxy iriumd /rpc/getproof

### Settlement
- POST /settlement/build           -> proxy iriumd /rpc/buildsettlementtx

## Local Endpoints

### POST /agreement/create/otc

Builds an OTC AgreementObject without calling iriumd. Useful for offline drafting.

Required fields: agreement_id, buyer_party_id, buyer_display_name, buyer_address,
  seller_party_id, seller_display_name, seller_address, total_amount, asset_reference,
  payment_reference, refund_timeout_height, secret_hash_hex (64-char hex),
  document_hash (64-char hex).

Optional fields: creation_time (defaults to now), metadata_hash, notes.

Returns a full AgreementObject JSON.

### POST /proof/create

Builds and signs a SettlementProof locally. Caller must supply the signing key.

Required fields: agreement_hash (64-char hex), proof_type, attested_by,
  signing_key_hex (32-byte secp256k1 key as hex), pubkey_hex (SEC1 compressed).

Optional fields: milestone_id, evidence_summary, evidence_hash, proof_id
  (auto-derived if absent), timestamp (defaults to now), expires_at_height,
  proof_kind, reference_id.

Returns a signed SettlementProof JSON.

## Error Response Format

All new endpoints return JSON error bodies on failure:

  { "error": "<message>", "code": "<code>" }

Codes: rate_limit, network_error, parse_error, build_error, invalid_key,
  payload_error, sign_error, serialize_error.

## Tests (5 unit tests in src/bin/irium-wallet-api.rs)

- test_agreement_create_otc_valid
- test_agreement_create_otc_missing_field  -> 422 on missing required field
- test_proof_create_valid                  -> verifies schema_id and non-empty signature
- test_proof_create_bad_key               -> 400 + code=invalid_key on bad hex
- test_proxy_endpoints_return_bad_gateway_when_node_down -> 502 for all 11 proxy routes

## Implementation Notes

- proxy_post_value() is a new generic async helper for POST requests to iriumd that
  returns structured JSON errors instead of bare StatusCode.
- json_err() produces { "error": ..., "code": ... } responses.
- No consensus changes, no activation heights, no MPSOv1, no LWMA changes.
