# Irium PoAW-X Public Testnet — Tester Feedback Form

Copy this form, fill it in for **each node you run**, and submit it as a GitHub issue
on `iriumlabs/irium` titled `[testnet] <your node name>`. Re-submit (or comment) if
you observe something new. Attach logs where asked (`journalctl -u irium-testnet`).

---

```
Tester / node name : ___________
Release tag        : poawx-testnet-v0.1
Binary SHA256      : iriumd=__________  irium-miner=__________   (from your sha256sum -c)
Host / infra       : ___________ (e.g. Proxmox LXC, Ubuntu 24.04, 2 vCPU/2GB)
Observation window : from ____ to ____ (total uptime: ____)
Are you mining?    : yes / no
```

## A. Node sync
- [ ] `peer_count` reached ≥ 1 within ~2 minutes of starting
- [ ] `peer_count` stayed ≥ 1 for the whole window (never stuck at 0)
- [ ] `genesis_hash` == `0000000028f25d65557e9d8d9e991f516c00d68f5aeae10b750645b398bd10a3`
- [ ] `anchor_loaded` == true
- [ ] height tracked the seed nodes — **max lag observed: ____ blocks**
- [ ] survived a process restart and resumed syncing (final height after restart: ____)
- Notes / anomalies: ___________

## B. Block validation
- [ ] no `connect_block failed` errors in the log
- [ ] no `stateless precheck failed` / `merkle precheck failed`
- [ ] no `phase21d`/`phase22a`/admission/ticket/precommit rejections
- Paste any rejection lines verbatim: ___________

## C. Reward splits
- [ ] sampled coinbase outputs split **55 / 22 / 13 / 10** (PRIMARY/COMPUTE/VERIFY/SUPPORT)
- Heights sampled: ___________   Any deviation seen? ___________

## D. Finality proofs
- [ ] `/rpc/block?height=N` shows a **nonzero** `poawx_finality_digest`
- [ ] no reorg observed that undid a previously-finalized block
- Notes: ___________

## E. Adaptive mode
- [ ] `poawx_adaptive_mode` observed value(s): normal / caution / defense / recovery
- [ ] mode changed as miners joined/left the network — describe when and to what: ___________

## F. Miner participation (skip if not mining)
- [ ] `irium-miner --poawx` produced an **accepted** block — first height: ____
- [ ] admission POSTs succeeded (no repeated `HTTP 400` / `HTTP 403`)
- [ ] no `insufficient sybil work` / `precommit` / `ticket` errors
- [ ] `IRIUM_POAWX_MINER_INTERVAL_SECS` used: ____ (please keep ≥ 30)
- Approx blocks you produced in the window: ____

## G. Crashes / rejections / resource use
- [ ] any crash or panic? If yes, attach the last ~100 `journalctl` lines
- [ ] node was OOM-killed or restarted unexpectedly? (how often: ____)
- CPU / RAM / disk over the window: ___________

## H. Overall
- Severity of the worst issue you hit: none / cosmetic / degraded / blocking
- Free-form feedback, suggestions, questions: ___________
```
