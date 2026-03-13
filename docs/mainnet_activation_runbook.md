# HTLCv1 Mainnet Activation Runbook

This runbook defines the exact execution sequence for activation day.

## 0. Scope Guard
- Do not run this unless activation has explicit approval.
- This runbook does not authorize activation by itself.

## 1. Activation Height Selection
Use a deterministic selection rule:
1. Capture current canonical height `H_now`.
2. Choose notice window completion timestamp `T_notice_end`.
3. Convert `T_notice_end` to expected chain height `H_notice_end`.
4. Set activation height:
   - `H_activate = max(H_now + 4000, H_notice_end + 720)`
5. Publish `H_activate` in all operator channels.

Rationale:
- 4000-block minimum buffer for heterogeneous operator response times.
- extra 720 blocks after notice end for safety.

## 2. Pre-Activation Commands (T-60m)
Run on each production node host:
```bash
cd /home/irium/irium
git rev-parse HEAD
systemctl status iriumd --no-pager
journalctl -u iriumd -n 100 --no-pager
curl -fsS http://127.0.0.1:8080/status || true
```

Confirm:
- same release commit everywhere
- healthy peer count
- no persistent validation errors

## 3. Apply Activation Config (T-10m to T-0)
Set env on each node (exact mechanism depends on your service manager). Example systemd override:
```bash
sudo systemctl edit iriumd
# add:
# [Service]
# Environment="IRIUM_HTLCV1_ACTIVATION_HEIGHT=<H_activate>"

sudo systemctl daemon-reload
sudo systemctl restart iriumd
```

Verify env loaded:
```bash
systemctl show iriumd -p Environment | tr ' ' '\n' | grep IRIUM_HTLCV1_ACTIVATION_HEIGHT
```

## 4. Activation Window Checks (H_activate-5 to H_activate+20)
- Track height progression and tip agreement.
- Watch logs for mempool/template/connect anomalies.
- Validate no split indicators (height mismatch or persistent peer churn).

Commands:
```bash
journalctl -u iriumd -f -o cat
curl -fsS http://127.0.0.1:8080/status
```

## 5. Post-Activation Validation
At `H >= H_activate`:
1. Verify legacy tx flow remains normal.
2. Verify HTLC tx acceptance using controlled test vectors.
3. Verify block template inclusion and block connect consistency.

## 6. If Problems Start
- Follow `docs/mainnet_activation_abort_criteria.md` immediately.
- Do not continue roll-forward without triage.

## 7. Completion
- Publish operator/miner/community confirmation with observed health metrics.
- Archive logs and final checklist artifacts.
