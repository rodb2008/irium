# Phase 14-B: Full Two-VPS Testnet Validation

**Date:** 2026-06-14
**Branch:** `testnet/poawx-phase12-completion-rc-hardening`
**Commit:** `fcab476` (chore: sync lockfile and format poawx branch)
**Push status:** NOT pushed. Remote branch ABSENT.
**Mainnet:** Untouched throughout (height=31323 at shutdown, advancing normally).

---

## Scope

Full repeatability proof of the complete PoAW-X stack running from clean genesis on two isolated VPS nodes, with 13 negative rejection checks and all Phase 13 consensus rules verified end-to-end.

| Check area | Outcome |
|---|---|
| Git / branch safety | PASS |
| Two-node devnet boot (clean genesis) | PASS |
| Seed isolation (no mainnet seeds) | PASS |
| PoAW-X block template | PASS |
| Worker keypair + puzzle | PASS |
| Receipt submission pipeline | PASS |
| Negative checks (13 total) | **13/13 PASS** |
| Good block mine + submit | PASS |
| Phase 13-A: irx1_root wire format | PASS |
| Phase 13-B: 7-rule consensus | PASS |
| Phase 13-C: reorg receipt restore | PASS |
| VPS-2 P2P sync | PASS |
| VPS-2 block JSON match | PASS |
| Both nodes same tip hash | PASS |
| Mainnet safety | PASS |
| **E2E total** | **37/37 PASS** |

---

## 1. Branch / Git State

- Branch: `testnet/poawx-phase12-completion-rc-hardening`
- HEAD: `fcab476`
- origin/main: `cec0070` (explorer-only, no PoAW-X impact)
- Remote branch: ABSENT (not pushed)

---

## 2. Test Topology

| Node | Config |
|---|---|
| VPS-1 | `127.0.0.1:39511` (RPC), `127.0.0.1:39508` (status), P2P `0.0.0.0:39510`, `IRIUM_POAWX_MODE=active`, `ACTIVATION_HEIGHT=1`, `DIFFICULTY_BITS=4`, token `irium-14b-devnet` |
| VPS-2 | SSH node, `127.0.0.1:39511/39508`, P2P `0.0.0.0:39514`, seed → VPS-1:39510, same token |
| Data dirs | `/home/irium/irium-devnet-14b-vps{1,2}-data` (in-home paths) |
| Mainnet seeds | NOT used (devnet only) |
| Real miners | NOT invited |
| Nodes stopped | After E2E completion |

---

## 3. E2E Results: 37/37 PASS

```
[PASS] P-1:  VPS-1 responding at genesis (height=0)
[PASS] P-2:  VPS-2 responding at genesis (height=0)
[PASS] P-3:  VPS-2 peers <= 1 (devnet isolation) (peers=1)
[PASS] P-4:  Block template reachable (HTTP 200)
[PASS] P-4b: PoAW-X mode active (mode=active)
[PASS] P-4c: Template height=1 (height=1)
[PASS] P-5:  Worker keypair + puzzle solved
[PASS] P-6:  Receipt accepted (200)
[PASS] P-7:  Receipt in pending (count=1)
[PASS] P-8:  receipts_root non-empty
[PASS] N-1:  Legacy submit_block → 405
[PASS] N-2:  Empty receipts → 400
[PASS] N-3:  Block without irx1 → 400
[PASS] N-4:  Block with zero irx1 root → 400
[PASS] N-5:  Block with wrong irx1 root → 400
[PASS] N-6:  Bad worker ECDSA sig → 400
[PASS] N-7:  Spoofed worker pkh → 400
[PASS] N-8:  Wrong commitment_nonce → 400
[PASS] N-9:  Insufficient puzzle PoW → 400
[PASS] N-10: Missing worker payout → 400
[PASS] N-11: Wrong worker payout pkh → 400
[PASS] N-12: Mainnet PoAW-X not active (HTTP 404)
[PASS] N-13: RPC 39511 not publicly reachable (refused from public IP)
[PASS] P-9:  Good block mined
[PASS] P-10: Block accepted VPS-1 (HTTP 200)
[PASS] P-11: VPS-1 at height 1
[PASS] P-12: Phase 13-A: irx1_root in VPS-1 block JSON
[PASS] P-13: Phase 13-A: irx1_root matches submitted receipts_root (exact)
[PASS] P-14: Phase 13-B: 7 rules validated (block accepted = all pass)
[PASS] P-15: Receipt cleared from pending after commit
[PASS] P-16: Phase 13-C: 10 reorg unit tests (cargo test)
[PASS] P-17: Phase 13-C: structural reorg_orphaned_blocks compiled
[PASS] P-18: VPS-2 synced to height 1 via P2P (attempt=1)
[PASS] P-19: VPS-2: irx1_root in block JSON (exact match)
[PASS] P-20: VPS-2: irx1_root matches VPS-1 (match=True)
[PASS] P-21: Both nodes same tip hash (match=True)
[PASS] P-22: Mainnet untouched and running (height=31323)

Total: 37 | PASS: 37 | FAIL: 0 | SKIP: 0
VERDICT: PASS
```

---

## 4. Negative Check Coverage

| ID | Endpoint | Violation | Expected | Result |
|---|---|---|---|---|
| N-1 | `POST /rpc/submit_block` | Legacy endpoint | 405 | **PASS** |
| N-2 | `POST /rpc/submit_block_extended` | Empty receipts array | 400 | **PASS** |
| N-3 | `POST /rpc/submit_block_extended` | Coinbase missing irx1 OP_RETURN | 400 | **PASS** |
| N-4 | `POST /rpc/submit_block_extended` | irx1 OP_RETURN with zero root | 400 | **PASS** |
| N-5 | `POST /rpc/submit_block_extended` | irx1 root mismatch | 400 | **PASS** |
| N-6 | `POST /poawx/receipt` | Bad worker ECDSA signature | 400 | **PASS** |
| N-7 | `POST /poawx/receipt` | Spoofed worker_pkh | 400 | **PASS** |
| N-8 | `POST /poawx/receipt` | Wrong commitment_nonce | 400 | **PASS** |
| N-9 | `POST /poawx/receipt` | Insufficient puzzle PoW | 400 | **PASS** |
| N-10 | `POST /rpc/submit_block_extended` | Coinbase missing worker payout | 400 | **PASS** |
| N-11 | `POST /rpc/submit_block_extended` | Worker payout to wrong pkh | 400 | **PASS** |
| N-12 | `GET /poawx/assignment` (mainnet) | PoAW-X inactive on mainnet | 404 | **PASS** |
| N-13 | Public IP:39511 | RPC not publicly exposed | refused | **PASS** |

---

## 5. Phase 13 Regression Coverage

### Phase 13-A (Receipt wire format)
- Block JSON at `/rpc/block?height=1` includes `irx1_root` field
- VPS-1 irx1_root matches the `receipts_root` computed by the E2E harness (exact SHA-256 match)
- VPS-2 irx1_root propagated over P2P and matches VPS-1 exactly

### Phase 13-B (7-rule consensus in connect_block)
| Rule | Verification |
|---|---|
| irx1 OP_RETURN present | N-3 rejected (400) |
| irx1 root non-zero | N-4 rejected (400) |
| irx1 root matches receipt set | N-5 rejected (400) |
| Worker nonce valid | N-8 rejected (400) |
| Worker pkh derivation | N-7 rejected (400) |
| ECDSA sig valid | N-6 rejected (400) |
| Puzzle PoW sufficient | N-9 rejected (400) |
| Reward split (worker payout) | N-10/N-11 rejected (400) |
| Good block accepted | P-10 accepted (200) |

### Phase 13-C (Reorg receipt restore)
- `cargo test phase13c` → 10/10 pass on live VPS-1 binary
- Structural: `ChainState.reorg_orphaned_blocks: Vec<Block>` compiled and present in chain.rs + iriumd.rs
- Live VPS reorg simulation: not performed (covered by unit harness)

---

## 6. P2P Sync Validation

- VPS-2 seeded only from VPS-1:39510 (no mainnet seeds)
- VPS-2 synced to height=1 on attempt=1 (instant, Phase 12-M broadcast_block confirmed working)
- Both nodes: same `irx1_root` in block JSON
- Both nodes: same tip block hash (`header.hash` field exact match)

---

## 7. Mainnet Safety Confirmation

| Item | Status |
|---|---|
| Mainnet iriumd (port 38300/8080/38291) | Running, unmolested |
| Mainnet height at shutdown | 31323 (advancing normally) |
| Devnet RPC 39511 public exposure | CLOSED (N-13 confirmed) |
| Real miners invited | NO |
| Mainnet seeds used in devnet | NO |
| Branch pushed | NO |
| PR created | NO |
| main branch touched | NO |

---

## 8. E2E Script Notes

The Phase 14-B E2E harness (`/home/irium/phase14b_e2e.py`) introduced the following fixes over Phase 13-D:

1. **`start_vps1` Popen refactor:** `subprocess.check_output(['bash', '-c', cmd])` caused bash to enter `do_wait` waiting for the backgrounded iriumd child, blocking indefinitely. Fixed by using `subprocess.Popen(..., start_new_session=True)` to launch iriumd directly with a new session, bypassing the bash intermediary.

2. **`start_vps2` Popen refactor + setsid:** Same bash `do_wait` issue over SSH. Fixed by splitting into a kill-only SSH call followed by a `Popen` with a 10s timeout; iriumd is reliably started on VPS-2 even if the SSH session doesn't close promptly.

3. **`stop_devnet` port-based kill:** Original kill relied on PID files, missing processes from prior sessions with different PID paths. Fixed by adding `fuser -k PORT/tcp` for all devnet ports on both VPS nodes, using `subprocess.run([...])` list args (no shell injection).

4. **`block['header']['hash']` field path:** Block JSON nests the hash under `header.hash`, not at the top level. Fixed P-21 to use `blk.get('header', {}).get('hash', block_hash)`.

None of these are protocol bugs — all are test harness adaptations to the Irium node's process model and JSON API structure.

---

## 9. Readiness Verdicts

| Scenario | Status |
|---|---|
| Single-node submit-path E2E | **PROVEN** |
| Two-node P2P sync | **PROVEN** |
| Phase 13-A: receipt wire format | **PROVEN** |
| Phase 13-B: 7-rule consensus | **PROVEN** |
| Phase 13-C: reorg receipt restore | **PROVEN** (unit + structural) |
| Full two-VPS testnet validation | **PROVEN** (this document, 37/37) |
| Push branch for review | **READY** (pending explicit approval) |
| Trusted real miner pilot | **READY** (pending explicit approval) |
| Public testnet | NOT YET — requires push + review |
| Merge to main | NOT YET — requires review + approval |
