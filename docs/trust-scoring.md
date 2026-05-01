# Irium Trust Scoring

## Overview

Every seller on the Irium network builds a reputation score derived from on-chain agreement history and proof submission records. Scores are computed locally on each node from imported agreement data — there is no central server.

## Trust Signals

| Signal | Description |
|--------|-------------|
| `completion_rate` | Percentage of agreements that resulted in a satisfied outcome. Computed as `satisfied / (satisfied + failed)`. |
| `dispute_rate` | Percentage of agreements that were disputed. Computed as `disputes / total_agreements`. |
| `avg_proof_response_time` | Average time in seconds between agreement creation and proof submission. Lower is better. |
| `risk` | Derived risk classification: `low`, `moderate`, or `high`. Based on default and dispute rates over lifetime history. |
| `recent_risk` | Same as `risk` but calculated over the last 10 agreements only. |

## Sybil Resistance

A seller's ranking score is suppressed until they have completed at least **3 verified agreements**. New sellers with fewer than 3 agreements display as "New seller -- not yet ranked". This prevents Sybil attackers from flooding the market with fake high-reputation accounts.

## Risk Classification

| Risk Level | Meaning |
|------------|---------|
| `low` | No defaults recorded in lifetime history. |
| `moderate` | Default rate <= 20% of total agreements. |
| `high` | Default rate > 20% of total agreements. |

A "default" is any agreement that ended in `timeout`, `unsatisfied`, or `failed` without a satisfactory proof.

## Ranking Score

The ranking score (0-80) determines offer ordering in `offer-list --sort score`:

- `+50` for lifetime risk = low
- `+30` for recent risk = low (last 10 agreements)
- `+20` for lifetime risk = moderate
- `+10` for recent risk = moderate

Sybil-suppressed sellers always score 0.

## Reputation Portability

Sellers can export their reputation using `reputation-export` and share the export file with any counterparty. The export includes a FNV-1a 64-bit content hash for tamper detection. Recipients verify with `reputation-import`.

Export fields are read-only snapshots -- they do not modify the recipient's local proof store.

## Abuse Prevention

- **Self-trade detection**: `reputation-self-trade-check --seller <addr> --buyer <addr>` detects when seller and buyer resolve to the same wallet address. Self-trade outcomes are flagged in the reputation record and excluded from ranking.
- **Minimum value threshold**: Agreements below the network minimum (configurable via `IRIUM_MIN_REPUTATION_VALUE`) are not counted toward reputation.
- **Rate limiting**: RPC endpoints enforce per-IP rate limits to prevent mass fake outcome submission.

## Plain-English Summary

`reputation-show` displays a one-line plain-English summary:

- `"Trusted -- 17 trades, 94.1% completion, no disputes"` -- good standing
- `"Caution -- 12 trades, 41.7% completion, 2 disputes (16.7% dispute rate)"` -- concerning
- `"New seller -- 2 trades (not yet ranked, minimum 3 required)"` -- below sybil threshold
- `"Risk: HIGH -- 10 trades, 30.0% completion, 3 disputes"` -- avoid

## Commands

```
irium-wallet reputation-show <seller_pubkey|address> [--json]
irium-wallet reputation-record-outcome --seller <addr> --outcome <satisfied|failed|disputed|timeout> [--proof-response-secs <n>] [--self-trade] [--json]
irium-wallet reputation-export --seller <addr> [--out <file>] [--json]
irium-wallet reputation-import --file <file> [--json]
irium-wallet reputation-self-trade-check --seller <addr> --buyer <addr> [--json]
```
