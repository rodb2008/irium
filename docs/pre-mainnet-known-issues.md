# Pre-Mainnet Known Issues (must-fix before mainnet)

## 1. Single-peer IBD completion stall  (severity: HIGH — onboarding/liveness)

**Symptom.** A node performing initial block download / resync from a SINGLE peer over a
flapping connection cannot reliably COMPLETE the sync. It downloads a prefix of the chain,
then hard-stalls: it holds the headers up to the peer tip and repeatedly issues getblocks for
the next range, but the block bodies are never delivered/connected, while the stall-recovery
path keeps clearing transient headers and thrashing without progress. A node restart does not
help; the node falls progressively further behind.

**Observed.** 2026-06-29, resyncing eu (freshly wiped) from vps as its only peer (the third
node firewalled off): eu synced 0 to 1556 in bursts, then stuck hard at 1556 while vps was at
1614+. eu repeatedly logged "requesting N blocks (range=[1557-1614])", "WARN stalled ...
clearing sync throttles", "Cleared NNNN transient headers after sync stall", and intermittent
"peers=0". eu's 0-1556 matched vps exactly (correct chain) — purely a completion/delivery stall.

**Impact for mainnet.** A node with one (or few, flapping) peers may never finish IBD and so
cannot join the network — a real liveness/onboarding risk, independent of the consensus fixes.

**Fix direction.**
- Make getblocks request/serve + stall-recovery robust to a single flapping peer: do NOT clear
  in-flight headers prematurely on a transient stall; persist/resume getblocks progress across
  reconnects; exponential backoff instead of thrash; consider smaller batch sizes under stall.
- Verify the SERVING side actually fulfills getblocks for the requested range (body delivery,
  not just headers).
- Add an integration test: a wiped node must complete IBD from a single intermittently-dropping
  peer within a bounded number of rounds.

**Discovered during:** the 2026-06-29 3-node adversarial soak recovery, after an external
participant (mis-configured miner: audit-root mismatch then reject/retry then candidate-admission
flood) split the honest pair; this stall blocked the minority-node resync.
