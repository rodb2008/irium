# PoAW-X Phase 25A — three-system devnet live proof (BLOCKED at cross-host P2P)

**Status: BLOCKED (not achieved).** The three-system propagation could not be demonstrated
because cross-host P2P is blocked by the VPS provider firewall (consistent with Phase 24E) and the
Windows machine is behind home NAT. Per the phase rules ("if firewall/provider blocks P2P, stop
and report exact operator handoff; do not self-open ports / no sudo / no firewall changes"), the
live run was stopped at the P2P feasibility probe BEFORE launching nodes. No mainnet/prod touched;
no test nodes launched; no source changed. NOT production-ready / mainnet-ready / audited.

## Systems

- Windows: `C:\Users\Ibrahim` (mainnet node PID 33752, ports 38291/38300/8080 — untouched).
- VPS-1: `irium-vps` = 207.244.247.86 (`vmi2780294`); mainnet PID 219530 alive; repo HEAD
  `771969d` (already synced to the test branch).
- VPS-2: `irium-eu` = 157.173.116.134 (`vmi2995746`); mainnet PID 1851441 alive; repo present at
  `/home/irium/irium`.

## Branch / HEAD

- Branch `testnet/poawx-phase20-blueprint-completion-local`, remote HEAD
  `771969d1dc4a96c9af80edf5e7d3f5b9f54bca42`. VPS-1 already at this HEAD. (Windows + VPS-2 builds
  were not started — see blocker below.)

## Preflight results

- SSH from Windows to BOTH VPSes works (`irium-vps`, `irium-eu`).
- Both VPS mainnet nodes alive and untouched (219530, 1851441); Windows mainnet 33752 untouched.

## Cross-host P2P feasibility probe (the blocker)

Intended Phase 25A hub topology: Windows → VPS-1 and VPS-2 → VPS-1 (VPS-1 as the relay hub on P2P
port 41210). Probed VPS-1:41210 reachability with a temporary listener:

- **VPS-2 → VPS-1:41210 = TIMEOUT** (`timeout 8` connect, exit 124; the listener saw no
  connection). A timeout (not a fast refusal) indicates the packets are dropped by a firewall.
- **Windows → VPS-1:41210 = blocked** (`Test-NetConnection` `TcpTestSucceeded=False`) while the
  control **Windows → VPS-1:22 = True** and a listener was bound on 41210 — so the failure is the
  41210 path being filtered, not a missing listener.

This matches Phase 24E: VPS-1's provider firewall/security group drops the P2P port even when the
host listens. Additionally, the Windows machine is behind home NAT and can never accept INBOUND
P2P, so it can only ever be a dialer.

Conclusion: with VPS-1:41210 unreachable from both peers, and no self-firewall-change permitted,
the devnet nodes cannot form the cross-host mesh, so a block submitted on one node cannot
propagate to the others. The three-system proof is **not achievable in this run**.

## What is already proven (not re-run here)

The single-system Irium-native CLI live-proof is already PASSED (Phase 24L, on Windows): a real
Irium-native-PoW all-gates block built by `poawx-live-proof-harness`, submitted to a real local
devnet node over loopback RPC, and accepted (height 0 → 1, official 0% fee, all gates). Phase 25A
adds nothing to that single-node result; its novel goal (multi-node propagation) is what the
firewall blocks.

## Exact operator handoff (operator-only; NOT performed here)

To enable a future Phase 25A propagation run, the operator (not the agent) must:

1. **VPS-1 (207.244.247.86):** open inbound **TCP 41210** at the **provider firewall / security
   group** (hosting control panel), source-restricted to the Windows egress IP
   **122.162.148.238** and to VPS-2 **157.173.116.134**. NOTE: Phase 24E proved host `ufw` alone
   is insufficient — the provider-level firewall must also allow it.
2. For the minimal hub topology, only VPS-1:41210 needs opening (Windows and VPS-2 both DIAL OUT
   to it; no inbound to Windows/VPS-2 required).
3. The Windows egress IP (122.162.148.238) is a dynamic home-ISP address and may change; re-check
   before the run.
4. Verify after opening (no sudo needed for the checks):
   - VPS-2: `timeout 8 bash -c 'cat </dev/null >/dev/tcp/207.244.247.86/41210 && echo OK'` → `OK`.
   - Windows: `Test-NetConnection 207.244.247.86 -Port 41210` → `TcpTestSucceeded : True`.

## Cleanup / safety

- No Phase 25A nodes/pool were launched. Only short-lived probe listeners on VPS-1:41210 were used
  and they auto-terminated (via `timeout`); 41210 is free afterward.
- Mainnet/prod untouched on all three machines (Windows 33752, VPS-1 219530, VPS-2 1851441 all
  alive). VPS-1 prod pool untouched. No `~/.irium` / `%USERPROFILE%\.irium` used or modified.
- No firewall/sudo/systemd changes. No public ports bound (probe listener was transient).

## Claim status

- Three-system devnet proof passed? **NO** (blocked at cross-host P2P — provider firewall + NAT).
- Production-ready? **NO.** Mainnet-ready? **NO.** Audited? **NO.**

## Remaining blockers

- Cross-host P2P provider firewall (VPS-1:41210 + the mesh) — operator handoff above.
- Windows inbound P2P via home NAT (use dialer-only hub topology to avoid).
- Independent audit; public testnet; governance / mainnet activation.
