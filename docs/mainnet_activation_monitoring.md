# HTLCv1 Mainnet Activation Monitoring Plan

## 1. Core Signals (Must Watch)
- chain height progression
- best tip hash agreement across operator sample
- peer count and churn
- mempool admission/rejection rates
- block template build health
- block connect success/failure rate
- RPC health latency/error rates

## 2. HTLC-Specific Signals
- HTLC create/claim/refund RPC success vs rejection
- activation-boundary rejects (`htlcv1_not_active_at_current_height`) before height only
- post-height HTLC validation rejects by reason
- template inclusion anomalies for HTLC txs

## 3. Alert Conditions
Raise immediate alert if:
- height stalls unexpectedly
- peer count collapses below agreed floor
- repeated `connect_block_failed`/validation errors
- persistent hash/tip mismatch between major nodes
- sudden rejection spikes for previously valid tx classes

## 4. Monitoring Commands
```bash
# service and recent logs
systemctl status iriumd --no-pager
journalctl -u iriumd -n 200 --no-pager

# live follow
journalctl -u iriumd -f -o cat

# status endpoint
curl -fsS http://127.0.0.1:8080/status
```

## 5. Activation-Window Cadence
- T-60m to T: every 5 minutes
- T to T+60m: continuous watch
- T+1h to T+24h: every 15 minutes + alert-driven escalation

## 6. Reporting Format
Every report should include:
- timestamp (UTC)
- node id/host
- height and tip hash
- peer count
- critical errors observed (if any)
- action taken

## 7. Safety Reminder
This monitoring plan supports activation execution only after explicit approval. It does not authorize activation.
