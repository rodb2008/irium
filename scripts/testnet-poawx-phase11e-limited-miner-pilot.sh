#!/usr/bin/env bash
# PoAW-X Phase 11-E: Limited Miner Pilot Automation Script
# Usage: bash scripts/testnet-poawx-phase11e-limited-miner-pilot.sh [--blocks N] [--skip-cleanup]
# Runs from VPS-1; requires testnet binary and stratum binary to be built.
# Does NOT touch mainnet services.
set -euo pipefail

# ── Config ──────────────────────────────────────────────────────────────────
BLOCKS="${BLOCKS:-12}"
SKIP_CLEANUP=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --blocks) BLOCKS="$2"; shift 2 ;;
    --skip-cleanup) SKIP_CLEANUP=1; shift ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

IRIUM_DIR="${HOME}/irium"
DATA_DIR="${HOME}/irium-poawx-phase11e"
LOG_DIR="${DATA_DIR}/logs"
IRIUMD_BIN="${IRIUM_DIR}/target/debug/iriumd"
STRATUM_BIN="${IRIUM_DIR}/pool/irium-stratum/target/release/irium-stratum"
HARNESS="${IRIUM_DIR}/scripts/poawx-stratum-long-soak-harness.py"

RPC_PORT=39511
P2P_PORT=39510
STRATUM_PORT=39512
RPC_TOKEN="${IRIUM_PHASE11E_RPC_TOKEN:?IRIUM_PHASE11E_RPC_TOKEN must be set}"

# ── Pre-flight: mainnet safety check ────────────────────────────────────────
echo "[preflight] checking mainnet is intact..."
MAINNET_PID=$(pgrep -f "iriumd" | head -1 || true)
if [[ -n "$MAINNET_PID" ]]; then
  MAINNET_ENV=$(cat /proc/${MAINNET_PID}/environ 2>/dev/null | tr "\0" "\n" | grep IRIUM_POAWX_MODE || true)
  if echo "$MAINNET_ENV" | grep -q "active"; then
    echo "ERROR: mainnet iriumd appears to have IRIUM_POAWX_MODE=active — aborting"
    exit 1
  fi
fi
echo "[preflight] mainnet check PASS"

# ── Check testnet ports are free ────────────────────────────────────────────
for PORT in $P2P_PORT $RPC_PORT $STRATUM_PORT; do
  if ss -tlnp 2>/dev/null | grep -q ":${PORT} "; then
    echo "ERROR: port $PORT already in use — stop existing testnet services first"
    exit 1
  fi
done

# ── Create data dir ─────────────────────────────────────────────────────────
mkdir -p "${DATA_DIR}" "${LOG_DIR}"

# Bootstrap from Phase 11-D testnet dir if available
if [[ -d "${HOME}/irium-poawx-testnet/anchors.json" ]] || [[ -f "${HOME}/irium-poawx-testnet/anchors.json" ]]; then
  echo "[setup] using existing anchors from phase11d testnet dir"
  BOOTSTRAP_DIR="${HOME}/irium-poawx-testnet"
else
  BOOTSTRAP_DIR="${DATA_DIR}"
fi

# ── Start testnet iriumd ─────────────────────────────────────────────────────
echo "[start] launching testnet iriumd on :${P2P_PORT}/:${RPC_PORT}..."
cd "${IRIUM_DIR}"
IRIUM_NETWORK=devnet \
  IRIUM_POAWX_MODE=active \
  IRIUM_P2P_BIND="0.0.0.0:${P2P_PORT}" \
  IRIUM_NODE_HOST=127.0.0.1 \
  IRIUM_NODE_PORT="${RPC_PORT}" \
  IRIUM_DATA_DIR="${DATA_DIR}" \
  IRIUM_BOOTSTRAP_DIR="${BOOTSTRAP_DIR}" \
  IRIUM_RPC_TOKEN="${RPC_TOKEN}" \
  IRIUM_DEV_EASY_BITS_TEMPLATE=1 \
  "${IRIUMD_BIN}" >> "${LOG_DIR}/iriumd.log" 2>&1 &
IRIUMD_PID=$!
echo "[start] iriumd PID ${IRIUMD_PID}"

# Wait for RPC to come up
echo "[start] waiting for RPC to be ready..."
for i in $(seq 1 30); do
  if curl -s --max-time 2 "http://127.0.0.1:${RPC_PORT}/status" > /dev/null 2>&1; then
    echo "[start] RPC ready after ${i}s"
    break
  fi
  sleep 1
done

STATUS=$(curl -s "http://127.0.0.1:${RPC_PORT}/status" 2>/dev/null || echo "FAIL")
echo "[start] status: ${STATUS}"

# ── Start stratum ────────────────────────────────────────────────────────────
echo "[start] launching stratum on :${STRATUM_PORT}..."
STRATUM_BIND="0.0.0.0:${STRATUM_PORT}" \
  IRIUM_RPC_BASE="http://127.0.0.1:${RPC_PORT}" \
  IRIUM_RPC_TOKEN="${RPC_TOKEN}" \
  IRIUM_STRATUM_POAWX=1 \
  IRIUM_POW_LIMIT_HEX=7fffff0000000000000000000000000000000000000000000000000000000000 \
  STRATUM_DEFAULT_DIFF=1 \
  IRIUM_STRATUM_VARDIFF_ENABLED=0 \
  IRIUM_STRATUM_MINER_FAMILY=cpuminer \
  IRIUM_STRATUM_MAX_SESSIONS=50 \
  "${STRATUM_BIN}" >> "${LOG_DIR}/stratum.log" 2>&1 &
STRATUM_PID=$!
echo "[start] stratum PID ${STRATUM_PID}"
sleep 2

# ── Run harness ──────────────────────────────────────────────────────────────
echo "[pilot] running ${BLOCKS}-block harness with --receipt --bogus..."
set +e
python3 "${HARNESS}" \
  127.0.0.1 "${STRATUM_PORT}" \
  "http://127.0.0.1:${RPC_PORT}" "${RPC_TOKEN}" \
  --blocks "${BLOCKS}" --receipt --bogus
HARNESS_RC=$?
set -e

if [[ $HARNESS_RC -eq 0 ]]; then
  echo "[pilot] PASS (rc=0)"
else
  echo "[pilot] FAIL (rc=${HARNESS_RC}) — check logs at ${LOG_DIR}/"
fi

# ── Cleanup ──────────────────────────────────────────────────────────────────
if [[ $SKIP_CLEANUP -eq 0 ]]; then
  echo "[cleanup] stopping pilot services..."
  kill "${STRATUM_PID}" 2>/dev/null || true
  kill "${IRIUMD_PID}" 2>/dev/null || true
  sleep 2
  # Force-kill if still running
  kill -9 "${STRATUM_PID}" 2>/dev/null || true
  kill -9 "${IRIUMD_PID}" 2>/dev/null || true
  echo "[cleanup] done"
else
  echo "[cleanup] SKIP_CLEANUP=1 — services left running: iriumd=${IRIUMD_PID} stratum=${STRATUM_PID}"
  echo "[cleanup] stop manually: kill ${IRIUMD_PID} ${STRATUM_PID}"
fi

echo "[mainnet] post-run mainnet check..."
curl -s http://localhost:8080/status | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'mainnet height={d["height"]}')" 2>/dev/null || echo "mainnet status: could not reach (normal if different port)"

echo "[done] Phase 11-E pilot script complete. Logs: ${LOG_DIR}/"
exit $HARNESS_RC
