# PoAW-X Phase 18A — Delegated Receipts & Official Zero-Fee Design

Status: DESIGN ONLY (no code yet). Branch: `testnet/poawx-phase18-auto-receipts-zero-fee-rewards`.
Scope: testnet/devnet only. Mainnet PoAW-X remains disabled/hard-gated.

This document is the agreed design for removing the manual seeded-receipt step so the
official Irium pool can automatically create valid PoAW-X receipts for a miner using a
**stock cpuminer**, while keeping the pool **non-custodial** and **0% fee**, and keeping
the miner wallet as the **only** payout identity.

---

## 1. Problem statement: stock cpuminer cannot sign receipts

A rewardable PoAW-X block requires, per receipt, two things bound to the miner's payout key:

- `verify_worker_identity` (`src/bin/iriumd.rs:13986`): a secp256k1 signature whose pubkey
  `HASH160` equals the receipt `worker_pkh`, over `challenge = SHA256(solution || commitment_nonce || height_le8)`.
- `poawx_validate_reward_split` (`src/bin/iriumd.rs:13946`): the coinbase must pay that same
  `worker_pkh` at least `worker_due = subsidy * POAWX_WORKER_REWARD_PERMILLE/1000` (10%/receipt).

The `commitment_nonce` is deterministic from the parent block and the 8-byte puzzle
`solution` is trivially grindable (min difficulty `POAWX_MIN_ACTIVE_DIFFICULTY_BITS = 4`),
so the pool can compute both. The **only** thing the pool cannot produce is the signature
by the miner's private key.

The stratum native submit (`NativeSubmit`, `pool/irium-stratum/src/stratum.rs:332`) carries
only `job_id / extranonce2 / ntime / nonce`. A stock cpuminer authenticating with
`mining.authorize <address>.<worker>` proves it knows the address *string*, not the private
key, and supplies no pubkey and no signature. Therefore the pool has no signing material and
cannot, by itself, mint a valid receipt for an arbitrary external miner. The manual seed
(`poawx_pending_receipts.json` loaded at `src/bin/iriumd.rs:17298`) exists only to inject an
operator's pre-signed receipt — this is the step Phase 18 removes.

## 2. Why pool-custodial identity is rejected

A pool could hold its own key, sign receipts as itself, and have the coinbase pay the pool,
then credit miners off-chain. This is rejected by policy:

- The official Irium pool must be **non-custodial** and **zero-fee**.
- Miners must be rewarded **directly to their wallet address**.
- The pool must **never** become the payout identity; `pool_pkh` must **never** be a
  `worker_pkh` for official rewards, and the pool must not custody rewards.

## 3. Why direct miner-signed receipts remain supported but are not enough

The existing path (here called **mode 0**) — the miner signs its own receipt — stays fully
valid and unchanged for future wallet-integrated / custom miners that can sign. But a
**stock cpuminer** cannot sign, so mode 0 alone cannot serve the common case. Mode 0 is
preserved for backward compatibility; mode 1 (delegated) is added for stock miners.

## 4. One-time delegation model

The miner wallet signs a **one-time delegation** authorizing a specific pool delegate key to
create PoAW-X receipts on the miner's behalf, within bounds. Key properties:

- Miner wallet remains the payout identity; coinbase pays the **miner** pkh.
- Delegation is signed by the miner key and **never** contains or exposes a private key.
- The pool signs only the per-height receipt challenge, and only under a valid delegation.
- The node verifies **both** the miner's delegation signature **and** the pool delegate's
  receipt signature — for every full node during normal validation/sync, not just the
  submitting pool.

## 5. Out-of-band wallet registration flow

One time, from a wallet/CLI/UI holding the payout private key:

```
irium-wallet poawx-register \
  --pool <pool-url> --addr <miner-address> --worker <worker> \
  --expiry-height <N> [--fee-bps 0]

  1. unlock wallet -> miner_pubkey, miner_pkh (== HASH160(miner_pubkey))
  2. GET <pool-url>/poawx/pool-identity -> pool_pubkey, network_id, fee_policy (=0 official)
  3. build Delegation (section 9), sign m_deleg (section 10) with the miner key
  4. POST <pool-url>/poawx/delegation { delegation, miner_pubkey, delegation_sig }
```

The private key never leaves the wallet. Every mine afterwards uses a stock cpuminer
unchanged:

```
minerd -o stratum+tcp://<pool> -u <miner-address>.<worker> -p x
```

Operator-placed delegation files are allowed **only** as a devnet/test fallback. The
stratum password token is **not** the primary design.

## 6. Pool identity / delegate key role

The pool holds its own secp256k1 keypair = the **pool identity / delegate key**. This key:

- is published as `pool_pubkey` via `GET /poawx/pool-identity`,
- is the key a delegation authorizes (`delegation.pool_pubkey == pool_pubkey`),
- is used **only** to sign the per-height receipt challenge,
- is **never** a payout identity and **never** appears in any coinbase output.

## 7. Delegation registry / storage design

Pool-local persistent storage, behind a `DelegationStore` trait:

- Devnet impl: write-through JSON at the pool state dir, e.g. `poawx_delegations.json`,
  keyed by `"<miner_pkh_hex>.<worker>"`, value = the verified Delegation record plus
  `received_at` and `status`.
- In-memory cache for lookup on the hot path.
- DB-ready: the trait allows a sqlite backend later (matching `irium-pool-api`).

On `POST /poawx/delegation`, the pool verifies the delegation (section 14, pool-side subset)
before storing: signature valid, `HASH160(miner_pubkey) == addr pkh`, `network_id` matches,
`pool_pubkey == self`, fee terms valid (official = 0), `expiry_height > current tip`.

The registry is the **source/lookup** layer. The relevant delegation is **copied into the
block receipt** at block-build time (section 8/9) so peer nodes can re-verify independently.

## 8. Receipt mode-0 vs mode-1 wire format

The receipt section is appended after transactions, preceded by the magic
`POAWX_RECEIPT_SECTION_MAGIC` (`src/poawx.rs:34`). To preserve byte-identity for existing
data, a **section version** discriminates:

- **v1 section (mode-0 only):** unchanged. Each receipt is the fixed 166-byte
  `PoawxBlockReceipt` (`src/poawx.rs:43`). Pre-activation blocks and direct-signed receipts
  use this exactly as today.
- **v2 section:** introduces a per-receipt `mode` byte. Mode 0 within v2 carries the same
  logical fields as today; mode 1 carries the delegated layout below. All fields are
  fixed-size, so each receipt remains length-deterministic.

Mode-0 receipt (direct, unchanged semantics):
`height(8) | lane(1) | worker_pkh(20) | worker_pubkey(33)=miner | worker_sig(64)=miner-over-challenge | solution(8) | commitment_nonce(32)`.

Mode-1 receipt (delegated):
`height(8) | lane(1) | worker_pkh(20)=miner | signer_pubkey(33)=pool delegate | signer_sig(64)=pool-over-challenge | solution(8) | commitment_nonce(32) | <Delegation blob, section 9>`.

The JSON forms (`PoawxPendingReceipt` in `src/bin/iriumd.rs:1520`,
`pool/irium-stratum/src/template.rs:49`, `pool/irium-stratum/src/block.rs`) gain the
delegation fields as `#[serde(default)]` so v1 entries load as mode 0.

## 9. Embedded delegation fields

The `Delegation` blob embedded in a mode-1 receipt (all multi-byte LE):

| field | size | meaning |
|---|---|---|
| `deleg_version` | 1 | delegation format/version |
| `network_id` | 1 | chain/network bind (devnet/testnet/mainnet) |
| `miner_pubkey` | 33 | `HASH160 == worker_pkh` (payout identity) |
| `pool_pubkey` | 33 | authorized delegate key; must `== signer_pubkey` |
| `worker_tag` | 32 | `SHA256(worker_name)` (or session id); zero if unscoped |
| `expiry_height` | 8 | last valid block height |
| `fee_bps` | 2 | fee policy; official = 0 |
| `fee_pkh` | 20 | required only if `fee_bps > 0`, else zero |
| `deleg_nonce` | 32 | replay/domain uniqueness |
| `delegation_sig` | 64 | miner ECDSA over `m_deleg` |

## 10. Signing messages and domain separators

Domain separator constant (new, in `src/poawx.rs`): `DOMAIN_DELEG = b"irium.poawx.delegation.v1"`.

Delegation message signed by the miner:
```
m_deleg = SHA256(
  DOMAIN_DELEG || deleg_version || network_id || miner_pubkey || pool_pubkey ||
  worker_tag || expiry_height_le8 || fee_bps_le2 || fee_pkh || deleg_nonce )
delegation_sig = ECDSA_sign(miner_priv, m_deleg)      // verify_prehash
```

Receipt challenge (unchanged; signer = pool delegate in mode 1, miner in mode 0):
```
challenge = SHA256(solution || commitment_nonce || height_le8)
signer_sig = ECDSA_sign(signer_priv, challenge)
```
Keeping the existing challenge keeps mode-0 verification byte-identical. Per-receipt domain
separation is provided by the per-height deterministic `commitment_nonce`.

## 11. Replay protection

- `deleg_nonce(32)` is unique per delegation.
- Binding to `network_id`, `miner_pkh`, `pool_pubkey`, `worker_tag` prevents cross-chain /
  cross-miner / cross-pool / cross-worker reuse.
- `expiry_height` bounds the validity window.
- Per-receipt freshness is enforced by the per-height `commitment_nonce` and `solution`,
  so a delegation alone cannot mint a receipt without a fresh per-height signed puzzle.

## 12. Expiry-height behavior

A mode-1 receipt is rejected if `block.height > delegation.expiry_height`. Height-based
expiry is deterministic across all nodes. The pool prunes expired delegations from the
registry; the node simply rejects expired ones during validation.

## 13. Fee terms and official 0% policy

- Official pool: `fee_bps = 0`, no `fee_pkh`, single coinbase output to the miner. Startup
  asserts official mode implies `pool_fee_bps == 0`. The delegation's `fee_bps` must be 0;
  the node enforces it on the official path.
- Third-party pools (later, explicit): `POOL_FEE_BPS` (0..=10000) and required valid
  `POOL_FEE_ADDRESS` when `> 0`, logged transparently; invalid combos refuse startup. A
  non-zero fee emits a second coinbase output to `fee_pkh`; the **delegation must carry
  matching `fee_bps`/`fee_pkh`** so the miner consented, and the node validates the coinbase
  fee output against the delegation terms. The fee-split rule is consensus-visible and gated.
- `irium-pool-api` remains observability only (no fee deduction, no payout engine).

## 14. Node verification flow

In `submit_block_extended` (`src/bin/iriumd.rs:14208`) and the `connect_block` receipt path
(`src/chain.rs`, around `:1957`/`:2034`) so peers re-validate on sync. Per receipt:

- Branch on mode.
- **Mode 0:** existing `verify_worker_identity` + puzzle PoW + reward split (unchanged).
- **Mode 1** (only when the activation gate is on and network is non-mainnet):
  1. `network_id` matches the node network.
  2. `HASH160(miner_pubkey) == worker_pkh`.
  3. `delegation_sig` verifies against `miner_pubkey` over `m_deleg`.
  4. `block.height <= expiry_height`.
  5. `signer_pubkey == delegation.pool_pubkey`; `signer_sig` verifies over `challenge`.
  6. Fee policy: official => `fee_bps == 0`; if `fee_bps > 0`, coinbase must contain the
     matching `fee_pkh` output for the fee portion.
  7. Puzzle PoW unchanged: `sha256d(seed || commitment_nonce || solution)` has
     `leading_zero_bits >= difficulty`.
  8. Reward split unchanged: coinbase pays `worker_pkh`(miner) >= `worker_due`.
- The mainnet receipt hard-rejection (`src/bin/iriumd.rs` around `:14246`) stays as the outer
  gate.

To keep the embedded delegation tamper-evident, the **mode-1 irx1 inner hash includes a
`delegation_digest`**; the **mode-0 inner hash is byte-identical to today**
(`SHA256(height_le8 || lane_byte || worker_pkh || solution || commitment_nonce)`). All three
root implementations must agree: `src/poawx.rs:146`, `src/bin/iriumd.rs:13739`,
`pool/irium-stratum/src/block.rs:281`.

## 15. Coinbase payout rule — miner pkh gets paid, not pool pkh

The native rewardable coinbase (`pool/irium-stratum/src/stratum.rs:1517`) pays the full
subsidy to `session.pkh` = the miner address from the stratum username, in a single output
(`build_canonical_job_snapshot` comment at `:1375`). This is unchanged.
`poawx_validate_reward_split` confirms the coinbase pays `worker_pkh`(miner) >= 10%/receipt;
at 100% to the miner this always holds. `pool_pkh` appears in no output. In official mode the
coinbase has exactly one payout output (plus the zero-value irx1 OP_RETURN), 0% fee.

## 16. Pool-side auto receipt creation flow

On an authorized session for `<miner-address>.<worker>` with PoAW-X active:

1. Look up a valid delegation for `(miner_pkh, worker)` in the registry.
2. If found: derive the deterministic `commitment_nonce` (from the assignment seed), grind
   the 8-byte `solution` to meet difficulty, build a mode-1 `PoawxPendingReceipt` with
   `worker_pkh = miner`, `signer_pubkey = pool delegate`, `signer_sig` over the challenge,
   and the embedded delegation copied from the registry.
3. Commit the receipts root as the coinbase `irx1` OP_RETURN
   (`compute_receipts_root_from_pending`), build the native rewardable coinbase paying the
   miner, and submit via `submit_block_extended` (no manual seed).
4. If no delegation exists: the session still mines normally (shares/vardiff unchanged) but
   produces a non-rewardable share — stock cpuminer is unaffected.

## 17. Backward compatibility

- Mode-0 wire, root, and validation are byte-identical to today; the existing direct
  miner-signed path and the current test suite stay valid.
- New JSON fields are `#[serde(default)]`; existing `poawx_pending_receipts.json` entries load
  as mode 0.
- Existing clear-on-accept / not-lost-on-reject / reorg-restore / prune-expired behavior is
  reused unchanged.

## 18. Mainnet activation / gating

- New `POAWX_DELEGATION_ACTIVATION_HEIGHT` (testnet/devnet). Below it, or on mainnet, mode-1
  receipts are rejected; mode-0 is unaffected.
- New `deleg_version` / receipt section v2 version constants.
- Mainnet keeps the existing blanket receipt rejection; `cpuminer_compat` stays
  non-rewardable; no variant-sweep block promotion.
- No state wipe/migration required (forward/back compatible via versioning + serde defaults).

## 19. Test plan

Unit:
- delegation sign/verify; HASH160 binding; wrong-miner / mismatched pkh rejected.
- expiry-height boundary (valid at `==`, rejected at `>`).
- replay: nonce/network/pool/worker binding rejects reuse.
- mode-1 irx1 root; root parity across `poawx.rs` / `iriumd.rs` / `block.rs`.
- mode-0 unchanged (regression).
- clear-on-accept (only committed height); not-lost-on-reject (pending file intact + reason).
- duplicate / malformed / stale receipt handling.
- official fee = 0 config; third-party fee config valid; invalid fee config rejected.

Stratum:
- builds a delegated mode-1 receipt and submits via `submit_block_extended` with **no manual
  seed**; no delegation => non-rewardable share, mining still works.

E2E devnet (single node):
- real cpuminer at `/home/irium/phase13-devnet/cpuminer-src/minerd`, no seed, wallet
  delegation registered, auto receipt, `submit_block_extended`, `BLOCK_ACCEPTED`,
  `irx1_root` present, miner address recorded as payee, fee = 0.

Two-node E2E (only after approval + operator firewall handoff):
- Node A accepts, Node B syncs same height/hash/`irx1_root`, both verify delegation +
  receipt signatures; fee = 0 verified.

## 20. Files expected to change during implementation

Consensus:
- `src/poawx.rs` — Delegation type, sign/verify, receipt v2 wire (mode-0/mode-1),
  `DOMAIN_DELEG`, mode-1 irx1 root, `delegation_digest`.
- `src/bin/iriumd.rs` — receipt struct fields, `compute_poawx_receipts_root` mode-1,
  `submit_block_extended` mode-1 verification, pending<->block mappers, load/save,
  getblocktemplate echo, new testnet-gated `POST /poawx/delegation` and
  `GET /poawx/pool-identity`, activation gate.
- `src/chain.rs` — `connect_block` mode-1 verification for sync; irx1 root checks.
- `src/activation.rs` / `src/constants.rs` — activation gate constant, mainnet hard-off.

Node tool (non-consensus):
- `src/bin/irium-wallet.rs` — `poawx-register` delegation-signing subcommand.

Pool:
- `pool/irium-stratum/src/stratum.rs` — pool delegate key load, registry lookup, auto mode-1
  receipt build, fee config + official zero-fee assert, submit path.
- `pool/irium-stratum/src/block.rs` — `compute_receipts_root_from_pending` mode-1; optional
  third-party fee coinbase output.
- `pool/irium-stratum/src/template.rs` — delegation fields (serde).
- `pool/irium-stratum/src/main.rs` — env parsing (`POOL_FEE_BPS`/`POOL_FEE_ADDRESS`, delegate
  key, official mode).
- `pool/irium-stratum/src/delegation.rs` — NEW: `DelegationStore` trait + JSON impl + verify +
  the two route handlers.

Docs:
- this file; plus runbook updates at implementation time.

## 21. Implementation phases

1. `src/poawx.rs` consensus primitives (Delegation, sign/verify, wire v2, mode-1 root) + unit
   tests (TDD). Show diff for sign-off before applying (consensus-sensitive).
2. Node: struct fields, root, `submit_block_extended` + `connect_block` verification,
   activation gate, `/poawx/delegation` + `/poawx/pool-identity`, registry load/save + tests.
3. Wallet: `irium-wallet poawx-register` + tests.
4. Pool: template fields, delegate key, registry, auto mode-1 receipt build, root parity +
   tests.
5. Fee config: official zero-fee assert + third-party validation + coinbase output + node
   validation + tests.
6. Stratum integration tests (no seed).
7. E2E devnet single node.
8. (Approval) two-node E2E.
9. Docs / runbook refresh.
