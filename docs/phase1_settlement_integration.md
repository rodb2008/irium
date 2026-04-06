# Phase 1 Settlement Integration

Phase 1 settlement remains non-consensus. This layer adds wallet, RPC, and explorer tooling on top of the existing agreement object, OP_RETURN anchor, and HTLCv1-backed funding flow.

## Trust model

- Consensus-enforced: ordinary transaction validity, output ownership rules, timelocks already present in the chain, and OP_RETURN inclusion.
- HTLC-enforced: preimage release and timeout refund only when the funded agreement leg actually uses the existing HTLCv1 script path.
- Metadata/indexed: agreement object contents, milestone progress, lifecycle reconstruction, mediator references, document hashes, and agreement status/history views.
- Off-chain required: agreement exchange, milestone completion interpretation, secret/preimage handling, and business workflow coordination.

Do not describe Phase 1 as native consensus escrow. The chain enforces standard transaction validity and HTLCv1 branch conditions only. Agreement status is reconstructed by software from on-chain observations plus agreement metadata.

## Wallet CLI

The wallet exposes the following agreement commands:

- `agreement-template`
- `agreement-save`
- `agreement-load`
- `agreement-list`
- `agreement-export`
- `agreement-import`
- `agreement-inspect`
- `agreement-hash`
- `agreement-fund`
- `agreement-status`
- `agreement-milestones`
- `agreement-txs`
- `verify-agreement-link`
- `agreement-release-eligibility`
- `agreement-refund-eligibility`
- `agreement-release-build`
- `agreement-refund-build`
- `agreement-release-send`
- `agreement-refund-send`
- legacy-compatible aliases: `agreement-release`, `agreement-refund`

### Agreement bundles

Agreement bundles are local/off-chain persistence artifacts for Phase 1. They are not consensus state.

Bundle contents:

- canonical agreement JSON
- `agreement_hash`
- `agreement_id`
- bundle `version`
- local metadata such as:
  - `saved_at`
  - optional `source_label`
  - optional `note`
  - optional linked funding txid hints

The canonical agreement object remains the source of truth. The wallet verifies that the bundle hash matches the contained agreement before using it.

Default local storage:

- `~/.irium/agreements/<agreement_hash>.json`

Override if needed:

- `IRIUM_AGREEMENT_BUNDLES_DIR`

Supported local flows:

- `agreement-save <agreement.json|bundle.json|agreement_id|agreement_hash>`
- `agreement-load <bundle.json|agreement_id|agreement_hash>`
- `agreement-list`
- `agreement-export <bundle.json|agreement_id|agreement_hash> --out <file>`
- `agreement-import <bundle.json>`

Bundle-aware reuse:

Existing commands such as `agreement-status`, `agreement-milestones`, `agreement-txs`, `agreement-release-eligibility`, `agreement-refund-eligibility`, `agreement-release-build`, `agreement-refund-build`, `agreement-release-send`, and `agreement-refund-send` now accept:

- a raw agreement JSON file
- an exported bundle JSON file
- a saved local `agreement_id`
- a saved local `agreement_hash`

If multiple saved bundles share the same `agreement_id`, the wallet fails safely and requires the hash.

### Release / refund flow

`agreement-release-eligibility` and `agreement-refund-eligibility` ask the node to evaluate whether a specific funded agreement leg is currently spendable through the HTLC release or refund branch.

Inputs:

- agreement JSON file, exported bundle JSON file, or saved local agreement id/hash
- funding transaction id
- optional `--vout` for the HTLC output index
- optional `--milestone-id` when the agreement has multiple legs
- optional `--destination` override
- optional `--secret` for release/preimage-based spends
- optional `--json`

`agreement-release-build` and `agreement-refund-build` return a signed transaction artifact without broadcasting it. `agreement-release-send` and `agreement-refund-send` use the same flow but request node submission as part of the build step.

The older `agreement-release` and `agreement-refund` commands remain available for compatibility. They still support `--broadcast`, but the preferred operator flow is now explicit build vs send.

### Safety notes for secrets and raw transactions

Release spends can embed the preimage inside the signed HTLC transaction. Because of that:

- the human-readable wallet summary no longer prints `raw_tx_hex` by default
- use `--show-raw-tx` only when you explicitly need the signed artifact on stdout
- `--json` still includes the signed transaction artifact for automation, so handle that output as sensitive operational data
- secret input is validated strictly as hex
- invalid destination addresses are rejected before RPC submission

The wallet summary explicitly shows:

- agreement id and agreement hash
- funding tx reference and HTLC vout
- release or refund branch
- destination address
- amount when available from the node response
- whether a preimage is required
- current eligibility and blocking reason if ineligible
- HTLC/refund deadline context
- whether the current result is build-only or requested for node submission
- a trust-model warning that lifecycle status is not native consensus settlement state

## Node RPC surface

Settlement RPCs from the earlier Phase 1 steps remain available:

- `/rpc/createagreement`
- `/rpc/inspectagreement`
- `/rpc/computeagreementhash`
- `/rpc/fundagreement`
- `/rpc/listagreementtxs`
- `/rpc/agreementstatus`
- `/rpc/agreementmilestones`
- `/rpc/verifyagreementlink`
- `/rpc/agreementreleaseeligibility`
- `/rpc/agreementrefundeligibility`
- `/rpc/buildagreementrelease`
- `/rpc/buildagreementrefund`

These helpers are non-consensus convenience endpoints. They evaluate HTLC branch eligibility using observable chain data and the supplied agreement metadata, then build the corresponding HTLC spend transaction when eligible.

## Explorer / API

The explorer continues to forward settlement requests to the node RPC layer and wraps the response with interpretation labels so clients can distinguish consensus facts from HTLC-backed behavior and indexed metadata.

API routes:

- `POST /api/agreement/create`
- `POST /api/agreement/inspect`
- `POST /api/agreement/hash`
- `POST /api/agreement/fund`
- `POST /api/agreement/status`
- `POST /api/agreement/milestones`
- `POST /api/agreement/txs`
- `POST /api/agreement/verify-link`
- `POST /api/agreement/release-eligibility`
- `POST /api/agreement/refund-eligibility`
- `POST /api/agreement/lookup`

The explorer also exposes matching non-`/api` POST proxy routes for programmatic clients.

### HTML agreement views

The explorer provides minimal server-rendered agreement views on top of the same API surface:

- `GET /agreement`
- `POST /agreement/inspect/view`
- `POST /agreement/status/view`
- `POST /agreement/milestones/view`
- `POST /agreement/txs/view`
- `POST /agreement/release-eligibility/view`
- `POST /agreement/refund-eligibility/view`
- `POST /agreement/lookup/view`

The landing page now supports two truthful entry paths:

1. full agreement JSON or exported agreement bundle input for complete lifecycle/milestone/status reconstruction
2. txid/hash lookup for anchor discovery and agreement-hash confirmation only

### Lookup limitations

Txid lookup can:

- inspect a linked transaction
- detect OP_RETURN agreement anchors in that transaction
- show anchor roles and milestone ids when present
- confirm whether a supplied agreement hash matches an anchor found in the tx

Txid lookup cannot:

- reconstruct the full agreement object from chain data alone
- recover off-chain agreement metadata from chain data alone
- derive the full lifecycle or milestone intent without the canonical agreement JSON or an exported agreement bundle

The explorer UI states that limitation explicitly. It accepts exported bundle JSON as a convenience input, but it does not persist bundles server-side or pretend that on-chain anchor discovery is equivalent to full agreement recovery.

## Funding leg discovery and reconstructed timeline

The wallet and explorer can now derive likely HTLC-backed funding legs and a reconstructed agreement activity timeline from:

- the canonical agreement JSON
- an optional locally saved agreement bundle
- observed linked transactions and agreement anchors on chain

This is a convenience layer only. It is not native consensus agreement state.

### Funding leg discovery

Funding leg discovery reuses the existing agreement anchor model and observed linked transactions to produce candidate HTLC legs with:

- funding txid
- HTLC vout and anchor vout
- anchor role
- milestone id when present
- amount and timeout information when derivable
- release/refund eligibility hints
- source notes showing whether the leg came from direct anchor observation, bundle hints, or derived linking

Safe selection rules:

- if exactly one candidate is unambiguous, wallet release/refund flows may auto-select it, but they print the exact selection
- if multiple candidates remain, the wallet fails safely and requires explicit narrowing such as `--milestone-id` or `--vout`
- if bundle hints conflict with observed chain data, the flow fails hard
- if no HTLC-backed leg is discoverable, the older explicit `funding_txid` path remains available

### Reconstructed activity timeline

The `agreement-timeline` wallet command and matching explorer page render a derived event journal built from:

- canonical agreement hashing
- local bundle verification/saving events
- observed linked funding/release/refund transactions
- discovered funding leg candidates
- current HTLC eligibility checks
- agreement expiry derived from observed chain height and agreement deadlines

The timeline labels each event by source, such as:

- local bundle
- chain observed
- derived/indexed
- HTLC eligibility evaluation

This timeline is useful operational context, but it must not be treated as consensus-native settlement state or a complete reconstruction of off-chain agreement intent. Chain data alone still cannot recover the full canonical agreement object.

## Agreement audit / export records

The wallet and explorer can now generate a derived Phase 1 settlement audit record from:

- the canonical agreement JSON
- an optional verified local bundle
- observed linked transactions and agreement anchors
- discovered funding-leg candidates
- reconstructed timeline events and HTLC eligibility snapshots

This audit record is not a replacement for the canonical agreement bundle.

- The canonical agreement JSON remains the source of truth for the agreement itself.
- The audit record is a versioned derived report artifact for review, export, and operator/business handoff.

### Wallet audit commands

- `agreement-audit <agreement.json|bundle.json|agreement_id|agreement_hash>`
- `agreement-audit-export <agreement.json|bundle.json|agreement_id|agreement_hash> --out <file> [--format json|csv]`

The audit export includes:

- record metadata and trust-model summary
- canonical agreement summary
- local bundle context when present
- chain-observed linked transaction context
- discovered funding-leg summary and ambiguity warnings
- reconstructed timeline
- conservative derived settlement-state summary
- explicit trust-boundary lists

### JSON and CSV export formats

- JSON remains the richer machine-readable export for the full derived audit record.
- The wallet and explorer also provide a shorter derived settlement statement built from the same audit record pipeline. The statement is presentation-oriented for quick operator review, counterparty sharing, or printing, but it remains a derived Phase 1 report artifact rather than native agreement state.
- CSV is a flattened review/export convenience format built from the same derived audit record.
- CSV does not replace the canonical agreement JSON or the derived JSON audit record.

The CSV export uses a stable schema with deterministic row ordering and section markers such as:

- `summary`
- `linked_tx`
- `funding_leg`
- `timeline_event`
- `trust_boundary`

CSV is intended for operator review, accounting handoff, and business record export where a spreadsheet-friendly view is useful. It is still a derived Phase 1 report artifact, not native consensus contract state.

### Explorer audit preview

The explorer now provides an audit preview view on top of the same node-backed RPC path. It renders a single settlement review page from supplied agreement JSON or bundle JSON, but it still does not persist bundles server-side and it does not claim to reconstruct the agreement from chain data alone.

The audit view also exposes small download actions for:

- audit JSON
- audit CSV

Those downloads are generated from the same supplied agreement or bundle plus observed chain activity. They do not imply server-side agreement recovery or persisted agreement state.

### Trust-model limits

The audit/export record is helpful for OTC, contractor, and merchant settlement review, but it remains Phase 1 only:

- consensus enforces ordinary transaction validity and anchor visibility only
- HTLCv1 enforces release/refund branches only for HTLC-backed legs
- lifecycle, funding-leg discovery, and timelines are derived/indexed outputs
- local bundle labels, notes, and hints are off-chain convenience only
- off-chain agreement exchange is still required for full context

## Practical limitations

Still intentionally deferred:

- any new consensus settlement output type
- generalized smart contract execution
- subjective dispute resolution
- automatic milestone attestation or Phase 2 proof automation
- Phase 3 reputation or business platform features
- any claim that refunds or releases happen automatically without constructing and broadcasting the corresponding HTLC spend


## Derived settlement statement

The statement view is a shorter presentation layer built from the existing derived audit record.

It is intended for:

- OTC counterparty review
- contractor and milestone settlement review
- merchant or operator handoff
- printable/shareable Phase 1 summaries

It is not intended to replace:

- the canonical agreement JSON
- the local agreement bundle
- the full derived audit record

Wallet commands:

- `agreement-statement <agreement.json|bundle.json|agreement_id|agreement_hash>`
- `agreement-statement-export <agreement.json|bundle.json|agreement_id|agreement_hash> --out <file> [--json]`

Explorer support:

- `POST /agreement/statement`
- `POST /api/agreement/statement`
- `POST /agreement/statement/view`
- `POST /agreement/statement/download.json`

The statement explicitly labels:

- observed on-chain transactions and anchor references
- HTLC-backed release/refund path summaries
- derived status summary
- off-chain/local agreement context

The statement remains non-consensus. It does not imply native escrow state, native milestone state, or automatic settlement execution.


## Artifact verification

The wallet and explorer now provide a counterparty verification flow for shared Phase 1 artifacts.

Accepted artifacts:
- canonical agreement JSON
- exported agreement bundle JSON
- derived audit JSON
- derived statement JSON

The verification flow checks, where possible:
- canonical agreement hash recomputation
- whether a supplied bundle matches the canonical agreement
- whether supplied audit or statement identity fields match the canonical agreement
- whether linked transactions and anchors referenced by the derived audit can still be confirmed from current chain observation
- whether current derived recomputation is consistent with the supplied artifacts

### What verification can and cannot prove

Verification can prove or flag:
- matching canonical agreement hash
- bundle hash mismatch or tamper
- audit/statement identity mismatch
- observed anchor or linked transaction presence when the current node can still observe it
- consistency or drift between supplied derived artifacts and freshly recomputed derived outputs

Verification cannot prove from chain data alone:
- the full off-chain agreement terms when no canonical agreement or bundle is supplied
- milestone meaning beyond the supplied agreement metadata
- subjective performance or dispute outcomes
- native consensus agreement state, because none exists in Phase 1

### Wallet verification command

- `agreement-verify-artifacts [--agreement <agreement.json>] [--bundle <bundle.json>] [--audit <audit.json>] [--statement <statement.json>] [--rpc <url>] [--json] [--out <file>]`

The wallet renders a human-readable verification summary by default and can emit stable JSON for automation or export.

### Explorer verification view

Explorer routes added for artifact verification:
- `POST /agreement/verify-artifacts`
- `POST /api/agreement/verify-artifacts`
- `POST /agreement/verify-artifacts/view`

The explorer verification page is derived from supplied artifacts plus current node-backed chain observation. It does not persist those artifacts server-side, and it does not imply that chain data alone can reconstruct the full agreement object.

## Phase 1.5 agreement workflow layer

Phase 1.5 stays fully non-consensus. It adds deterministic agreement creation, bundle packaging, inspection, and verification on top of the existing Phase 1 settlement and artifact-verification surface.

### Canonical agreement schema

Phase 1.5 templates now emit canonical agreements with:

- `schema_id: irium.phase1.canonical.v1`
- existing `version` field preserved for object compatibility
- lexicographically sorted object keys during canonical serialization
- omitted `Option::None` and empty skipped fields
- UTF-8 canonical JSON bytes hashed with `sha256`

Canonical agreement JSON remains the source of truth for agreement terms. Legacy agreements without `schema_id` remain valid for backward compatibility, but new Phase 1.5 creation flows set the schema id explicitly.

Validation is now stricter for:

- agreement template type and party layout
- amount and deadline consistency
- optional metadata-only references such as asset/payment/purpose text
- optional attestor or resolver references as metadata only
- document and metadata hash formatting
- milestone arrays and refund timeout ordering

### Template creation commands

Wallet commands added for local canonical agreement creation:

- `agreement-create-simple-settlement`
- `agreement-create-otc`
- `agreement-create-deposit`
- `agreement-create-milestone`
- `agreement-bundle-create`
- `agreement-bundle-pack`
- `agreement-bundle-inspect`
- `agreement-bundle-verify`
- `agreement-bundle-unpack`

All template creation happens locally and is self-custodial. These commands:

- never require a hosted service
- support `--out <file>`
- support `--json`
- produce deterministic canonical agreement JSON when given the same inputs
- do not imply native on-chain agreement enforcement

Template intent:

- OTC: buyer/seller, asset reference, payment reference, HTLC-backed release/refund summary
- Milestone: payer/payee plus milestone array with ids, titles, amounts, deadlines, and optional deliverable metadata hashes
- Deposit: payer/payee, deposit amount, purpose reference, refundable-conditions summary, refund timeout
- Simple settlement: two-party amount, settlement deadline, release/refund summaries

### Agreement bundle format

Phase 1.5 standardizes the bundle around the canonical agreement plus optional derived artifacts.

Bundle structure now supports:

- `bundle_schema_id: irium.phase1.bundle.v1`
- canonical agreement JSON
- agreement hash and agreement id
- local bundle metadata
- optional embedded audit JSON
- optional embedded statement JSON
- optional metadata summary
- optional chain-observation snapshot
- optional external document hash references

Bundle hashing remains deterministic and is computed from the canonicalized bundle JSON. Bundle verification checks:

- canonical agreement hash match
- bundle schema id when present
- embedded audit/statement agreement identity consistency
- external document hash formatting

### Explorer Phase 1.5 surfaces

The explorer now includes Phase 1.5 helper pages on top of the same shared logic:

- `GET /agreement`
- `POST /agreement/create/simple-settlement/view`
- `POST /agreement/create/otc/view`
- `POST /agreement/create/deposit/view`
- `POST /agreement/create/milestone/view`
- existing inspect / audit / statement / verify-artifacts views continue to work

The explorer agreement page now supports:

- template-helper forms that generate canonical agreement JSON locally in the browser request cycle
- bundle-aware inspection with bundle hash and schema display
- clearer verification sections for supplied bundle, audit, and statement artifacts
- download/copy affordances through the existing JSON export surfaces and displayed hashes

### Self-custodial workflow expectations

A normal Phase 1.5 workflow is:

1. create canonical agreement JSON locally
2. inspect and hash it
3. optionally pack it into a deterministic bundle
4. share the agreement or bundle directly with counterparties
5. verify the shared artifacts independently with wallet or explorer tools
6. derive statements and audits as informational report artifacts only

What remains off-chain in Phase 1.5:

- agreement exchange and storage
- milestone interpretation
- attestor or resolver references
- business workflow coordination
- bundle distribution and artifact sharing

What remains non-consensus:

- templates
- bundles
- statements
- audits
- verification reports
- explorer agreement pages

Future Phase 2 proof automation can reuse these canonical artifacts, but this patch does not implement proof execution, signatures, attestor automation, or on-chain state changes.

## Phase 1.75 artifact signatures

Phase 1.75 adds a local authenticity layer for agreement artifacts and bundles. It stays fully non-consensus.

What signatures do:

- prove that a holder of a local private key signed a specific agreement hash or bundle hash
- provide portable proof of authorship or intent for off-chain agreement exchange
- support offline verification in wallet tooling and explorer views

What signatures do not do:

- enforce settlement on-chain
- prove the agreement is true or fair
- prove the signer had authority beyond control of the signing key
- create native consensus agreement state

### Signature envelope

Agreement and bundle signatures use a deterministic envelope with:

- `version`
- `target_type`: `agreement` or `bundle`
- `target_hash`
- `signer_public_key`
- optional `signer_address`
- `signature_type`: currently `secp256k1_ecdsa_sha256`
- optional `timestamp`
- optional `signer_role`
- `signature`

The agreement hash remains the root of trust for agreements. The bundle hash remains the root of trust for bundle signatures.

### Detached vs embedded signatures

Two transport styles are supported:

- detached signature JSON for agreement or bundle signatures
- embedded bundle signatures stored in `bundle.signatures`

Embedded signatures do not change the bundle hash. Bundle hashing excludes the mutable `signatures` list so signed bundles remain portable without changing the underlying bundle identity.

### Wallet commands

Wallet additions for Phase 1.75:

- `agreement-sign --agreement <agreement.json|-> --signer <base58_addr> [--role <text>] [--timestamp <unix>] [--out <file>] [--json]`
- `agreement-verify-signature [--agreement <agreement.json|->] [--bundle <bundle.json|->] --signature <signature.json|-> [--json] [--out <file>]`
- `agreement-bundle-sign --bundle <bundle.json|agreement_id|agreement_hash|-> --signer <base58_addr> [--role <text>] [--timestamp <unix>] [--embed] [--out <file>] [--json]`
- `agreement-bundle-verify-signatures --bundle <bundle.json|agreement_id|agreement_hash|-> [--json]`
- `agreement-signature-inspect --signature <signature.json|-> [--agreement <agreement.json|->] [--bundle <bundle.json|->] [--json]`

These commands:

- run locally and self-custodially
- do not require server-side services
- verify signatures offline
- report authenticity only, not correctness or enforceability

### Explorer support

Explorer additions for Phase 1.75:

- `POST /agreement/verify-signature`
- `POST /api/agreement/verify-signature`
- `POST /agreement/verify-signature/view`

The explorer agreement landing page now includes a signature verification section. It accepts canonical agreement JSON or bundle JSON plus optional detached signature JSON and renders:

- detached signature validity
- embedded bundle signature validity
- signer metadata when supplied
- whether the signature target hash matches the supplied agreement or bundle
- authenticity-only warning text

### Trust boundary extension

Phase 1.75 extends the trust model with an authenticity layer:

- authenticity: a valid signature proves the supplied public key signed the supplied target hash
- truth: not guaranteed
- enforceability: not guaranteed
- consensus state: unchanged

Use conservative language when presenting results:

- "verified signature" means the signature matched the supplied artifact hash
- "signature mismatch" means the supplied artifact and signature were inconsistent or invalid
- "unverifiable" means the explorer or wallet did not have enough canonical artifact context to check the claim fully

### Examples

OTC example:

1. create canonical agreement JSON locally with `agreement-create-otc`
2. compute and review the agreement hash
3. have buyer and seller each run `agreement-sign`
4. optionally pack the agreement plus detached artifacts into a bundle for transport
5. receiving party runs `agreement-verify-signature` or uses explorer signature verification

Milestone example:

1. create canonical agreement JSON locally with `agreement-create-milestone`
2. contractor and client sign the same canonical agreement hash
3. pack the agreement and signatures into a bundle if desired
4. recipients verify bundle signatures offline with `agreement-bundle-verify-signatures`

Phase 1.75 prepares the groundwork for later objective-proof automation, but this patch does not add signatures to consensus, attestor automation, proof execution, or automatic settlement behavior.

## Phase 1.75.1 Signature Integration

This narrow follow-up step integrates the existing non-consensus signature layer into artifact verification, audit, and statement surfaces.

### What changed

Wallet flows now accept optional detached signature artifacts for:

- `agreement-audit`
- `agreement-audit-export`
- `agreement-statement`
- `agreement-statement-export`
- `agreement-verify-artifacts`

Supported flags:

- `--agreement-signature <signature.json|->`
- `--bundle-signature <signature.json|->`

Explorer agreement forms now also accept optional detached agreement and bundle signature JSON for:

- artifact verification view
- audit view
- statement view

### Verification integration

Artifact verification now includes an optional `authenticity` section when signatures are supplied or when embedded bundle signatures are present.

It reports, conservatively:

- detached agreement signatures supplied
- detached bundle signatures supplied
- embedded bundle signatures supplied
- valid signatures
- invalid signatures
- unverifiable signatures
- signer summaries
- authenticity warnings

Interpretation rules remain strict:

- valid means the supplied signature cryptographically matched the supplied target hash and the expected canonical hash was known
- invalid means signature verification failed, the target type was wrong, or the target hash mismatched the supplied artifact
- unverifiable means the signature was cryptographically valid but the explorer or wallet did not have enough canonical context to confirm the intended target hash

### Audit and statement integration

The derived audit record may now include a concise `authenticity` section when signatures are supplied during wallet or explorer generation.

The derived statement may now include a compact authenticity summary such as:

- agreement signatures valid / invalid / unverifiable counts
- bundle signatures valid / invalid / unverifiable counts
- authenticity-only notice

These authenticity sections are presentation and verification metadata only. They do not modify canonical agreement hashing, derived settlement state, funding-leg discovery, or HTLC enforcement.

### Trust boundary reminder

Signatures in Phase 1.75.1 mean only:

- a supplied key signed a supplied agreement hash or bundle hash

They do not mean:

- the agreement is true
- the agreement is fair
- the signer had legal authority
- the agreement is enforced on-chain
- the derived settlement state is native consensus state

Agreement signatures remain separate from settlement enforcement. HTLC-backed release and refund remain objective only when the actual funding leg uses existing HTLCv1 rules.

## Phase 1.75.2 Share Packages

This step adds a compact non-consensus handoff format for counterparties who need to exchange the current agreement artifacts without introducing a new trust root.

### What a share package is

A share package is a versioned JSON transport artifact. It can carry some combination of:

- canonical agreement JSON
- agreement bundle JSON
- derived audit JSON
- derived statement JSON
- detached agreement signatures
- detached bundle signatures

The package is only a packaging convenience. It is not a trusted object by itself. Verification still depends on canonical agreement hashing, bundle hashing, detached signature checks, and derived re-verification against observed chain data where available.

### Wallet commands

New wallet commands:

- `agreement-share-package`
- `agreement-share-package-inspect`
- `agreement-share-package-verify`

Typical export flow:

1. prepare a canonical agreement JSON or bundle JSON
2. optionally include a derived audit or statement artifact
3. optionally include detached agreement or bundle signatures
4. export a handoff package with `agreement-share-package --out package.json`

Selective export is supported for narrower handoff use cases. Use repeatable `--include` flags to keep a package intentionally small, for example:

- `--include agreement`
- `--include agreement --include statement`
- `--include agreement --include bundle`
- `--include agreement --include statement --include agreement-signatures`

The share-package manifest records what was included and what was omitted. Omitted artifacts are absent from that package only; recipients must not treat omission as proof that no additional off-chain artifacts, statements, audits, or signatures exist elsewhere.

Typical receive-and-verify flow:

1. inspect the package contents with `agreement-share-package-inspect package.json`
2. verify the package with `agreement-share-package-verify package.json`
3. selectively import verified local artifacts with `agreement-share-package-import package.json --import ...`
4. review local intake receipts with `agreement-share-package-list` and `agreement-share-package-show <receipt-id>`

Verification output classifies package contents as:

- verified
- mismatched
- unverifiable
- absent

Absence is not treated as failure.

### Verified local intake and inbox receipts

Verified share packages can now be ingested into a local wallet-side inbox. This remains local-only and file-based.

New wallet commands:

- `agreement-share-package-import`
- `agreement-share-package-list`
- `agreement-share-package-show`

Import is conservative:

- the package is verified first using the existing share-package verification flow
- the recipient must explicitly choose what to import with repeatable `--import` flags
- imported canonical agreements and bundles are stored in the existing local agreement areas
- imported detached signatures are stored separately by target hash
- imported audit and statement artifacts remain informational local files only
- each successful intake writes a local receipt with provenance metadata, package copy, and verification snapshot

Typical import examples:

- `agreement-share-package-import package.json --import agreement`
- `agreement-share-package-import package.json --import agreement --import bundle --import agreement-signatures`
- `agreement-share-package-import package.json --import statement --import audit`

Local provenance metadata is informational only. It records where the verified package came from and what was imported, but it does not make the agreement trusted, true, authorized, enforceable, or native chain state.

Conflict rules remain strict:

- conflicting local agreement content for the same hash is rejected
- conflicting `agreement_id -> hash` mappings fail safely and require hash-based disambiguation
- detached signatures that do not verify against the canonical target hash are not imported
- omitted artifacts remain absent from that package only

### Local housekeeping for inbox receipts and imported artifacts

Wallet-side share-package storage can now be managed with conservative local housekeeping commands. This is local-only, file-based housekeeping for the recipient wallet. It does not revoke any package, invalidate artifacts elsewhere, or change chain state.

New wallet commands:

- `agreement-local-store-list`
- `agreement-share-package-archive`
- `agreement-share-package-prune`
- `agreement-share-package-remove`

Storage model:

- active receipts remain under `~/.irium/share-package-inbox/<receipt-id>/...`
- archived receipts move under `~/.irium/share-package-inbox/archived/<receipt-id>/...`
- archived receipts may carry a local `housekeeping.local.json` sidecar with informational metadata such as `archived_at`, `archived_by_action`, and `prune_reason`
- canonical imported agreements, bundles, and detached signatures remain in their existing local stores
- housekeeping metadata stays separate from canonical agreement and bundle objects

Command semantics:

- `agreement-local-store-list` reports local-only inventory for active receipts, archived receipts, stored bundles, stored raw agreements, detached signatures, and informational receipt-local files
- `agreement-share-package-archive <receipt-id>` moves a local receipt into the archived inbox area and preserves the receipt metadata and provenance snapshot
- `agreement-share-package-remove ...` removes an exact local target only. By default this removes the targeted receipt or local informational copy only. Canonical imported artifacts require explicit `--remove-imported-artifacts` handling.
- `agreement-share-package-prune` is conservative and should be used with `--dry-run` first. Archived receipt cleanup is gated behind explicit flags such as `--include-archived` and `--older-than <days>`.

Safety rules:

- housekeeping is local only and informational only
- deleting local artifacts does not revoke, invalidate, or erase anything on-chain
- deleting local artifacts does not invalidate copies already shared elsewhere
- canonical imported agreements, bundles, and detached signatures are not treated as temporary clutter by default
- when an imported canonical artifact is still referenced by multiple local receipts, removal fails safely or is skipped with a warning
- receipt-local audit and statement copies remain informational files only, but removal still affects local records only
- prune and remove support `--dry-run` so operators can inspect what would change before mutating files

Recommended operator flow:

1. inspect local state with `agreement-local-store-list --include-archived`
2. archive completed or inactive receipts with `agreement-share-package-archive <receipt-id>`
3. run `agreement-share-package-prune --dry-run --include-archived --older-than <days>`
4. only if the dry run looks correct, rerun without `--dry-run`
5. use `agreement-share-package-remove` only for exact local receipt or artifact cleanup

Intentional limitations:

- no new trust root is introduced
- no package-level signature or registry system is introduced
- no background lifecycle engine or database is introduced
- no settlement automation or Phase 2 proof flow is introduced
- no local housekeeping action changes consensus, activation, mining, mempool, replay, or wallet format behavior

### Operator workflow examples

These examples stay fully non-consensus. They describe practical operator flows for the existing Phase 1, Phase 1.5, and Phase 1.75.x tooling. None of them creates native consensus agreement state.

#### OTC settlement handoff

1. create canonical OTC agreement JSON with `agreement-create-otc`
2. compute and share the canonical agreement hash with `agreement-hash`
3. optionally save a local bundle with `agreement-bundle-create` or `agreement-bundle-pack`
4. fund the agreement through the existing HTLC-backed flow with `agreement-fund`
5. track linked transactions and derived status with `agreement-txs`, `agreement-status`, and `agreement-timeline`
6. export a compact handoff package with `agreement-share-package --out otc-package.json --include agreement --include bundle --include agreement-signatures --include statement`
7. the counterparty verifies with `agreement-share-package-verify otc-package.json` before importing anything locally

#### Contractor / milestone settlement review

1. create the canonical milestone agreement with `agreement-create-milestone`
2. fund the agreement and confirm milestone funding candidates with `agreement-fund` and `agreement-funding-legs`
3. use `agreement-timeline` and `agreement-audit` to review milestone-linked events and derived lifecycle state
4. produce a concise review artifact with `agreement-statement` or `agreement-statement-export`
5. if a counterparty sends updated off-chain artifacts, verify them with `agreement-verify-artifacts` or the explorer verification page before relying on them

#### Merchant / delayed settlement or deposit flow

1. create the canonical deposit-style agreement locally with the deposit template flow
2. fund through the normal HTLC-backed path
3. use `agreement-status`, `agreement-milestones`, and `agreement-release-eligibility` or `agreement-refund-eligibility` to understand what branch is objectively available
4. use `agreement-audit-export --format csv` when an operator needs an accounting-friendly export for internal review
5. remember that business workflow coordination still remains off-chain metadata and operator judgment in Phase 1

#### Share-package verify and import flow

1. inspect the package with `agreement-share-package-inspect package.json`
2. verify the package with `agreement-share-package-verify package.json`
3. import only the local artifacts you actually need with explicit `--import` flags
4. review the resulting local receipt with `agreement-share-package-show <receipt-id>`
5. use `agreement-local-store-list --include-archived` to inspect what is stored locally afterward

#### Local housekeeping flow

1. list the current local receipt and artifact store with `agreement-local-store-list --include-archived`
2. archive inactive receipts with `agreement-share-package-archive <receipt-id>`
3. preview cleanup safely with `agreement-share-package-prune --dry-run --include-archived --older-than <days>`
4. if the report looks correct, rerun the same prune command without `--dry-run`
5. use `agreement-share-package-remove` only for exact local targets when you need precise cleanup beyond the conservative prune path

### Manifest summary and package profiles

Each share package now carries a descriptive manifest summary. It is transport metadata only and does not become a new trust root.

The manifest includes:

- `package_profile`
- `included_artifact_types`
- `omitted_artifact_types`
- `verification_notice`

Current conservative profile labels are:

- `minimal_agreement_handoff`
- `review_package`
- `verification_package`
- `full_informational_package`

These labels only describe what was packed for handoff. Recipients must still verify included artifacts against canonical agreement hashes, canonical bundle hashes, detached signatures, and derived verification output.

### Explorer support

The explorer now provides a pasted share-package verification entry point.

Supported routes:

- `POST /agreement/share-package/verify`
- `POST /api/agreement/share-package/verify`
- `POST /agreement/share-package/verify/view`

Explorer verification is derived from the supplied package contents plus observed chain activity where available. It does not create native agreement state or settlement enforcement.

### Trust boundary reminder

A share package may contain canonical, derived, and signature artifacts together, but it does not change the trust model:

- canonical agreement JSON remains the source of truth for agreement terms
- bundle and signature hashes remain the authenticity roots
- audit and statement content remain informational and derived
- chain data alone still cannot recover the full agreement object
- package contents do not become native consensus state by being packaged together

### Intentionally deferred

This step does not add:

- package-level signatures as a new protocol
- server-side package hosting or registry
- attestor logic
- threshold signatures
- proof execution
- settlement automation
