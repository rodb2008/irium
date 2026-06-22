# Retest Protocol

What must pass before a fix is offered for auditor retest, and how evidence is recorded. **Process only
— no findings exist yet.** **NOT audited / production-ready / mainnet-ready.**

## Always required (every finding that touches code)

1. **Focused tests** for the affected area, e.g.
   `cargo test <name> --lib -- --test-threads=1` (single substring filter; the multi-filter form is
   not supported).
2. **At least one negative test** proving the gate still **rejects** the bad case the finding
   describes (e.g. block without matching admitted set → phase21e error). A fix without a negative test
   is incomplete.
3. **Full serialized lib suite:** `cargo test --lib -- --test-threads=1` — must be all-pass. Run
   serialized: PoAW-X tests mutate process-global env + the global admission cache; one pre-existing
   test (`phase24k_native_pow_all_gates_validators`) is parallel-only flaky.
4. **Release build:** `cargo build --release --bin iriumd --bin poawx-live-proof-harness` → exit 0.

## Required live devnet validation (conditional)

Required when the finding touches **P2P, sync, persistence, or consensus-adjacent logic** (e.g.
admission serving/ingest, persisted-admission reload, candidate-set/seed handling). **This is a
stop-and-ask step — a live run needs explicit approval before execution.** Constraints when approved:

- Devnet only (`IRIUM_NETWORK=devnet`), PoAW-X gates active; **mainnet hard-off** confirmed.
- **Loopback-only RPC**; cross-host P2P source-restricted (no `0.0.0.0/0`, no broad firewall rule).
- **Isolated storage** under a dedicated test data root — never `/tmp`, never `~/.irium` /
  `%USERPROFILE%\.irium`.
- Start/stop nodes by **exact pidfiles/PIDs only**; never `pkill`/`killall`. Protect mainnet PIDs and
  the production pool.
- No sudo/firewall changes; no secrets printed or stored.
- Validate the specific behavior the finding concerns (e.g. re-sync reaches tip; tampered admission
  rejected on the wire; restart reload re-validates) and confirm propagation/validity across nodes.

## Evidence capture format

Record in the finding record (`FINDING_RECORD_TEMPLATE.md`) and summarized in the dashboard:

```
### Retest evidence — F-NNN
- Commit(s): <local sha> (landed remote: <sha if VPS-1 fallback>)
- Local tests:
  - <command> -> <N passed / 0 failed>
  - negative: <command> -> <test name> ... ok
- Full suite: cargo test --lib -- --test-threads=1 -> <N passed / 0 failed>
- Release build: cargo build --release ... -> exit 0
- Live devnet (if run, with approval ref): <what was validated>, nodes at height <H>, tip <hash>,
  mainnet+prod untouched, ports closed, storage isolated. (Logs summarized; no secrets.)
- Date / operator: <date> / <name>
```

Rules for evidence:
- Summarize logs; **never paste raw logs containing secrets, keys, addresses with funds, or machine
  credentials.**
- Prefer reproducible, repo-local test output over live output where both prove the point.
- Quote exact commands so the auditor can re-run them.

## Recording the auditor retest

- The auditor independently re-runs the relevant checks and records a verdict (Pass / Fail / Partial)
  in the finding record and the findings tracker.
- A finding moves to **Closed** only on auditor **Pass** or on explicit, documented project decision
  (Won't Fix / Accepted Risk) — never on the project's own retest alone.
- Update `AUDIT_STATUS_DASHBOARD.md` after each retest result.
