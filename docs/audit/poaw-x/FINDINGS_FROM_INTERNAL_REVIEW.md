# PoAW-X — findings from the internal review (Phase 23A)

Source: `docs/poaw-x-phase23a-true-vrf-internal-security-review.md` (internal Claude Code
review of the true-VRF dependencies + AssignmentProofV2 wiring). **Internal only — NOT an
independent audit.** Provided here so the external auditor can confirm/extend or refute.

## Summary

- **No Critical, High, or Medium findings.** No consensus change was required.
- All required negative tests are present; Phase 23A added bounded-deserialization /
  malformed-input tests for the V2 proof, the ext `AVR2` section, the candidate-admission
  trailing wire, and the pool opaque mirror.
- **Risk: LOW for local/devnet only.** NOT acceptable for public/non-test/mainnet without an
  external audit.

## Findings

| ID | Severity | Area | Description | Fixed? |
|---|---|---|---|---|
| 23A-INFO-1 | Informational | dependency | `vrf_fun`/`secp256kfun` are pre-1.0 (0.12.x) and not formally audited; ECVRF correctness assumed pending external audit. | n/a — external audit |
| 23A-INFO-2 | Informational | dependency | `secp256kfun` vendors k256 field arithmetic (`src/vendor/k256`); include in audit scope. | n/a — audit scope note |
| 23A-LOW-1 | Low | admission | "best among candidates admitted to THIS node in the window" is propagation-sensitive (documented honest limitation); needs public-network review/tuning. | n/a — testnet review |
| 23A-INFO-3 | Informational | dependency | One `unsafe` in the proc-macro (NonZero marker) + `subtle-ng` (constant-time); standard, no FFI. | n/a |

## What the internal review verified (for the auditor to confirm)

- Message binding of all 8 fields; verification rejects wrong message/key/proof/output/digest.
- Replay/substitution resistance per bound field (height/role/miner/ticket/seed/key).
- Canonical fixed 273-byte wire + exact-length 81-byte bincode `VrfProof`; malformed rejected.
- Score = first-8-bytes-LE of the VRF output; deterministic; integer-only.
- Wallet secrets input-only, never echoed (test-asserted).
- Node-authoritative validation at admission ingest + `connect_block`; pool holds no secret,
  fail-closed; committed-admission root binds the VRF output.
- Mainnet hard-off across all gates.

## Internal verification results (at Phase 23A)

`cargo tree` no `openssl`/`secp256k1-sys`/`bindgen`/`native-tls`; fmt clean (lib + pool);
poawx 128/0, phase20 33/0, reward 9/0, wallet 428/0, iriumd bin 256/0, pool 96/0 (delegation
42, native_rewardable 6).
