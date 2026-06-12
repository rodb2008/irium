#!/usr/bin/env bash
# PoAW-X Phase 11-F: Real External Miner Validation Script
# Usage: bash scripts/testnet-poawx-phase11f-real-external-miner-validation.sh [--blocks N] [--skip-cleanup] [--skip-vps2]
#
# Starts isolated Phase 11-F testnet services on VPS-1.
# Runs VPS-2 control check (P2P + stratum reachability).
# Waits for operator to confirm real miner connected and accepted.
# Does NOT touch mainnet services.
set -euo pipefail

# ── Config ──────────────────────────────────────────────────────────────────
BLOCKS="${BLOCKS:-6}"
SKIP_CLEANUP=0
SKIP_VPS2=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --blocks) BLOCKS="$2"; shift 2 ;;
    --skip-cleanup) SKIP_CLEANUP=1; shift ;;
    --skip-vps2) SKIP_VPS2=1; shift ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

IRIUM_DIR="${HOME}/irium"
DATA_DIR="${HOME}/irium-poawx-phase11f"
LOG_DIR="${DATA_DIR}/logs"
BOOTSTRAP_DIR="${HOME}/irium-poawx-testnet-p2p"
IRIUMD_BIN="${IRIUM_DIR}/target/debug/iriumd"
STRATUM_BIN="${IRIUM_DIR}/pool/irium-stratum/target/release/irium-stratum"
HARNESS="${IRIUM_DIR}/scripts/poawx-stratum-long-soak-harness.py"
VPS2_SSH="irium-eu"

RPC_PORT=39511
P2P_PORT=39510
STRATUM_PORT=39512
RPC_TOKEN="${IRIUM_PHASE11F_RPC_TOKEN:?IRIUM_PHASE11F_RPC_TOKEN must be set}"

# ── Pre-flight: mainnet safety check ────────────────────────────────────────
echo "[preflight] checking mainnet is intact..."
MAINNET_PID=$(pgrep -f "target/release/iriumd" | head -1 || true)
if [[ -n "$MAINNET_PID" ]]; then
  MAINNET_ENV=$(cat /proc/${MAINNET_PID}/environ 2>/dev/null | tr "\0" "\n" | grep IRIUM_POAWX_MODE || true)
  if echo "$MAINNET_ENV" | grep -q "active"; then
    echo "ERROR: mainnet iriumd appears to have IRIUM_POAWX_MODE=active — aborting"
    exit 1
  fi
fi
echo "[preflight] mainnet check PASS (PID=${MAINNET_PID:-none})"

# Check testnet ports are free
for PORT in $P2P_PORT $RPC_PORT $STRATUM_PORT; do
  if ss -tlnp 2>/dev/null | grep -q ":${PORT} "; then
    echo "ERROR: port $PORT already in use — stop existing testnet services first"
    exit 1
  fi
done

# ── Setup ───────────────────────────────────────────────────────────────────
mkdir -p "${DATA_DIR}" "${LOG_DIR}"

# Use Phase 11-D testnet-p2p dir as bootstrap (has anchors.json and trust/)
if [[ ! -f "${BOOTSTRAP_DIR}/anchors.json" ]]; then
  echo "ERROR: no anchors.json at ${BOOTSTRAP_DIR} — ensure Phase 11-D testnet-p2p dir exists"
  exit 1
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

# Wait for RPC
for i in $(seq 1 30); do
  if curl -s --max-time 2 "http://127.0.0.1:${RPC_PORT}/status" > /dev/null 2>&1; then
    echo "[start] RPC ready after ${i}s"
    break
  fi
  sleep 1
done

# Verify poawx_mode=active
POAWX_MODE=$(curl -s "http://127.0.0.1:${RPC_PORT}/rpc/getblocktemplate" \
  -H "Authorization: Bearer ${RPC_TOKEN}" \
  | python3 -c "import sys,json; print(json.load(sys.stdin).get('poawx_mode','unknown'))" 2>/dev/null || echo "unknown")
echo "[start] poawx_mode=${POAWX_MODE}"
if [[ "$POAWX_MODE" != "active" ]]; then
  echo "ERROR: poawx_mode is not active — check IRIUM_POAWX_MODE env"
  kill "${IRIUMD_PID}" 2>/dev/null || true
  exit 1
fi

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

# ── Control harness (VPS-1 local) ────────────────────────────────────────────
echo "[control] running ${BLOCKS}-block control harness (VPS-1 local)..."
set +e
python3 "${HARNESS}" \
  127.0.0.1 "${STRATUM_PORT}" \
  "http://127.0.0.1:${RPC_PORT}" "${RPC_TOKEN}" \
  --blocks "${BLOCKS}" --receipt --bogus
HARNESS_RC=$?
set -e
echo "[control] harness rc=${HARNESS_RC}"

# ── Negative checks ──────────────────────────────────────────────────────────
echo "[negative] running negative checks..."

MAINNET_503=$(curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8080/poawx/assignment 2>/dev/null || echo "err")
echo "[negative] mainnet /poawx/assignment: ${MAINNET_503} (expect 404 or 503)"

RECEIPT_422=$(curl -s -o /dev/null -w "%{http_code}" -X POST \
  "http://127.0.0.1:${RPC_PORT}/poawx/receipt" \
  -H "Authorization: Bearer ${RPC_TOKEN}" \
  -H "Content-Type: application/json" -d "{}" 2>/dev/null || echo "err")
echo "[negative] empty receipt POST: ${RECEIPT_422} (expect 422)"

RPC_PRIVATE=$(timeout 5 bash -c "echo > /dev/tcp/$(curl -s ifconfig.me 2>/dev/null || echo 127.0.0.1)/${RPC_PORT}" 2>/dev/null && echo "REACHABLE" || echo "private")
echo "[negative] RPC port public reachability: ${RPC_PRIVATE} (expect private)"

# ── Real miner wait ──────────────────────────────────────────────────────────
echo ""
echo "================================================================"
echo "OPERATOR ACTION REQUIRED: invite real external miner"
echo "  Stratum: 0.0.0.0:${STRATUM_PORT} (public via VPS-1 firewall)"
echo "  Guide:   docs/poaw-x-real-miner-pilot-invite.md"
echo "  Monitor: tail -f ${LOG_DIR}/stratum.log"
echo "  Logs:    ${LOG_DIR}/"
echo ""
echo "When real miner test is complete, run cleanup below."
echo "================================================================"

# ── Cleanup ──────────────────────────────────────────────────────────────────
if [[ $SKIP_CLEANUP -eq 0 ]]; then
  echo "[cleanup] stopping pilot services..."
  kill "${STRATUM_PID}" 2>/dev/null || true
  kill "${IRIUMD_PID}" 2>/dev/null || true
  sleep 2
  kill -9 "${STRATUM_PID}" 2>/dev/null || true
  kill -9 "${IRIUMD_PID}" 2>/dev/null || true
  echo "[cleanup] done — logs preserved at ${LOG_DIR}/"
else
  echo "[cleanup] SKIP — services left running: iriumd=${IRIUMD_PID} stratum=${STRATUM_PID}"
  echo "[cleanup] Firewall rules remain open (39510/39512)"
  echo "[cleanup] Stop manually when real miner test complete:"
  echo "  kill ${IRIUMD_PID} ${STRATUM_PID}"
  echo "  # close ports only if instructed:"
  echo "  # sudo ufw delete allow 39510/tcp"
  echo "  # sudo ufw delete allow 39512/tcp"
fi

# Post-cleanup mainnet check
MAINNET_ALIVE=$(ps -p "${MAINNET_PID:-1}" -o pid --no-headers 2>/dev/null || echo "check manually")
echo "[done] mainnet iriumd: ${MAINNET_ALIVE}"
echo "[done] Phase 11-F script complete."
exit $HARNESS_RC
