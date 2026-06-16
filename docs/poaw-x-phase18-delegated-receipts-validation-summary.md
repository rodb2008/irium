# PoAW-X Phase 18 — Delegated Receipts & Zero-Fee Rewards: Validation Summary

**Status:** Validation complete (Phase 18A–18D). This document is the docs-only
closeout (Phase 18E). No code changes.

**Branch:** `testnet/poawx-phase18-auto-receipts-zero-fee-rewards`
**HEAD at validation:** `4da6dd101ad50d4062c48caa16faf9cfd6514e10`
**Network scope:** testnet / devnet only. Mainnet delegated (mode-1) path is
consensus-gated and hard-rejected.

---

## 1. Objective

Allow a **stock, unmodified cpuminer** to mine a PoAW-X **mode-1 (delegated)**
block — removing the previous manual seeded-receipt step — while keeping the
**miner wallet as the sole payout identity** and running the official pool at a
**0% fee**, fully **non-custodial**. The pool never becomes a payout identity; it
only signs receipts on the miner's behalf under an explicit, one-time
delegation that travels inside the block so every peer can verify it.

---

## 2. Design model

- **Non-custodial one-time delegation.** The miner wallet signs a single
  delegation authorizing a pool delegate key to produce PoAW-X receipts on its
  behalf. The delegation is registered once (no private key leaves the wallet).
- **Stock cpuminer unchanged.** No miner-side signing, no custom fields. The
  miner connects, authorizes, and submits shares exactly as for any sha256d
  stratum pool.
- **Miner wallet remains the payout identity.** The coinbase pays the miner
  pkh. The pool delegate identity is never an output.
- **Pool delegate key is signer-only.** It exists solely to sign delegated
  receipts; it holds no funds and is never paid.
- **Official fee 0%.** `fee_bps == 0` is enforced; any `fee_bps > 0` fails
  closed (rejected) on the delegated path.

The delegated receipt embeds the canonical **226-byte delegation**, so an
independent node validates both the miner's delegation signature and the
pool's delegated-receipt signature during `connect_block`. The mode-0 (direct
miner-signed) path is preserved byte-identically.

---

## 3. Commit chain

| Commit | Phase | Summary |
|--------|-------|---------|
| `c8e1f64` | 18A | Design doc (delegated receipts + zero-fee model) |
| `51c686c` | 18B step-1 | Consensus primitives: `Delegation` 226B wire, v2 receipt section, mode-1 `connect_block` verify (gated, mainnet hard-off, `fee_bps>0` fails closed); mode-0 byte-identical |
| `a5c01cc` | 18B step-2 | Delegation registration: wallet `poawx-register` + pool loopback HTTP endpoints + signer-only delegate key + no-privkey delegation store |
| `d78f2b2` | 18B step-3 | Pool mode-1 receipt production from stored delegation via `/poawx/assignment` + grind + delegate sign; node carries delegation through pending→block + reorg |
| `4da6dd1` | 18C | Fix: `native_rewardable` `mining.notify` coinbase now carries the mode-1 `irx1` commitment so the miner mines the exact coinbase the validator expects |

---

## 4. Phase 18C — single-node E2E result

- Stock cpuminer, command unchanged.
- **No manual receipt seed** (no `poawx_pending_receipts.json`).
- **Mode-1 delegated block accepted** via two-phase bootstrap
  (node poawx-inactive → block 1 → poawx-active, activation height 2).
- **`submit_block_extended`** used (legacy `submit_block` returns 405 when
  PoAW-X is active).
- **`irx1_root` present** in the coinbase OP_RETURN commitment.
- **Miner paid**; **delegate not paid** (single p2pkh output).
- **Fee 0%.**
- **Mainnet/prod untouched**; ports clear after cleanup.

---

## 5. Phase 18D — two-node cross-VPS sync result

A mode-1 block produced on Node A was independently synced and validated by
Node B over a real cross-VPS P2P path (loopback peering is blocked by
anti-eclipse filters, so a genuine external path was required).

- **Node A** (producer) accepted block 2.
- **Node B** synced and **independently validated** the delegated receipt
  (reached the tip via `connect_block`; it would have stalled if validation
  failed).
- **Height:** 2
- **Block hash:** `000000009ba408944b3bf5571262e8b4ac707890fe9f293c9cdd0ecb0d071734`
- **irx1_root:** `018a745ba2e3318190edd1f1b196f4a65e084fa23b61179ea6d3863b56b47c08`
- **Embedded delegation present** (identical 226-byte delegation on both nodes).
- **Miner pkh:** `1f8f6380…`
- **Delegate not paid** (single p2pkh output).
- **Fee 0%.**

Block hash, merkle root, `irx1_root`, and the embedded delegation digest matched
byte-for-byte between the two independent nodes.

---

## 6. Safety proof

- **Mainnet untouched** on both hosts (seed-node mainnet processes alive,
  official binary at the stable path).
- **Production pool untouched.**
- **No push to main**, **no merge**, **no PR**, **no tag**.
- **Source-restricted UFW rule** (single host → Node A P2P port only) was
  **opened by the operator and removed**; rule verified absent afterward.
- **Test ports clear** after cleanup on both hosts.
- **Exact-pidfile cleanup** only (no `pkill`/`killall`); temporary run roots
  removed.

---

## 7. Operational findings

- A **standalone sync node** (copied binary, no repo checkout) must be launched
  from a working directory containing **`bootstrap/anchors.json`** and
  **`bootstrap/trust/allowed_anchor_signers`**; otherwise the node exits with
  "Failed to load anchors".
- **Genesis is embedded** in the binary, but **anchors are not** — they are read
  from `./bootstrap/` relative to the current working directory.
- On **testnet/devnet**, feed manual peers via node config **`p2p_seeds`** or env
  **`IRIUM_MANUAL_PEERS`** / `IRIUM_ADDNODE`. **`IRIUM_STATIC_PEERS` is
  mainnet-only** for the dialer and is ignored on non-mainnet networks.
- **Avoid `pkill -f`** against the node binary path over SSH: the pattern can
  match the invoking command's own argument list and terminate the session.
  Kill by exact PID after confirming `/proc/<pid>/cmdline`.
- **Activation height 2** was an isolated-E2E convenience only; it is **not** a
  mainnet activation value.

---

## 8. Remaining work after Phase 18

- Trusted external miner pilot (real third-party miner over the public path).
- Longer soak test (sustained block production and reorg behavior).
- Broader miner compatibility matrix (multiple cpuminer/ASIC firmware families).
- Public testnet runbook for external participants.
- Monitoring / metrics for the delegated-receipt path.
- Final governance and mainnet activation path — to be addressed much later,
  separately from this validation.
