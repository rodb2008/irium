# Irium Node Operator Upgrade Notice (Draft)

This notice package is **pre-activation** guidance only.

## 1) Node Operator Notice Template
Subject: Irium HTLCv1 Mainnet Upgrade Preparation (No Activation Yet)

- Irium mainnet software now includes HTLCv1 code paths behind activation gating.
- **No activation is happening with this notice.**
- HTLC remains disabled unless `IRIUM_HTLCV1_ACTIVATION_HEIGHT` is explicitly set.
- Action required now:
  1. Upgrade to the announced main commit/release.
  2. Keep activation env unset.
  3. Confirm node health and peer connectivity.
  4. Report readiness status before the activation vote/window.

Verification commands:
```bash
git rev-parse HEAD
systemctl status iriumd --no-pager
journalctl -u iriumd -n 100 --no-pager
```

## 2) Miner/Pool Notice Template
Subject: Miner/Pool Readiness Notice for Future HTLCv1 Activation

- This is a readiness notice; **activation is not happening now**.
- Keep production mining on current main behavior.
- Confirm your node/pool stack runs the announced upgrade commit.
- Confirm template/mempool behavior is stable and report anomalies early.

Required response fields:
- software commit hash
- pool/node role
- observed chain height/tip
- readiness yes/no

## 3) Community Announcement Template
Headline: Irium HTLCv1 Upgrade Path Published (Mainnet Still OFF)

- HTLCv1 support is integrated but remains activation-gated.
- No mainnet activation has been executed.
- A separate activation proposal will include:
  - activation height,
  - notice period,
  - rollback/abort criteria,
  - monitoring plan.
- Until then, mainnet behavior remains unchanged.

## 4) Required Message Footer
- HTLCv1 is still OFF on mainnet.
- No mainnet activation has been performed.
