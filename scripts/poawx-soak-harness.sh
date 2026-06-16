#!/usr/bin/env bash
# poawx-soak-harness.sh — loopback-only PoAW-X soak/reorg harness PLANNER.
#
# SAFETY: by default this PRINTS the plan/commands and starts NOTHING. It never uses sudo,
# never touches the firewall, never binds a public port, never uses pkill/killall, and only
# operates on an isolated $TROOT under $HOME. Actually starting services requires the explicit
# guard RUN=1 AND operator approval; even then all binds are 127.0.0.1 and teardown is by
# exact pidfile. This script is a documented harness, not an auto-runner.
#
# Usage:
#   poawx-soak-harness.sh plan      # print the full loopback bring-up + scenarios (default)
#   poawx-soak-harness.sh smoke     # print the short smoke steps (does NOT run unless RUN=1)
#   poawx-soak-harness.sh --help
set -uo pipefail
CMD="${1:-plan}"
TROOT="${TROOT:-/home/irium/phase20-soak}"
RPC=39811; ST=39808; STRAT=39812; DEL=39813; MET=39814
case "$CMD" in
  --help|-h) sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'; exit 0;;
  plan|smoke) : ;;
  *) echo "unknown subcommand: $CMD (use plan|smoke|--help)" >&2; exit 1;;
esac

cat <<EOF
=== PoAW-X soak/reorg harness ($CMD) — loopback-only, $TROOT ===
Ports (all 127.0.0.1): status $ST  rpc $RPC  stratum $STRAT  delegation $DEL  metrics $MET
STRATUM_DEFAULT_DIFF=1 (stratum SHARE difficulty; chain difficulty is automatic via LWMA-144)

Bring-up (loopback): node poawx-inactive -> mine block1 -> restart node active(act=2) ->
  GET /poawx/pool-identity -> wallet --emit-only -> loopback POST /poawx/delegation ->
  stock cpuminer -> mode-1 block via submit_block_extended.

Scenarios (each loopback, exact pidfiles, no sudo/firewall):
  1. single-node long soak     : repeat mine N blocks; assert tip advances, irx1 present.
  2. restart during mining     : kill stratum/node by pidfile, restart, assert tip continuity.
  3. pending receipt reload    : restart node; assert pending receipts reload (no loss).
  4. reorg w/ mode-1 receipts  : build competing tip; assert delegation preserved on restore.
  5. invalid receipt           : submit forged receipt -> rejected.
  6. stale assignment          : assignment height/lane mismatch -> fail closed.
  7. expired delegation        : tip > expiry -> rejected.
  8. observer validation       : (two-VPS, operator-approved only) node B re-validates block.
Teardown: kill exact pidfiles (prod-pid-allowlist + /proc cmdline check); rm -rf $TROOT;
  verify ports clear; verify mainnet/prod PIDs alive. Never pkill/killall.
EOF

if [ "$CMD" = smoke ]; then
  echo ""
  if [ "${RUN:-0}" != "1" ]; then
    echo "[smoke] dry-run: set RUN=1 AND obtain operator approval to actually start loopback services."
    echo "[smoke] this harness will NOT start node/stratum/cpuminer without RUN=1."
    exit 0
  fi
  echo "[smoke] RUN=1 set — (intentionally not auto-executing here; perform the documented"
  echo "        loopback bring-up steps above under operator supervision, then verify + teardown)."
fi
