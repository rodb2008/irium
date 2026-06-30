#!/usr/bin/env bash
# poawx-log-passfail.sh — scan PoAW-X stratum + node logs for PASS/FAIL signals.
#
# SAFETY: read-only. Greps log files only. No sudo, no firewall, no services, no network.
#
# Usage: poawx-log-passfail.sh <stratum.log> [node.log]
#        poawx-log-passfail.sh --help
set -uo pipefail
[ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ] && { sed -n '2,7p' "$0" | sed 's/^# \{0,1\}//'; exit 0; }
[ $# -ge 1 ] || { echo "usage: poawx-log-passfail.sh <stratum.log> [node.log]" >&2; exit 1; }
SL="$1"; NL="${2:-}"
cnt(){ local n; n=$(grep -cE "$2" "$1" 2>/dev/null); echo "${n:-0}"; }
echo "=== stratum: $SL ==="
echo "native_rewardable jobs : $(cnt "$SL" 'adapter_kind=native_rewardable')"
echo "mode-1 receipts built  : $(cnt "$SL" '\[poawx-trace\] build_mode1 OK')"
echo "submit_block_extended  : $(cnt "$SL" 'submit_block_extended')"
echo "BLOCK_ACCEPTED         : $(cnt "$SL" 'BLOCK_ACCEPTED')"
echo "shares rejected        : $(cnt "$SL" '\[share\] reject')"
echo "  low_difficulty       : $(cnt "$SL" 'reason=low_difficulty')"
echo "delegation rejected    : $(cnt "$SL" 'delegation rejected|fee_bps|wrong worker|expired')"
acc=$(cnt "$SL" 'BLOCK_ACCEPTED')
if [ -n "$NL" ]; then
  echo "=== node: $NL ==="
  echo "tip height (last)      : $(grep -oE '"height":[0-9]+|local height=[0-9]+' "$NL" 2>/dev/null | tail -1)"
  echo "anchors load errors    : $(cnt "$NL" 'Failed to load anchors')"
  echo "panics                 : $(cnt "$NL" 'panic|thread .main. panicked')"
fi
echo "---"
if [ "$acc" -ge 1 ]; then echo "VERDICT: PASS (>=1 block accepted)"; else echo "VERDICT: NO BLOCK ACCEPTED YET (check rejects / mining window)"; fi
