# Phase 12-J: RC Security & Consensus Boundary Audit

**Status:** Complete (documentation, no code changes)
**Branch:** testnet/poawx-phase12-completion-rc-hardening
**Audit base commit:** c4119b9 + 64fa624 (Phase 12-I, not pushed)
**Audit date:** 2026-06-12
**Auditor:** Claude Sonnet 4.6 (automated, on behalf of Irium Labs)

---

## 1. Scope

This is a no-code documentation audit assessing whether the current PoAW-X
implementation is ready for localhost/private live-node RC testing and what must
be fixed before any controlled real miner pilot. All hard rules remain in force:
testnet only, no mainnet touch, no push, no PR, no real miners.

---

## 2. Consensus Boundary Audit

### 2.1 Validation enforced in `connect_block` (chain.rs)

Called from: `chain.rs:ChainState::connect_block()` at line 832.

| Check | Function | What it enforces |
|-------|----------|-----------------|
| irx1 OP_RETURN presence | `validate_poawx_coinbase` | Coinbase must contain `0x6a 0x24 "irx1" <non-zero 32-byte root>` at or above `IRIUM_POAWX_ACTIVATION_HEIGHT` when `IRIUM_POAWX_MODE=active` and not mainnet |

**Does NOT enforce:**
- Whether the 32-byte root is correct or meaningful
- Puzzle PoW (sha256d leading zeros) for any worker
- Worker identity (pubkey, signature, pkh binding)
- Reward split (P2PKH outputs to workers)
- Receipt count, lane, or expiry

### 2.2 Validation enforced in P2P block precheck (p2p.rs)

Called from: P2P block receive path (line ~447).

| Check | What it enforces |
|-------|-----------------|
| irx1 OP_RETURN presence | Same as `validate_poawx_coinbase`: presence with non-zero root |

Same limitations as above — no receipt data validation.

### 2.3 Validation enforced only in `submit_block_extended` (iriumd.rs, submit-path)

All of the following are checked **only when a block enters through the RPC endpoint**:

| Check | What it enforces |
|-------|-----------------|
| O-2 gate | Mainnet: 503 if receipts non-empty |
| C-2 gate | `submit_block` (legacy) returns 405 when `IRIUM_POAWX_MODE=active` |
| C-3 gate | `submit_block_extended` returns 400 if active + testnet + empty receipts |
| Difficulty floor | Rejects if `IRIUM_POAWX_PUZZLE_DIFFICULTY_BITS < 4` (fail-closed) |
| Receipts root match | `compute_poawx_receipts_root(req.poawx_receipts)` must equal `req.poawx_receipts_root` |
| Nonce correctness | Each `commitment_nonce` must equal `SHA256(seed \|\| "commitment_nonce")` derived from parent block hash |
| Worker identity | `verify_worker_identity`: pubkey→pkh binding + ECDSA sig over `SHA256(solution \|\| nonce \|\| height)` |
| Puzzle PoW | `sha256d(seed \|\| nonce \|\| solution)` must have ≥ `IRIUM_POAWX_PUZZLE_DIFFICULTY_BITS` leading zero bits |
| irx1 root match | Coinbase must contain irx1 OP_RETURN with root == computed receipts root |
| Reward split | `poawx_validate_reward_split`: each worker's P2PKH output must sum to `worker_due * receipt_count` |

### 2.4 The Bypass Risk

**A P2P-relayed block** that contains a valid-looking `irx1` OP_RETURN (any non-zero 32-byte value)
but was NOT submitted through `submit_block_extended` will:

- **PASS** P2P precheck (`block_has_irx1_commitment` → presence only)
- **PASS** `connect_block` (`validate_poawx_coinbase` → presence only)

And **BYPASS**:
- Worker identity verification
- Puzzle PoW verification
- Reward split enforcement
- Receipt root validity

**Practical impact for private testnet (single controlled node):** None — no P2P peers.
**Practical impact for two-node controlled setup:** Low — both nodes are trusted.
**Practical impact for real miner pilot or public testnet:** High — any miner can fabricate irx1 roots, skip puzzle work, and pay no workers.

---

## 3. Block-Contained Data Audit

### 3.1 What is actually written to the block

The block coinbase contains:
```
OP_RETURN <36 bytes: "irx1" + 32-byte receipts_root>
```

The receipts root is computed as:
```
SHA256(
  for each receipt (sorted by height, lane, worker_pkh, nonce):
    SHA256(height_le8 || lane_bytes || worker_pkh_bytes || solution_bytes || nonce_bytes)
)
```

**Included in root:** height, lane, worker_pkh (hash of pubkey), solution, commitment_nonce
**NOT included in root:** worker_pubkey (compressed secp256k1 key), worker_sig (ECDSA signature)

### 3.2 What can be verified from block data alone

From the irx1 root and block header, any node can verify:
- The commitment exists and is non-zero (consensus check)
- The root value was what the submitter claimed

Any node **cannot** verify from block data alone:
- The actual puzzle solutions (not in block)
- Whether solutions meet the required PoW difficulty (not checkable without solution + seed + nonce)
- Worker identity proofs (pubkeys and signatures not in block)
- Whether the P2PKH coinbase outputs correctly correspond to actual workers
- How many receipts were included

### 3.3 Recommendation

The current irx1 commitment is a **trust-on-submit commitment**: it proves the submitting miner
committed to some receipt data at submit time. It does not enable independent validation by any
other node, peer, or auditor.

**Minimal block-contained data required before real miner pilot:**
Option A (lightweight): Embed a UTXO-style receipt summary (worker_pkh array + solution hashes)
as additional OP_RETURN outputs — allows peers to verify root correctness.

Option B (full): Add a separate `poawx_receipts` wire field to the block serialization (new block
version), containing the full receipt data. Enables full on-chain validation everywhere.

Neither option is implemented. This is the **primary blocker** for any non-controlled environment.

---

## 4. Reorg/Lifecycle Audit

### 4.1 Current behavior

On `submit_block_extended` success at height H:
1. Receipts for height H are removed from `poawx_pending_receipts`
2. Expired receipts (older than `POAWX_RECEIPT_MAX_AGE_BLOCKS = 24`) are pruned
3. The pruned state is persisted to disk

On disconnect/reorg (block at height H disconnected):
- **No explicit receipt restoration**
- Receipts that were cleared at height H are gone
- The worker would need to re-post a receipt for the new chain state
- The new chain state has a different seed/nonce (different parent block hash → different seed)
- Stale receipts (height H-1 solutions with old nonce) will fail the nonce check

### 4.2 Risk classification

**Classification: Acceptable limitation for private testnet; post-RC follow-up for pilot**

Rationale:
- In a private single-miner testnet, reorgs are extremely rare (single-node or two trusted nodes)
- When a reorg does occur, the worker simply re-posts a new receipt for the new chain tip
- The 24-block expiry window means stale receipts don't accumulate permanently
- The receipt system is designed to be per-block-height, not cumulative across reorgs

**R-2 verdict: Not a blocker for localhost/two-node private E2E. Must be addressed before real miner pilot.**

---

## 5. Reward Split Audit

### 5.1 Current state

`poawx_validate_reward_split` is called in `submit_block_extended` after the irx1 check.

- Worker due: `block_reward(height) * 100 / 1000` (10% per receipt)
- For each distinct `worker_pkh`: checks that coinbase P2PKH outputs sum to `worker_due * receipt_count`
- Uses exact P2PKH script match (`p2pkh_script(pkh)`) — non-standard scripts not credited

### 5.2 Submit-path-only limitation

The reward split check **only runs when a block enters through `submit_block_extended`**.

A block that:
1. Contains a valid irx1 OP_RETURN
2. Pays no workers in coinbase
3. Is relayed via P2P

...would be accepted by both P2P precheck and `connect_block` — the reward split is not
checked in either location.

For controlled testnet: the single miner uses `submit_block_extended`, so the check runs.
For pilot: a miner could bypass by bypassing the RPC path — impossible in a controlled setup.

**Risk: Zero for private controlled testnet. High for uncontrolled environment.**

---

## 6. Worker Identity Audit

### 6.1 Cryptographic scheme

- Key: secp256k1 ECDSA compressed public key (33 bytes)
- Identity: `RIPEMD160(SHA256(pubkey))` → 20-byte PKH (same as Bitcoin P2PKH)
- Challenge: `SHA256(solution_bytes || nonce_bytes || height.to_le_bytes(8))`
- Signature: ECDSA over prehash challenge

### 6.2 Replay risks

| Risk | Mitigation | Status |
|------|-----------|--------|
| Same solution, different height | Challenge includes `height` | PROTECTED |
| Same height, different solution | Challenge includes `solution` | PROTECTED |
| Cross-block-hash replay (reorg) | Nonce derived from parent block hash → changes on reorg | PROTECTED |
| Cross-chain replay (mainnet) | O-2 gate: mainnet rejects all PoAW-X receipt submissions | PROTECTED |
| Cross-fork replay | Nonce binds to specific parent hash; fork changes nonce | PROTECTED |
| Cross-testnet replay | Chain ID not in challenge; could replay to another testnet instance | MINOR RISK |

The cross-testnet replay risk is minor: if two separate testnet instances share a common
genesis and chain tip hash at some height, a receipt could be replayed. In practice this
requires an unlikely coincidence of chain states.

### 6.3 Private key and sensitive data

- **Private keys** are NEVER stored, transmitted, or logged anywhere in the PoAW-X flow
- `worker_pkh` (20-byte hash) is logged: `eprintln!("[poawx] receipt stored height={} lane={} worker_pkh={} ...`
- `worker_pubkey` (compressed public key) is stored in `PoawxPendingReceipt` (in-memory + persisted JSON) — acceptable, pubkeys are not secret
- `worker_sig` is stored in `PoawxPendingReceipt` — a signature, not a private key; acceptable
- No raw private keys appear anywhere in the codebase PoAW-X paths
- Seed and nonce are deterministic from public block data; they are public values

**Worker identity audit: CLEAN**

---

## 7. RPC and Network Exposure Audit

### 7.1 Bind addresses

- Main RPC server: `IRIUM_NODE_HOST` defaults to `"127.0.0.1"`, port `IRIUM_NODE_PORT` defaults to `38300`
- Status server: `IRIUM_STATUS_HOST` defaults to `"127.0.0.1"`, port `8080`
- **Both default to localhost — NOT publicly exposed by default**
- The 39511 port referenced in hard rules is not part of the RPC server default config (likely a reverse proxy or firewall rule at the infra level)

### 7.2 Endpoint safety

| Endpoint | Guard | Risk |
|----------|-------|------|
| `POST /rpc/submit_block_extended` | RPC auth + O-2 (mainnet→503) | SAFE: auth required, mainnet blocked |
| `POST /poawx/receipt` | RPC auth + `IRIUM_POAWX_MODE=active` check | SAFE: auth required |
| `GET /poawx/assignment` | RPC auth + mode check | SAFE: only returns public seed/nonce |
| `POST /rpc/submit_block` (legacy) | RPC auth + C-2 (405 when active) | SAFE: disabled when active |

### 7.3 Secrets exposure

- No private keys or wallet seeds in any PoAW-X endpoint response or log
- No env var values logged
- No miner IP addresses stored in receipt data
- `worker_pkh` in logs is a derived hash, not a privacy-sensitive identifier

**RPC exposure audit: CLEAN**

---

## 8. Test Coverage Audit

### 8.1 What is covered

| Phase | Tests | Coverage |
|-------|-------|---------|
| 12-B | 4 | consensus irx1 enforcement in connect_block and P2P precheck |
| 12-C | 6 | puzzle difficulty hardening, minimum enforcement, fail-closed |
| 12-D | 5 | receipt persistence and reload |
| 12-E | 8 | receipt expiry, pruning, cap enforcement |
| 12-F | 5 | irx1 commitment validation in chain.rs |
| 12-G | 8 | worker identity: valid, spoofed pkh, bad pubkey, bad sig, wrong height, truncated sig, wrong solution |
| 12-H | 12 | reward split: exact pay, underpay, missing output, wrong script, multi-worker, multi-receipt |
| 12-I | 12 | end-to-end: receipt post, field preservation, stale, SBE identity/nonce/PoW, irx1, payout, regression guards |
| **Total** | **60 PoAW-X** | **231 total (0 failures)** |

### 8.2 Missing tests before live-node E2E

| Missing test | Risk |
|-------------|------|
| P2P block with fake irx1 root accepted by chain | Documents the bypass gap explicitly |
| P2P block missing irx1 rejected when activation height set | Regression for C-1 |
| submit_block_extended with un-posted receipts (skipping poawx_post_receipt) | Confirm submit is independent of pre-posting |
| Receipt re-post after height-committed clears and reposts cleanly | Idempotency of receipt lifecycle |

### 8.3 Missing tests before real miner pilot

| Missing test | Priority |
|-------------|---------|
| Two-block chain: receipt for block N consumed, fresh receipt for N+1 | High |
| Reorg simulation: receipt cleared at H, chain back to H-1 | High |
| Multiple workers, all correctly paid, block accepted | High |
| Cross-height replay attempt: solution from height H submitted at H+1 | Medium |
| Seed/nonce determinism: same input → same output (property test) | Medium |

---

## 9. Readiness Verdicts

| Target environment | Verdict | Rationale |
|-------------------|---------|-----------|
| **Localhost single-node live E2E** | **YES (conditional)** | Submit-path enforces all guards. No P2P bypass risk. Requires: `IRIUM_POAWX_MODE=active`, `IRIUM_POAWX_ACTIVATION_HEIGHT` set, node stays on localhost. |
| **VPS two-node private E2E (T-1)** | **YES (conditional)** | Both nodes fully controlled. P2P bypass gap exists but irrelevant when both nodes follow the submit protocol. Requires: both nodes use `submit_block_extended`. Must document the P2P gap as a known limitation. |
| **Trusted real miner pilot** | **NO** | Miner could fabricate irx1 root, bypass puzzle PoW, pay no workers, submit via P2P. Reward split and identity checks are submit-path-only. Block-contained receipt data required first. |
| **Broader public testnet** | **NO** | All of the above, plus: uncontrolled peers, adversarial miners, R-2 unresolved, no consensus-level receipt validation. |

### 9.1 Conditions for localhost/VPS E2E

Before running T-1 (live-node E2E):
1. Set `IRIUM_POAWX_ACTIVATION_HEIGHT` to a value ≥ current chain height + 10
2. Set `IRIUM_POAWX_MODE=active`
3. Set `IRIUM_POAWX_PUZZLE_DIFFICULTY_BITS=8` (or 4 for faster iteration)
4. Use `submit_block_extended` exclusively — never bypass via P2P injection
5. Confirm both nodes' RPC binds to 127.0.0.1 (default) or a private interface
6. Do not expose the RPC port publicly

---

## 10. Required Phases Before Real Miner Pilot

| Phase | Description | Blocks |
|-------|-------------|--------|
| **P-1** | Block-contained receipt commitment: embed worker_pkh list + solution hashes in coinbase outputs beyond irx1, OR define a `poawx_receipts` block wire field | Pilot |
| **P-2** | Consensus-level PoW verification: all nodes validate puzzle difficulty from block-contained data | Pilot |
| **R-2** | Reorg handling: restore receipts or re-derive them after disconnect events | Pilot |
| **T-1** | Live-node E2E: two-node private VPS test with real block chain state | Pre-pilot |

---

## 11. Biggest Remaining Risk

**The irx1 commitment is a submit-path-only integrity proof.**

Any node that receives a block via P2P (rather than `submit_block_extended`) accepts it if the
irx1 OP_RETURN is structurally present with any non-zero root, regardless of whether real puzzle
work was done, any worker was paid, or the root matches anything real.

This is not a risk for a controlled private testnet (no adversarial P2P peers). It is a complete
bypass for any environment with untrusted peers. The fix requires embedding verifiable receipt
data in the block itself and adding consensus-level validation — this is Phase P-1 + P-2.

---

## 12. Summary

**Code changes this phase:** None.
**Audit verdicts:** See Section 9.
**Biggest risk:** P2P bypass of all PoAW-X receipt validation (section 2.4).
**Next recommended phase:** T-1 (two-node VPS live-node E2E) OR P-1 (block-contained receipt data).
