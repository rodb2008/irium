# PoAW-X Phase 25B — three-system devnet live proof (BLOCKED; agent lacks firewall access)

**Status: BLOCKED (not achieved). Stopped before launching nodes, per the phase rules.** The
required cross-host P2P path (VPS-1 TCP 41210) is still firewall-blocked, and the agent has **no
access to open it**: VPS-1 has **no passwordless sudo** (cannot change host UFW/iptables) and there
is **no provider firewall CLI/credentials** on the host. So neither the host firewall nor a
provider/security-group rule can be added by the agent. No nodes/pool/miners were launched; no
builds were started; no mainnet/prod touched; no source changed. NOT production-ready /
mainnet-ready / audited.

## Systems / branch

- Windows `C:\Users\Ibrahim` (mainnet PID 33752, untouched); VPS-1 `207.244.247.86`
  (`vmi2780294`, mainnet 219530 untouched); VPS-2 `157.173.116.134` (`vmi2995746`, mainnet
  1851441 untouched).
- Branch `testnet/poawx-phase20-blueprint-completion-local` @ `e7b7388…` (VPS-1 synced).

## Firewall verification (still blocked)

- Windows egress IP (live): `122.162.148.238`.
- With a temporary listener bound on VPS-1 `0.0.0.0:41210`:
  - VPS-2 → VPS-1:41210 → **TIMEOUT** (`exit 124`).
  - Windows → VPS-1:41210 → **`TcpTestSucceeded: False`** (control VPS-1:22 reachable).
- Agent access check: `sudo -n true` → "a password is required" (no passwordless sudo); `ufw
  status` → "need to be root"; no `hcloud`/`aws`/`gcloud`/`doctl`/`contabo` CLI present.
- The temporary probe listener was stopped by exact PID; 41210 is not left bound.

## Exact MANUAL provider action still required (operator-only)

The operator must open inbound **TCP 41210** on VPS-1 `207.244.247.86`, source-restricted, by the
applicable mechanism (host `vmi2780294` is a Contabo-style VPS — check BOTH the provider panel and
the host firewall, since Phase 24E showed host UFW alone was insufficient):

1. **Provider firewall** (Contabo Customer Control Panel / provider firewall for server
   `207.244.247.86`): inbound TCP `41210` from `122.162.148.238/32` (Windows) and from
   `157.173.116.134/32` (VPS-2). Description: `Phase25B devnet P2P to VPS-1`.
2. **Host firewall** (as root on VPS-1), exact source-restricted rules:
   - `sudo ufw allow from 122.162.148.238 to any port 41210 proto tcp comment 'phase25b-windows-p2p'`
   - `sudo ufw allow from 157.173.116.134 to any port 41210 proto tcp comment 'phase25b-vps2-p2p'`
   - `sudo ufw status numbered`
3. Do NOT open `0.0.0.0/0`, all-ports, UDP, RPC, or stratum ports.
4. The Windows egress IP is a dynamic home-ISP address — recheck before the run.

## Verify after opening (no sudo needed)

- VPS-2: `timeout 8 bash -c 'cat </dev/null >/dev/tcp/207.244.247.86/41210 && echo OK'` → `OK`.
- Windows: `Test-NetConnection 207.244.247.86 -Port 41210` → `TcpTestSucceeded : True`.

Only when both pass should the three-system run proceed (nodes on VPS-1 hub + VPS-2 + Windows,
Irium-native CLI harness, block submit + cross-host propagation).

## Already proven (not re-run)

Single-system Irium-native CLI live-proof PASSED in Phase 24L (real Irium-PoW all-gates block
accepted by a real node, height 0 → 1). Phase 25B's only novel goal is cross-host propagation,
which the firewall blocks.

## Claim status

- Three-system devnet proof passed? **NO** (blocked at the firewall; agent cannot open it).
- Production-ready? **NO.** Mainnet-ready? **NO.** Audited? **NO.**

## Remaining blockers

- Provider/host firewall for VPS-1:41210 (operator action above).
- Windows inbound P2P via home NAT (use dialer-only hub topology — no inbound to Windows needed).
- Independent audit; public testnet; governance / mainnet activation.
