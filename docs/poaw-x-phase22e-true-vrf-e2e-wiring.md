# PoAW-X Phase 22E — true-VRF AssignmentProofV2 end-to-end production wiring

**Status:** Implemented, **local-only** (not pushed; remote branch absent; no merge/PR/tag/
release). **Gated; mainnet hard-off; not mainnet-ready.** Builds on Phase 22D (the
`AssignmentProofV2` primitive + chain enforcement). This phase wires the proof through the
full production path: **wallet/miner → candidate admission → node cache/RPC → pool
fetch/bundle → block ext AVR2 → node connect_block**.

## Ownership model (unchanged, enforced)

- The **miner/wallet** owns the VRF secret and is the ONLY party that produces a V2 proof.
  The secret is input-only (`--secret-hex`) and is **never** echoed or stored.
- The **pool holds NO VRF secret** and **never generates or verifies** VRF proofs. It only
  fetches admitted proofs from the node and **bundles** them into the produced block ext.
- The **node** validates every V2 proof — at admission ingest AND at block acceptance — and
  is authoritative.

## Data flow

1. **Wallet/miner** (`irium-wallet poawx-candidate-admission --secret-hex …`): produces
   `AssignmentProofV2`, builds the candidate via `RoleCandidate::from_assignment_v2`
   (candidate `assignment_proof_digest` = VRF output ⇒ score derives from the VRF output),
   binds it into a `CandidateAdmissionV1` (`new_with_v2`), emits JSON + `wire_hex`, and
   optionally `--submit --node-rpc <loopback-url>` POSTs it.
2. **Candidate admission** (`CandidateAdmissionV1`): carries an optional trailing V2 proof —
   absent when the gate is off (byte-identical to pre-22E), required + bound into the
   admission digest when on. Canonical, bounded (`CANDIDATE_ADMISSION_MAX_BYTES`), and a
   mutation changes the digest/root.
3. **Node**: `ingest_bytes` → `validate` rejects malformed/wrong-net/height/role/solver/
   ticket/seed/output/proof before caching; stores admitted V2 candidates; the loopback RPC
   (`POST /poawx/candidate-admission`, `GET /poawx/candidate-admissions`) and P2P relay are
   length-agnostic, so they carry the V2-extended wire unchanged and export the admitted set
   deterministically (sorted) to the pool.
4. **Pool**: `refresh_pool_admitted_cache` fetches; `build_admitted_v2_proofs` /
   `build_pool_true_vrf_section` assemble the AVR2 section from the **selected** candidates'
   admitted proofs; `build_synthetic` / `build_collected` attach it and **fail closed** if a
   selected role lacks a valid V2 proof. Official fee-0 and third-party fee paths both work.
5. **Block validation**: `connect_block` re-validates the AVR2 section
   (`validate_block_true_vrf`) and the candidate set (`validate_block_candidate_sets`); a
   V1-only block is rejected when V2 is required; both fee paths pass with miner-supplied
   proofs.

## Reconciliation (key design point)

Under the true-VRF gate the candidate's `assignment_proof_digest` is the **VRF output**, not
the V1 placeholder hash. `RoleCandidate::validate_self` therefore **skips** the V1 placeholder
recompute when `true_vrf_active(height)` (penalty + effective-score checks still run); the VRF
binding is enforced instead by the `AssignmentProofV2` at admission ingest and at block
acceptance. This lets a single block satisfy BOTH candidate-set/admission enforcement and
true-VRF enforcement. With the gate off, `validate_self` is byte-identical to pre-22E.

## Gate behaviour

| `IRIUM_POAWX_TRUE_VRF_*` | network | behaviour |
|---|---|---|
| off (default) | any | V1 admissions/blocks accepted; AVR2 absent ⇒ byte-identical to pre-22E |
| activation+required | testnet/devnet | admission + block MUST carry a valid V2 proof bound to the candidate; V1-only rejected; pool fails closed without proofs |
| any | **mainnet (id 0)** | **hard-off** — gate never active |

## Tests

- Admission (lib `poawx_admission`): V2 accept; wrong net/height/role/solver/ticket/seed
  reject; mutated proof/output reject; V1-only rejected when V2 required; gate-off accepts V1
  (byte-identical); committed-admission root changes when the VRF output changes.
- Node/chain (`chain`): `phase22e_true_vrf_e2e_block` (ingest accepts valid V2 + rejects
  mutated; admitted set == block ext; one V2 block satisfies both candidate-set AND true-VRF
  validation; V1-only rejected); `phase22e_wrong_candidate_score_rejects`.
- Pool (`delegation`): `phase22e_pool_e2e_bundle_and_failclosed` (extract admitted proofs;
  candidate digest == VRF output; build AVR2 section; official fee-0 + third-party fee attach
  AVR2 and node deserializes; fail closed when a proof is missing).
- Wallet (`irium-wallet`): `phase22e_candidate_admission_v2_emit_and_submit` (V2 JSON +
  `true_vrf`; emitted wire decodes to a node admission whose proof verifies and binds; secret
  never leaks; submit flags accepted; mainnet disabled).

## Limits / next

- **Security review of `vrf_fun`/`secp256kfun` + the full wiring is still required before any
  non-test network.** Not mainnet-ready; no mainnet-ready claim.
- Public testnet, external audit, and on-chain governance/vote are **excluded** from this
  phase.
- `AssignmentProofV1` remains the labeled placeholder when the gate is off.
- Dependency tree: no `openssl` / `secp256k1-sys` / `bindgen`; the pool adds no VRF
  dependency (it treats the proof as opaque wire). Chain difficulty / LWMA-144 untouched.
