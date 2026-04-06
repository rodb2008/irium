# Irium Phase 1 Settlement Foundation

## Scope
Phase 1 adds a settlement-first foundation for agreement-linked payments on the live Irium mainnet without changing consensus or economic policy.

This phase is intentionally limited to:
- agreement object specification
- deterministic agreement hashing
- agreement-hash anchoring through OP_RETURN metadata outputs
- settlement funding templates built on existing HTLCv1 outputs
- lifecycle/status reconstruction from on-chain links plus documented off-chain agreement metadata
- RPC and explorer-facing settlement inspection support

This phase does not add:
- subjective dispute resolution in consensus
- admin keys or privileged override paths
- general smart contracts or VM expansion
- resolver trust inside consensus
- oracle automation
- Phase 2 proof automation

## Capability Assessment
Current Irium primitives already provide the required objective base for a non-consensus Phase 1 rollout.

### Existing objective primitives
- UTXO transaction model with stable transaction serialization
- P2PKH spend model
- Native `HTLCv1` output support in `src/tx.rs` and validation in `src/chain.rs`
- Height-based timeout enforcement through HTLC refund paths
- Preimage-based release paths through HTLC claim witnesses
- OP_RETURN-style small metadata commitments already used in `src/relay.rs`
- Activation framework already present in `src/activation.rs`
- Wallet/mempool/node RPC surfaces already present in `src/bin/iriumd.rs`
- Explorer proxy/API layer already present in `src/bin/irium-explorer.rs`

### Gaps for Phase 1
Current mainnet primitives do not provide:
- generalized multi-branch contracts
- threshold multisig settlement policies
- native milestone consensus accounting
- native agreement/settlement output types
- subjective dispute resolution

Those gaps do not block an initial Phase 1 rollout because the required templates can be modeled as:
- one or more HTLC funding legs
- one or more agreement anchor outputs
- off-chain agreement metadata with deterministic hashing
- indexed lifecycle reconstruction from linked transactions

### Recommendation
Initial Phase 1 rollout should be non-consensus only.

Consensus changes are not required for:
- agreement-linked funding
- escrow-style HTLC release/refund structure
- milestone-capable settlement layout via multiple HTLC legs
- refundable deposit/reservation structures where the lock/refund path is objectively expressible through HTLCv1
- lifecycle/status/history indexing

If a future phase wants native settlement outputs, that should be a separate activation-gated design after Phase 1 adoption validates the model.

## Agreement Object
The canonical agreement object is implemented in `src/settlement.rs`.

Required fields include:
- `agreement_id`
- `version`
- `template_type`
- `parties`
- `payer`
- `payee`
- optional `mediator_reference` as metadata only
- `total_amount`
- `network_marker`
- `creation_time`
- `deadlines`
- `release_conditions`
- `refund_conditions`
- optional `milestones`
- optional `deposit_rule`
- optional `proof_policy_reference`
- `document_hash`
- optional `metadata_hash`
- optional business references such as `invoice_reference` and `external_reference`

The agreement hash is:
- deterministic
- versioned
- SHA-256 over canonicalized JSON with stable key ordering

## On-Chain Linking Model
Phase 1 uses agreement anchors encoded as OP_RETURN outputs.

Anchor payload format:
- prefix: `agr1:`
- agreement hash
- role short code
- optional milestone id

Examples:
- funding anchor
- release anchor
- refund anchor
- milestone release anchor
- deposit lock anchor
- OTC settlement anchor
- merchant settlement anchor

This keeps settlement linking:
- backward compatible
- explorer-readable
- RPC-queryable
- wallet-constructible
- non-consensus

## Settlement Templates
Phase 1 templates are metadata templates over existing HTLC outputs.

### Simple release/refund
- funding path: one HTLC leg
- release path: preimage claim
- refund path: timeout refund
- on-chain enforcement: HTLC only
- off-chain coordination: who releases/when

### Milestone settlement
- funding path: multiple HTLC legs, one per milestone
- release path: milestone-by-milestone preimage claim
- refund path: per-leg timeout refund
- on-chain enforcement: HTLC legs only
- off-chain coordination: milestone acceptance/order

### Refundable deposit / reservation
- funding path: one HTLC lock leg tagged as deposit
- release path: beneficiary claim through agreed preimage
- refund path: timeout refund
- on-chain enforcement: HTLC timeout/preimage only

### OTC settlement
- funding path: HTLC-backed settlement leg plus OTC anchor role
- release/refund path: same HTLC mechanics
- off-chain coordination: trade execution context

### Merchant delayed settlement
- funding path: HTLC-backed merchant settlement leg
- release/refund path: same HTLC mechanics
- off-chain coordination: fulfillment confirmation

### Contractor milestone template
- funding path: milestone HTLC legs
- release path: per milestone claim
- refund path: per milestone timeout refund
- off-chain coordination: work acceptance

## Lifecycle Rules
Lifecycle is not a new consensus state machine.

It is derived from:
- observable linked on-chain transactions
- agreement metadata
- current chain height

Current indexed states:
- `draft`
- `proposed`
- `funded`
- `partially_released`
- `released`
- `refunded`
- `expired`
- `cancelled` as local/off-chain only
- `disputed_metadata_only` as metadata only

Important trust boundary:
- lifecycle indexing is business/application logic
- only HTLC release/refund rules are consensus-enforced
- mediator/dispute references remain off-chain metadata in Phase 1

## RPC Surface
Phase 1 RPC additions are implemented in `src/bin/iriumd.rs`.

Added endpoints:
- `/rpc/createagreement`
- `/rpc/inspectagreement`
- `/rpc/computeagreementhash`
- `/rpc/fundagreement`
- `/rpc/listagreementtxs`
- `/rpc/agreementstatus`
- `/rpc/agreementmilestones`
- `/rpc/verifyagreementlink`

Current behavior:
- validates deterministic agreement objects
- computes agreement hash/summary
- builds funding transactions from wallet UTXOs using existing HTLCv1 primitives
- adds agreement anchor outputs
- reconstructs linked history by scanning chain transactions for agreement anchors
- derives lifecycle and milestone status without changing validation rules

## Backward Compatibility
Phase 1 does not change:
- block validation
- transaction validation rules
- old transaction replay
- address format
- mining rules
- mempool policy for legacy transactions
- wallet format compatibility
- historical chain replay

Nodes that ignore Phase 1 RPCs still interoperate fully because agreement data is carried through ordinary transactions plus OP_RETURN metadata outputs.

## Deferred Work
Intentionally deferred from Phase 1:
- native settlement output types
- release/refund wrappers with richer anchor-aware spend builders
- native milestone consensus accounting
- dispute automation
- resolver attestation logic
- proof-policy execution
- business platform ambitions beyond basic settlement exports and linked history

## Operator Notes
- No activation is required for the Phase 1 settlement object and anchor model itself.
- HTLC-backed funding requires the existing HTLCv1 activation rules already present on the network.
- Agreement metadata must not be presented as consensus truth where it is only indexed context.
- Explorers and business tools must clearly distinguish objective chain facts from off-chain agreement assumptions.
