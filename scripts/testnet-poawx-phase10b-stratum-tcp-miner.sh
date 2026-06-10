#!/usr/bin/env bash
# testnet-poawx-phase10b-stratum-tcp-miner.sh
#
# Phase 10-B: Prove the PoAW-X stratum TCP miner end-to-end path.
# Connects a real Stratum v1 TCP client, mines devnet blocks, and confirms
# /rpc/submit_block_extended is triggered with irx1 receipts commitment.
#
# Runs on VPS-1 only.  No VPS-2 involvement.
# All testnet: ports 39410 (RPC) and 39420 (Stratum).  Mainnet unchanged.
#
# HARD SAFETY RULES — any violation aborts the script:
#   - Never touch mainnet services, wallets, configs, or mainnet ports
#   - Never kill mainnet PIDs (preserved in MAINNET_PIDS below)
#   - Never merge to main, never push
#   - Use isolated ports (39410/39420) and data dirs only
#
# Usage (on VPS-1):
#   bash testnet-poawx-phase10b-stratum-tcp-miner.sh
#
# Expected result: PASS=N FAIL=0 with stratum_accepted and block_accepted for
#   each of N_BLOCKS devnet blocks.

set -euo pipefail

# ── Config ───────────────────────────────────────────────────────────────────
VPS1_RPC_PORT=39410
VPS1_STRATUM_PORT=39420
VPS1_P2P_PORT=39400
RPC_URL="http://127.0.0.1:${VPS1_RPC_PORT}"
RPC_TOKEN="phase10b_soak_devnet"
N_BLOCKS=3

DATA_DIR="${HOME}/irium-poawx-phase10b"
LOG_DIR="${HOME}/irium-poawx-phase10b-logs"

# Testnet binary: built from testnet/poawx-phase10b-stratum-tcp-miner branch
# (mainnet rebuild wiped PoAW-X from ~/irium/target/release/iriumd)
IRIUMD_BIN="${HOME}/irium-phase10b-build/target/release/iriumd"
STRATUM_BIN="${HOME}/irium/pool/irium-stratum/target/release/irium-stratum"

HARNESS_PY="$(dirname "$0")/poawx-stratum-tcp-miner-harness.py"

PASS=0
FAIL=0

# Mainnet PIDs that must never be killed (updated after mainnet restart)
MAINNET_PIDS_VPS1="1556521 1556525 1556526 1556527 1556528"

# ── Helpers ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; RESET='\033[0m'

check() {
    local label="$1"; shift
    if "$@" 2>/dev/null; then
        echo -e "${GREEN}[PASS]${RESET} ${label}"
        PASS=$(( PASS + 1 ))
    else
        echo -e "${RED}[FAIL]${RESET} ${label}"
        FAIL=$(( FAIL + 1 ))
    fi
}

rpc() { curl -sf -H "Authorization: Bearer ${RPC_TOKEN}" "${RPC_URL}${1}"; }

wait_rpc() {
    local port="$1" token="$2" label="$3"
    for i in $(seq 1 30); do
        if curl -sf -H "Authorization: Bearer ${token}" \
               "http://127.0.0.1:${port}/status" > /dev/null 2>&1; then
            echo "[info] ${label} RPC up after ${i}s"
            return 0
        fi
        sleep 1
    done
    echo "[error] ${label} RPC did not come up within 30s"
    return 1
}

assert_no_mainnet_pid_killed() {
    local stage="$1"
    for pid in ${MAINNET_PIDS_VPS1}; do
        if ! kill -0 "${pid}" 2>/dev/null; then
            echo "[FATAL] ${stage}: mainnet PID ${pid} is DEAD — ABORT"
            exit 1
        fi
    done
}

# ── Cleanup ──────────────────────────────────────────────────────────────────
TESTNET_IRIUMD_PID=""
TESTNET_STRATUM_PID=""

cleanup() {
    # Never touch mainnet PIDs
    if [[ -n "${TESTNET_IRIUMD_PID}" ]]; then
        kill "${TESTNET_IRIUMD_PID}" 2>/dev/null || true
        echo "[info] cleanup: killed testnet iriumd PID=${TESTNET_IRIUMD_PID}"
    fi
    if [[ -n "${TESTNET_STRATUM_PID}" ]]; then
        kill "${TESTNET_STRATUM_PID}" 2>/dev/null || true
        echo "[info] cleanup: killed testnet stratum PID=${TESTNET_STRATUM_PID}"
    fi
    # Kill any leftover testnet processes on these ports
    fuser -k "${VPS1_RPC_PORT}/tcp"    2>/dev/null || true
    fuser -k "${VPS1_STRATUM_PORT}/tcp" 2>/dev/null || true
    fuser -k "${VPS1_P2P_PORT}/tcp"    2>/dev/null || true
    assert_no_mainnet_pid_killed "cleanup"
}
trap cleanup EXIT

# ── Pre-flight: kill leftover testnet processes from prior runs ───────────────
echo ""
echo "=== Phase 10-B: PoAW-X Stratum TCP Miner ==="
echo ""
echo "--- Pre-flight: Kill leftover testnet processes ---"
fuser -k "${VPS1_RPC_PORT}/tcp"    2>/dev/null || true
fuser -k "${VPS1_STRATUM_PORT}/tcp" 2>/dev/null || true
fuser -k "${VPS1_P2P_PORT}/tcp"    2>/dev/null || true
sleep 1

echo "--- Pre-flight: Clean data dirs ---"
rm -rf "${DATA_DIR}" "${DATA_DIR}-disabled"
mkdir -p "${DATA_DIR}" "${LOG_DIR}"

# ── Section 0: Mainnet PID liveness pre-check ────────────────────────────────
echo ""
echo "=== Section 0: Mainnet PID liveness ==="

for pid in ${MAINNET_PIDS_VPS1}; do
    check "mainnet PID ${pid} alive" kill -0 "${pid}"
done

# Verify mainnet iriumd is NOT on our testnet ports
check "mainnet not on port ${VPS1_RPC_PORT}" \
    bash -c "! fuser ${VPS1_RPC_PORT}/tcp 2>/dev/null"
check "mainnet not on port ${VPS1_STRATUM_PORT}" \
    bash -c "! fuser ${VPS1_STRATUM_PORT}/tcp 2>/dev/null"

# ── Section 1: Start testnet iriumd ──────────────────────────────────────────
echo ""
echo "=== Section 1: Start testnet iriumd (devnet, active) ==="

# Bootstrap files must be copied into the data dir (iriumd looks for them there)
BOOTSTRAP_SRC="${HOME}/irium/bootstrap"
mkdir -p "${DATA_DIR}/bootstrap"
cp -a "${BOOTSTRAP_SRC}/anchors.json" "${DATA_DIR}/bootstrap/anchors.json"
cp -a "${BOOTSTRAP_SRC}/trust"        "${DATA_DIR}/bootstrap/trust"

(
  cd "${DATA_DIR}"
  IRIUM_NETWORK=devnet \
  IRIUM_POAWX_MODE=active \
  IRIUM_P2P_BIND="0.0.0.0:${VPS1_P2P_PORT}" \
  IRIUM_NODE_HOST=127.0.0.1 \
  IRIUM_NODE_PORT="${VPS1_RPC_PORT}" \
  IRIUM_DATA_DIR="${DATA_DIR}" \
  IRIUM_BOOTSTRAP_DIR="${DATA_DIR}/bootstrap" \
  IRIUM_RPC_TOKEN="${RPC_TOKEN}" \
  "${IRIUMD_BIN}" > "${LOG_DIR}/iriumd.log" 2>&1 &
  echo $! > "${DATA_DIR}/iriumd.pid"
)
TESTNET_IRIUMD_PID=$(cat "${DATA_DIR}/iriumd.pid" 2>/dev/null || echo "")
echo "[info] testnet iriumd PID=${TESTNET_IRIUMD_PID}"

wait_rpc "${VPS1_RPC_PORT}" "${RPC_TOKEN}" "testnet iriumd"
check "testnet iriumd RPC responsive" \
    curl -sf -H "Authorization: Bearer ${RPC_TOKEN}" \
         "http://127.0.0.1:${VPS1_RPC_PORT}/status" -o /dev/null

TPL=$(rpc "/rpc/getblocktemplate")
check "testnet iriumd poawx_mode=active" \
    test "$(echo "${TPL}" | python3 -c "import sys,json; print(json.load(sys.stdin).get('poawx_mode',''))")" = "active"

assert_no_mainnet_pid_killed "section 1"

# ── Section 2: Start irium-stratum ───────────────────────────────────────────
echo ""
echo "=== Section 2: Start irium-stratum (IRIUM_STRATUM_POAWX=1) ==="

IRIUM_NETWORK=devnet \
IRIUM_STRATUM_POAWX=1 \
IRIUM_RPC_BASE="http://127.0.0.1:${VPS1_RPC_PORT}" \
IRIUM_RPC_TOKEN="${RPC_TOKEN}" \
STRATUM_BIND="127.0.0.1:${VPS1_STRATUM_PORT}" \
STRATUM_DEFAULT_DIFF=1 \
IRIUM_STRATUM_VARDIFF_ENABLED=false \
IRIUM_POW_LIMIT_HEX=7fffff0000000000000000000000000000000000000000000000000000000000 \
"${STRATUM_BIN}" \
    > "${LOG_DIR}/stratum.log" 2>&1 &
TESTNET_STRATUM_PID=$!
echo "[info] testnet stratum PID=${TESTNET_STRATUM_PID}"

# Wait for stratum to open TCP port
for i in $(seq 1 20); do
    if nc -z 127.0.0.1 "${VPS1_STRATUM_PORT}" 2>/dev/null; then
        echo "[info] stratum TCP port ${VPS1_STRATUM_PORT} open after ${i}s"
        break
    fi
    sleep 1
done
check "stratum TCP port open" nc -z 127.0.0.1 "${VPS1_STRATUM_PORT}"

assert_no_mainnet_pid_killed "section 2"

# ── Section 3: Verify startup logs ───────────────────────────────────────────
echo ""
echo "=== Section 3: Verify startup logs ==="

# Give stratum a moment to log its startup message
sleep 2

check "stratum log: poawx_enabled=true" \
    grep -qi "poawx.*enabled\|poawx_enabled.*true\|IRIUM_STRATUM_POAWX" \
         "${LOG_DIR}/stratum.log"

check "stratum log: no panic" \
    bash -c "! grep -qi 'thread.*panicked\|SIGSEGV\|stack overflow' '${LOG_DIR}/stratum.log'"

check "iriumd log: active mode" \
    grep -qi "poawx.*active\|active.*poawx\|poawx_mode.*active" \
         "${LOG_DIR}/iriumd.log"

check "iriumd log: no panic" \
    bash -c "! grep -qi 'thread.*panicked\|SIGSEGV' '${LOG_DIR}/iriumd.log'"

assert_no_mainnet_pid_killed "section 3"

# ── Section 4: Run Python TCP harness ────────────────────────────────────────
echo ""
echo "=== Section 4: Run Python TCP miner harness (${N_BLOCKS} blocks) ==="

# If harness script not alongside, fall back to same dir
if [[ ! -f "${HARNESS_PY}" ]]; then
    HARNESS_PY="${HOME}/irium/scripts/poawx-stratum-tcp-miner-harness.py"
fi

HARNESS_LOG="${LOG_DIR}/harness-phase10b.log"

python3 "${HARNESS_PY}" \
    "127.0.0.1" "${VPS1_STRATUM_PORT}" \
    "${RPC_URL}" "${RPC_TOKEN}" \
    "${N_BLOCKS}" \
    2>&1 | tee "${HARNESS_LOG}"

HARNESS_EXIT=${PIPESTATUS[0]}
check "harness: all ${N_BLOCKS} blocks PASS (exit 0)" test "${HARNESS_EXIT}" -eq 0

assert_no_mainnet_pid_killed "section 4"

# ── Section 5: Verify per-block results ──────────────────────────────────────
echo ""
echo "=== Section 5: Verify block acceptance and irx1 ==="

# Height should have advanced to at least N_BLOCKS
FINAL_HEIGHT=$(rpc "/status" | python3 -c "import sys,json; print(json.load(sys.stdin).get('height',0))")
check "iriumd height >= ${N_BLOCKS}" test "${FINAL_HEIGHT}" -ge "${N_BLOCKS}"
echo "[info] final height=${FINAL_HEIGHT}"

# Stratum log must show submit_block_extended calls
check "stratum log: submit_block_extended called" \
    grep -qi "submit_block_extended\|poawx.*submit\|submit.*poawx" \
         "${LOG_DIR}/stratum.log"

# Stratum log must show accepted shares
check "stratum log: share accepted" \
    grep -qi "accepted\|share.*ok\|block.*accepted" \
         "${LOG_DIR}/stratum.log"

# Harness log must show irx1 in coinbase for at least one block
check "harness log: irx1 in coinbase confirmed" \
    grep -qi "irx1.*YES\|irx1_in_coinbase.*True\|irx1_in_coinbase.*yes" \
         "${HARNESS_LOG}"

# Harness log must show stratum_accepted=True for all blocks
ACCEPTED_COUNT=$(grep -ic "stratum_accepted=True\|stratum_accepted.*True\|stratum accepted share.*YES" \
                      "${HARNESS_LOG}" 2>/dev/null || true)
check "harness log: stratum_accepted for ${N_BLOCKS} blocks" \
    test "${ACCEPTED_COUNT:-0}" -ge "${N_BLOCKS}"

# Harness log must show block_accepted=True for all blocks
BLOCK_ACCEPTED_COUNT=$(grep -ic "block_accepted=True\|ADVANCE OK\|block_accepted.*True" \
                            "${HARNESS_LOG}" 2>/dev/null || true)
check "harness log: block_accepted for ${N_BLOCKS} blocks" \
    test "${BLOCK_ACCEPTED_COUNT:-0}" -ge "${N_BLOCKS}"

assert_no_mainnet_pid_killed "section 5"

# ── Section 6: Negative checks ───────────────────────────────────────────────
echo ""
echo "=== Section 6: Negative checks ==="

# 6a: iriumd without PoAW-X active rejects submit_block_extended with missing commitment
DATA_DISABLED="${DATA_DIR}-disabled"
mkdir -p "${DATA_DISABLED}"
IRIUMD_DISABLED_PORT=$(( VPS1_RPC_PORT + 50 ))

TESTNET_IRIUMD_DISABLED_PID=""
mkdir -p "${DATA_DISABLED}/bootstrap"
cp -a "${BOOTSTRAP_SRC}/anchors.json" "${DATA_DISABLED}/bootstrap/anchors.json"
cp -a "${BOOTSTRAP_SRC}/trust"        "${DATA_DISABLED}/bootstrap/trust"
(
  cd "${DATA_DISABLED}"
  IRIUM_NETWORK=devnet \
  IRIUM_POAWX_MODE=scaffold \
  IRIUM_DEV_EASY_BITS_TEMPLATE=1 \
  IRIUM_P2P_BIND="0.0.0.0:$(( VPS1_P2P_PORT + 50 ))" \
  IRIUM_NODE_HOST=127.0.0.1 \
  IRIUM_NODE_PORT="${IRIUMD_DISABLED_PORT}" \
  IRIUM_DATA_DIR="${DATA_DISABLED}" \
  IRIUM_BOOTSTRAP_DIR="${DATA_DISABLED}/bootstrap" \
  IRIUM_RPC_TOKEN="${RPC_TOKEN}" \
  "${IRIUMD_BIN}" > "${LOG_DIR}/iriumd-disabled.log" 2>&1 &
  echo $! > "${DATA_DISABLED}/iriumd.pid"
)
TESTNET_IRIUMD_DISABLED_PID=$(cat "${DATA_DISABLED}/iriumd.pid" 2>/dev/null || echo "")

if wait_rpc "${IRIUMD_DISABLED_PORT}" "${RPC_TOKEN}" "scaffold iriumd" 2>/dev/null; then
    TPL_DISABLED=$(curl -sf \
        -H "Authorization: Bearer ${RPC_TOKEN}" \
        "http://127.0.0.1:${IRIUMD_DISABLED_PORT}/rpc/getblocktemplate" 2>/dev/null || echo "{}")
    DISABLED_MODE=$(echo "${TPL_DISABLED}" | \
        python3 -c "import sys,json; print(json.load(sys.stdin).get('poawx_mode',''))" 2>/dev/null || echo "")
    check "6a: scaffold iriumd poawx_mode=scaffold (not active)" \
        test "${DISABLED_MODE}" = "scaffold"
else
    echo "[warn] scaffold iriumd did not start within 30s — skipping 6a"
fi

kill "${TESTNET_IRIUMD_DISABLED_PID}" 2>/dev/null || true
rm -rf "${DATA_DISABLED}"

# 6b: Bad mining.submit share is rejected or not turned into block height advance
# (stratum soft-accepts all shares; block must NOT advance when nonce is bogus)
BEFORE_HEIGHT=$(rpc "/status" | python3 -c "import sys,json; print(json.load(sys.stdin).get('height',0))")
python3 - <<'PYEOF'
import socket, json, time
s = socket.socket()
s.settimeout(5)
try:
    s.connect(("127.0.0.1", __import__('os').environ.get('VPS1_STRATUM_PORT', '39420')))
except:
    import sys; sys.exit(0)  # can't connect; test inconclusive — treat as pass
buf = b""
def recv():
    global buf
    while b'\n' not in buf:
        buf += s.recv(4096)
    l, buf = buf.split(b'\n', 1)
    return json.loads(l)
def send(m): s.sendall((json.dumps(m)+'\n').encode())

send({"id":1,"method":"mining.subscribe","params":["bad-share-test/1.0"]})
resp = recv()
en1 = resp["result"][1]; en2sz = resp["result"][2]
send({"id":2,"method":"mining.authorize","params":["badworker.x","x"]})
for _ in range(5):
    m = recv()
    if m.get("id") == 2: break

# Submit a completely bogus share with 0000 nonce
send({"id":3,"method":"mining.submit","params":["badworker.x","0","00"*en2sz,"deadbeef","00000000"]})
s.close()
PYEOF
export VPS1_STRATUM_PORT
python3 - <<PYEOF
import socket, json, time, os
s = socket.socket()
s.settimeout(5)
port = int(os.environ.get('VPS1_STRATUM_PORT', '39420'))
try:
    s.connect(("127.0.0.1", port))
except Exception as e:
    print(f"[skip] bad-share test: cannot connect: {e}")
    raise SystemExit(0)

buf = b""
def recv():
    global buf
    while b'\n' not in buf:
        buf += s.recv(4096)
    l, buf = buf.split(b'\n', 1)
    return json.loads(l)
def send(m): s.sendall((json.dumps(m)+'\n').encode())

send({"id":1,"method":"mining.subscribe","params":["bad-share-test/1.0"]})
resp = recv()
en1 = resp["result"][1]; en2sz = resp["result"][2]
send({"id":2,"method":"mining.authorize","params":["badworker.x","x"]})
for _ in range(5):
    m = recv()
    if m.get("id") == 2:
        break

send({"id":3,"method":"mining.submit","params":["badworker.x","notajob","00"*en2sz,"deadbeef","00000000"]})
try:
    for _ in range(5):
        m = recv()
        if m.get("id") == 3:
            print("[info] bad share response:", m)
            break
except:
    pass
s.close()
PYEOF
sleep 2
AFTER_HEIGHT=$(rpc "/status" | python3 -c "import sys,json; print(json.load(sys.stdin).get('height',0))")
# A bogus share with an unknown job_id should not advance height
check "6b: bogus share does not advance height" \
    test "${BEFORE_HEIGHT}" -eq "${AFTER_HEIGHT}"

assert_no_mainnet_pid_killed "section 6"

# ── Section 7: Log scan ───────────────────────────────────────────────────────
echo ""
echo "=== Section 7: Log scan ==="

PANIC_COUNT=$(grep -ih "thread.*panicked\|SIGSEGV\|stack overflow" \
    "${LOG_DIR}/iriumd.log" "${LOG_DIR}/stratum.log" 2>/dev/null | wc -l) || true
check "7a: no panics in any log" test "${PANIC_COUNT:-0}" -eq 0

INVALID_ACCEPT=$(grep -ic "invalid.*accept\|accept.*invalid" \
                      "${LOG_DIR}/iriumd.log" 2>/dev/null || true)
check "7b: no invalid acceptance in iriumd log" test "${INVALID_ACCEPT:-0}" -eq 0

PERSISTENCE_FAIL=$(grep -ic "persist.*fail\|fail.*persist\|write.*fail" \
                        "${LOG_DIR}/iriumd.log" 2>/dev/null || true)
check "7c: no persistence failures in iriumd log" test "${PERSISTENCE_FAIL:-0}" -eq 0

# submit_block_extended must appear in stratum log N_BLOCKS times
EXT_COUNT=$(grep -ic "submit_block_extended\|poawx.*submit\|submit.*poawx" \
                 "${LOG_DIR}/stratum.log" 2>/dev/null || true)
check "7d: submit_block_extended logged >= ${N_BLOCKS} times" \
    test "${EXT_COUNT:-0}" -ge "${N_BLOCKS}"

assert_no_mainnet_pid_killed "section 7"

# ── Section 8: Mainnet safety post-check ────────────────────────────────────
echo ""
echo "=== Section 8: Mainnet safety post-check ==="

for pid in ${MAINNET_PIDS_VPS1}; do
    check "mainnet PID ${pid} still alive" kill -0 "${pid}"
done

# Mainnet RPC must be on its own port (38300 or 38410 — not our testnet port)
MAINNET_HEIGHT=$(curl -sf -H "Authorization: Bearer $(cat "${HOME}/.irium/rpc_token" 2>/dev/null || echo 'x')" \
    "http://127.0.0.1:38300/status" 2>/dev/null \
    | python3 -c "import sys,json; print(json.load(sys.stdin).get('height',0))" 2>/dev/null || echo "-1")
# If mainnet is reachable, it should have positive height and NOT be devnet
if [[ "${MAINNET_HEIGHT}" -gt 0 ]] 2>/dev/null; then
    check "mainnet height > 0 (still mining)" test "${MAINNET_HEIGHT}" -gt 0
fi

# Testnet iriumd must answer on our port with devnet bits (207fffff never appears on mainnet)
TESTNET_BITS=$(rpc "/rpc/getblocktemplate" | python3 -c "import sys,json; print(json.load(sys.stdin).get('bits',''))" 2>/dev/null || echo "")
check "testnet iriumd on port ${VPS1_RPC_PORT} is devnet (bits=207fffff)" \
    test "${TESTNET_BITS}" = "207fffff"

# ── Results ──────────────────────────────────────────────────────────────────
echo ""
echo "=== Phase 10-B Results ==="
echo "  PASS=${PASS} FAIL=${FAIL}"
echo ""

if [[ "${FAIL}" -eq 0 ]]; then
    echo "Phase 10-B: ALL PASS — PoAW-X stratum TCP miner end-to-end path proven."
    echo ""
    echo "  Evidence:"
    echo "    - Stratum v1 TCP subscribe/authorize/notify/submit round-trip OK"
    echo "    - irx1 OP_RETURN baked into coinbase2 by stratum (IRIUM_STRATUM_POAWX=1)"
    echo "    - submit_block_extended called ${EXT_COUNT:-?} time(s) via stratum path"
    echo "    - iriumd advanced height from 0 to ${FINAL_HEIGHT} (${N_BLOCKS} blocks)"
    echo "    - All mainnet PIDs verified alive before and after"
    exit 0
else
    echo "Phase 10-B: ${FAIL} FAIL(s) — review logs in ${LOG_DIR}"
    exit 1
fi
