#!/usr/bin/env bash
# poawx-pilot-readiness-check.sh — read-only pre-flight checks for a PoAW-X trusted-miner pilot.
#
# SAFETY: read-only. No sudo, no firewall, no services started, binds nothing, kills nothing.
# Verifies that private endpoints are loopback-bound, that prod PIDs are alive, and that the
# pilot $TROOT is isolated. Prints PASS/FAIL per check and a final verdict.
#
# Usage: poawx-pilot-readiness-check.sh [--rpc 39811] [--status 39808] [--delegation 39813] \
#          [--metrics 39814] [--stratum-port 39812] [--troot <path>] [--prod-pids "219530 4042500 ..."]
#        poawx-pilot-readiness-check.sh --help
set -uo pipefail
RPC=39811; ST=39808; DEL=39813; MET=39814; SPORT=39812; TROOT=""; PRODPIDS=""
while [ $# -gt 0 ]; do case "$1" in
  --help|-h) sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'; exit 0;;
  --rpc) RPC="$2"; shift 2;; --status) ST="$2"; shift 2;; --delegation) DEL="$2"; shift 2;;
  --metrics) MET="$2"; shift 2;; --stratum-port) SPORT="$2"; shift 2;;
  --troot) TROOT="$2"; shift 2;; --prod-pids) PRODPIDS="$2"; shift 2;;
  *) echo "unknown arg $1" >&2; exit 1;; esac; done
fail=0
chk(){ if [ "$2" = ok ]; then printf "PASS  %s\n" "$1"; else printf "FAIL  %s\n" "$1"; fail=1; fi; }
loopback_only(){ # $1 port -> PASS if listening only on 127.0.0.1/::1 (or not listening)
  local p="$1" lines; lines=$(ss -ltnH 2>/dev/null | awk -v P=":$p" '$4 ~ P{print $4}')
  [ -z "$lines" ] && { echo ok; return; }
  echo "$lines" | grep -qvE '^(127\.0\.0\.1|\[::1\]):' && echo bad || echo ok
}
for pair in "RPC:$RPC" "status:$ST" "delegation:$DEL" "metrics:$MET"; do
  name=${pair%%:*}; port=${pair##*:}
  [ "$(loopback_only "$port")" = ok ] && chk "$name :$port loopback-only/private" ok || chk "$name :$port NON-loopback bind!" bad
done
# stratum may be public (firewall-gated) — informational only
sline=$(ss -ltnH 2>/dev/null | awk -v P=":$SPORT" '$4 ~ P{print $4}')
[ -n "$sline" ] && echo "INFO  stratum :$SPORT bound at: $sline (must be source-restricted by operator UFW)" || echo "INFO  stratum :$SPORT not yet listening"
# prod PIDs alive
if [ -n "$PRODPIDS" ]; then for p in $PRODPIDS; do kill -0 "$p" 2>/dev/null && chk "prod/mainnet PID $p alive" ok || chk "prod/mainnet PID $p NOT alive" bad; done; fi
# TROOT isolation
if [ -n "$TROOT" ]; then
  case "$TROOT" in /home/*/phase*|/home/*/*pilot*|/tmp/*) chk "TROOT isolated ($TROOT)" ok;; *mainnet*|*/.irium*|*prod*) chk "TROOT looks like prod/mainnet ($TROOT)" bad;; *) chk "TROOT path ($TROOT) — confirm isolated" ok;; esac
fi
echo "---"; [ $fail -eq 0 ] && echo "READINESS: PASS" || echo "READINESS: FAIL (resolve above before opening any firewall rule)"
exit $fail
