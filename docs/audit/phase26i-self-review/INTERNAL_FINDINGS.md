# PoAW-X Phase 26 — Internal Findings (Phase 26I self-review)

Internal, self-found observations only. **Not an independent audit; not a sign-off.** No Critical or
High issue was found. None of these weaken a validation gate, change consensus, or break sync — so no
source change is proposed in this phase. Items flagged "Needs Auditor Review" are deferred to the
external reviewer (they are properties/limitations, not confirmed defects).

## Status legend

`Open` · `Confirmed` · `Needs Auditor Review` · `Accepted Risk` · `Fixed Later` · `Not an Issue`

## Findings

| ID | Severity | Title | Status | Affected file / function | Description | Recommendation | Auditor should review? |
|----|----------|-------|--------|--------------------------|-------------|----------------|------------------------|
| SR-001 | Informational | phase21e propagation-sensitivity (pre-existing) | Needs Auditor Review | `src/chain.rs` `validate_block_candidate_sets` (phase21e); `src/poawx_admission.rs` window logic | phase21e proves "best among candidates admitted to THIS node within the window (64)", not "best among all unseen offline miners". Unchanged by Phase 26; persistence/serving improve *availability* of those admissions but do not change the property. | Assess the consensus implications for a public, untrusted network; decide whether the window/propagation model is acceptable for testnet vs needs strengthening before any wider use. | **Yes** |
| SR-002 | Informational | Serving multiplier `16×block_count` not tied to role count | Needs Auditor Review | `src/p2p.rs` `send_historical_admissions` (`:6704`) | The per-response cap is `block_count * 16`. A height has a bounded number of roles (Primary/Compute/Verify/Support), so 16× is a generous but fixed upper bound. The bound is correct (DoS-safe) but the constant is not derived from the role count. | Confirm 16× is a safe, intentional upper bound vs the max legitimate admissions-per-height; consider documenting the derivation or tightening the constant. | **Yes** |
| SR-003 | Informational | Admission window = 64; deep-chain / scale sync unproven | Needs Auditor Review | `src/poawx_admission.rs` window/prune; `src/p2p.rs` serving | Live validation was a 6–7 block, three-node devnet. Behavior for deep chains, many heights behind, or large admission volumes is untested. Known limitation, documented in the kickoff guide. | Treat as a public-testnet test objective; auditor to opine on window sizing and prune behavior under load. | **Yes** |
| SR-004 | Informational | Multi-block-from-scratch getblocks can briefly stall before handshake-push | Needs Auditor Review | `src/p2p.rs` sync/handshake-push paths (pre-existing) | Per prior phases, a fresh node's getblocks may show a transient "stalled" before handshake-push (~30–45s) delivers blocks + admissions. Orthogonal to the consensus change; affects time-to-sync, not validity. | Auditor to assess sync robustness/recovery; candidate for P2P hardening before public testnet. | Yes |
| SR-005 | Informational | Pre-existing cosmetic compiler warnings | Confirmed | `src/chain.rs:9237` (unused `committee`), `src/poawx.rs:2346` (self-assign `ca2.digest = ca2.digest`) | Build emits 4 warnings; the two notable ones are in test/helper code and have no runtime effect. Not introduced as defects by Phase 26 consensus logic. | Optional cleanup (prefix `_committee`; remove the self-assign or comment intent) in a later docs/cleanup pass — only with approval, since the rule is no silent source edits. | No |
| SR-006 | Informational | Single-reviewer self-review (independence gap) | Accepted Risk | n/a (process) | This review is by the same party that wrote the code; it cannot replace independent review. | Proceed to independent audit using the Phase 26H kickoff package before any "audited" claim or public-testnet launch. | **Yes (by definition)** |

## Notes

- No finding triggered the Phase 26I stop-and-report condition (no validation/consensus/sync break).
- All "Needs Auditor Review" items are existing properties or limitations already disclosed in
  `docs/audit/phase26h-kickoff/AUDITOR_REVIEW_GUIDE.md` ("Known limitations") — recorded here so the
  external auditor can confirm or challenge them rather than rediscover them.
- No secrets, keys, wallet data, or raw private logs are included in any entry.
