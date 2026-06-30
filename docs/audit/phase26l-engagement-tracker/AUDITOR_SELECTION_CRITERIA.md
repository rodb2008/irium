# Auditor Selection Criteria

What to look for when choosing an independent reviewer for PoAW-X Phase 26. **No auditor has been
selected or contacted.** **NOT audited / production-ready / mainnet-ready.** Do not invent names —
these are criteria, not candidates.

## Must-have

- **Blockchain consensus experience** — has reviewed block-validation / fork-choice / finality logic
  and understands how a single gate weakness becomes a chain-level problem.
- **P2P / sync review experience** — comfortable reasoning about gossip, block/headers sync,
  request/serve paths, and DoS/resource bounds (directly relevant to admission serving).
- **Rust experience** — can read the node's Rust (ownership, async, error handling) at the level needed
  to verify the `connect_block` gates and the serving/persistence code.
- **Cryptographic protocol familiarity** — understands signatures, digests, and VRF outputs well
  enough to assess binding/replay/forgery, even if treating primitives as opaque is acceptable here.
- **Honest review of testnet/devnet code** — willing to scope findings to a pre-mainnet system and to
  state clearly what is and isn't covered, without overclaiming.
- **Written findings** — produces a written report with severities, reproduction, and recommendations
  (per `AUDIT_DELIVERABLES.md`).
- **Retest availability** — available to retest fixes and record a verdict, not just a one-shot report.

## Important

- **Timeline fit** — can start and deliver within the project's `[Timeline]`.
- **Cost fit** — estimate within `[Budget/Scope Notes]`.
- **Communication** — responsive, clear, and able to engage on disputes with evidence.
- **Reproducibility discipline** — runs the non-live repro commands and cites file:line at a specific
  commit.

## Conflict-of-interest check

- Not a contributor to the PoAW-X Phase 26 code or its design (independence).
- No financial stake in Irium tokens/rewards or in a launch outcome that biases the verdict.
- Disclose any prior engagements, affiliations, or relationships with the team.
- If any conflict exists, document it and decide whether it is disqualifying before engaging.

## Nice-to-have

- Public, citable prior audit reports in a comparable domain.
- Experience with multi-role / staking-style admission or committee-selection schemes.
- Familiarity with reproducible build/test review.

## How to use

1. Score candidates against Must-have first; drop any that miss a Must-have.
2. Run the conflict-of-interest check before serious discussions.
3. Compare on timeline/cost/communication.
4. Record the chosen reviewer's details via `OWNER_ACTIONS_REQUIRED.md` and proceed to
   `SEND_CHECKLIST.md`.

> Selecting a reviewer does not make the system "audited." Only the reviewer's scoped sign-off, after
> the review, supports that claim.
