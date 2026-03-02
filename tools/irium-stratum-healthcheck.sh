#!/usr/bin/env bash
set -euo pipefail

HEALTH_URL="${IRIUM_STRATUM_HEALTH_URL:-http://127.0.0.1:3334/health}"

json="$(curl -fsS --max-time 5 "$HEALTH_URL" || true)"
if [[ -z "$json" ]]; then
  echo "[healthcheck] empty health response; restarting irium-stratum"
  systemctl restart irium-stratum
  exit 0
fi

status="$(jq -r .status //  <<<"$json" 2>/dev/null || true)"
if [[ "$status" != "ok" ]]; then
  echo "[healthcheck] status=$status response=$(echo "$json" | tr -d "\n" | cut -c1-220); restarting irium-stratum"
  systemctl restart irium-stratum
  exit 0
fi

echo "[healthcheck] ok"
