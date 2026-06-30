#!/usr/bin/env bash
# poawx-firewall-template.sh — PRINT the exact operator UFW commands for a trusted-miner
# pilot. It NEVER executes ufw/sudo; it only prints the source-restricted open/verify/close
# commands for the operator to run themselves.
#
# SAFETY: prints only. No sudo, no ufw, no firewall change, no network call.
#
# Usage: poawx-firewall-template.sh --miner-ip <IP> --stratum-port <P> [--observer-ip <IP> --p2p-port <P>]
#        poawx-firewall-template.sh --help
set -euo pipefail
MINER_IP=""; SPORT=""; OBS_IP=""; PPORT=""
while [ $# -gt 0 ]; do case "$1" in
  --help|-h) sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; exit 0;;
  --miner-ip) MINER_IP="${2:?}"; shift 2;;
  --stratum-port) SPORT="${2:?}"; shift 2;;
  --observer-ip) OBS_IP="${2:?}"; shift 2;;
  --p2p-port) PPORT="${2:?}"; shift 2;;
  *) echo "unknown arg $1" >&2; exit 1;; esac; done
[ -n "$MINER_IP" ] && [ -n "$SPORT" ] || { echo "error: --miner-ip and --stratum-port required" >&2; exit 1; }
case "$MINER_IP" in *[!0-9.]*|"") echo "error: --miner-ip must be a bare IPv4 (no 'Anywhere')" >&2; exit 1;; esac

echo "# === OPERATOR-RUN ONLY (the agent never runs these). Source-restricted, never Anywhere. ==="
echo "# OPEN (before the miner connects):"
echo "sudo ufw allow from ${MINER_IP} to any port ${SPORT} proto tcp comment 'poawx trusted miner stratum temp'"
if [ -n "$OBS_IP" ] && [ -n "$PPORT" ]; then
  case "$OBS_IP" in *[!0-9.]*|"") echo "error: --observer-ip must be a bare IPv4" >&2; exit 1;; esac
  echo "sudo ufw allow from ${OBS_IP} to any port ${PPORT} proto tcp comment 'poawx observer p2p temp'"
fi
echo "# VERIFY:"
echo "sudo ufw status numbered | grep -E '${SPORT}${PPORT:+|${PPORT}}'"
echo "# CLOSE (immediately after the pilot):"
echo "sudo ufw delete allow from ${MINER_IP} to any port ${SPORT} proto tcp"
[ -n "$OBS_IP" ] && [ -n "$PPORT" ] && echo "sudo ufw delete allow from ${OBS_IP} to any port ${PPORT} proto tcp"
echo "sudo ufw status numbered | grep -E '${SPORT}${PPORT:+|${PPORT}}' || echo 'rules absent'"
echo "# Note: stratum is the only miner-facing port; RPC/status/delegation/metrics stay loopback."
