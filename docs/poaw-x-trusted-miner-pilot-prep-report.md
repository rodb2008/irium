# PoAW-X Trusted Miner Pilot — Prep Report

**Date:** 2026-06-15
**Type:** Preparation only — pilot NOT started, no miners invited.

---

## 1. Branch / Hash

- Branch: `origin/testnet/poawx-phase12-completion-rc-hardening`
- HEAD (before this doc): `a0aedc6` (pushed; commit signed & GitHub-verified under the configured signing identity)
- Node version: v1.9.115 (PoAW-X + MTP; reconciled with official main `5d4604c`)
- Validation: Phase 14-F — two-VPS E2E 74/74 PASS (standard + lane="cpu" B-1 live proof); 1588/1588 unit/integration tests; both mainnets on isolated official binaries.

## 2. Docs Created / Updated

Created (this prep):
- `docs/poaw-x-trusted-miner-pilot-runbook.md` — purpose, scope, environment plan, preflight, flow, success/stop, rollback, privacy, go/no-go.
- `docs/poaw-x-trusted-miner-pilot-invite-template.md` — sanitized invite with placeholders only.
- `docs/poaw-x-trusted-miner-operator-checklist.md` — branch/mainnet/process/port/RPC/firewall/stratum/receipt/irx1/P2P/log/result/shutdown checks + sanitized monitoring commands.
- `docs/poaw-x-trusted-miner-acceptance-criteria.md` — must-pass / disqualifiers / sign-off.
- `docs/poaw-x-trusted-miner-stop-conditions.md` — immediate stop conditions + stop procedure + escalation.
- `docs/poaw-x-trusted-miner-pilot-prep-report.md` — this report.

Pre-existing (reviewed, still useful as background; superseded for the trusted pilot by the above):
- `docs/poaw-x-limited-miner-pilot-guide.md` (Phase 11-E) — stratum 39512, cpuminer-multi flow.
- `docs/poaw-x-real-miner-pilot-invite.md` (Phase 11-F) — earlier invite; predates Phase 12–14.
- `docs/poaw-x-phase14f-post-remediation-two-vps-validation.md` — current validation evidence.
- `docs/STRATUM_SETUP.md`, `docs/POOL_STRATUM.md`, `docs/SOLO_STRATUM.md` — stratum background (note: live stratum services on the host are MAINNET pool services; the pilot uses a SEPARATE testnet stratum on 39512).

## 3. Environment (summary; full plan in runbook §3)

- Testnet node: devnet binary (repo target / isolated devnet path) — **not** the mainnet service binary.
- Ports: stratum **39512** (exposed to invited miner only), P2P **39510** (optional seed), RPC **39511** (PRIVATE), status **39508** (PRIVATE).
- Devnet env: `IRIUM_NETWORK=devnet`, `IRIUM_POAWX_MODE=active`, activation height 1, difficulty 4 bits.
- Data dirs under `$HOME`; no mainnet collision (mainnet uses 38300/8080/38291 + `/home/irium/.irium`).
- Binary isolation verified (Phase 14-F): dev/testnet builds cannot overwrite either mainnet service binary.

## 4. Remaining Information Needed Before Inviting a Miner

These must be decided/provided privately (never committed):
- **Trusted miner identity + private contact channel** (`CONTACT_METHOD`).
- **`PILOT_HOST`** — which host/IP exposes the testnet stratum (and approval to expose `STRATUM_PORT`).
- **`START_TIME`** and **`DURATION`** for the session.
- **`TESTNET_WALLET_OR_WORKER_NAME`** convention to hand the tester.
- Confirmation of **firewall rule** allowing only `STRATUM_PORT` (39512) from the miner, with 39511/39508 blocked publicly.
- Decision: run the testnet node/stratum on which host without affecting co-located mainnet/pool services (resource headroom check).

## 5. Safety Blockers (must be clear at launch time)

- Testnet node + testnet stratum not yet started (intentional — prep only).
- Firewall rule for `STRATUM_PORT` not yet applied/verified (operator + sudo, at launch).
- RPC 39511 private-from-public must be re-verified at launch.
- Explicit launch approval not yet given.

None of these are code blockers; they are operational steps to perform at launch under the runbook/checklist.

## 6. Go / No-Go

- Code/validation: **GO** (branch validated, pushed, signed; mainnets isolated and healthy).
- Operational launch: **NO-GO until** Section 4 info is provided, Section 5 blockers cleared, and explicit approval given.

## 7. Can the pilot start after explicit approval?

**Yes** — once: (a) a trusted miner + private channel are set, (b) `PILOT_HOST`/`START_TIME`/`DURATION` decided, (c) the testnet node + stratum are brought up per the runbook with the operator checklist passing (including firewall + RPC-private verification), and (d) explicit launch approval is given. Mainnet remains untouched; no public testnet.
