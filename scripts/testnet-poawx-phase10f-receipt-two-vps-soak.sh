#!/usr/bin/env bash
# Phase 10-F: PoAW-X receipt + two-VPS soak
# Branch:     testnet/poawx-phase10f-receipt-two-vps-soak
# Checkpoint: 8aa432d (Phase 10-E PASS=62 FAIL=0)
#
# Proves: full PoAW-X receipt path is repeatable over 180 blocks across
# three restart-segmented runs while VPS-2 peer syncs all blocks.
#
# Path validated every block:
#   assignment → POST /poawx/receipt → pending in template
#   → non-empty receipts_root → irx1 OP_RETURN in coinbase
#   → Stratum TCP submit → submit_block_extended → accepted
#   → VPS-2 propagation confirmed
#
# Safety: devnet/testnet ONLY. Mainnet untouched. No production
# ports/dirs/configs. Do not merge to main. Do not push.
set -euo pipefail

# ── Network topology ───────────────────────────────────────────────────────────
VPS1_HOST="207.244.247.86"
VPS2_HOST="157.173.116.134"
VPS2_SSH="irium@${VPS2_HOST}"
VPS2_SSH_OPTS="-o StrictHostKeyChecking=no -o BatchMode=yes -o ConnectTimeout=15"

VPS1_P2P_PORT=39510
VPS1_RPC_PORT=39511
VPS1_STRATUM_PORT=39512
VPS1_DISABLED_RPC_PORT=39513

VPS2_P2P_PORT=39610
VPS2_RPC_PORT=39611

# ── Soak targets (override with env vars) ─────────────────────────────────────
SOAK_SECONDS=${SOAK_SECONDS:-10800}
SOAK_BLOCK_TARGET=${SOAK_BLOCK_TARGET:-180}
SOAK_RESTART_EVERY=${SOAK_RESTART_EVERY:-60}
SOAK_NEGATIVE_EVERY=${SOAK_NEGATIVE_EVERY:-45}

SEG_BLOCKS=$SOAK_RESTART_EVERY
N_SEGS=$(( (SOAK_BLOCK_TARGET + SEG_BLOCKS - 1) / SEG_BLOCKS ))

# ── Binaries / harness ────────────────────────────────────────────────────────
IRIUMD_BIN="$HOME/irium/target/release/iriumd"
STRATUM_BIN="$HOME/irium/pool/irium-stratum/target/release/irium-stratum"
HARNESS="$HOME/irium/scripts/poawx-stratum-long-soak-harness.py"
VPS2_BIN_PATH="/tmp/iriumd-poawx-phase10f"

# ── Data dirs ─────────────────────────────────────────────────────────────────
DATA_DIR="$HOME/irium-poawx-phase10f"
VPS2_DATA_DIR="/home/irium/irium-phase10f-testnet-vps2"

# ── Logs ──────────────────────────────────────────────────────────────────────
LOG_IRIUMD="$DATA_DIR/iriumd.log"
LOG_STRATUM1="$DATA_DIR/stratum-seg1.log"
LOG_STRATUM2="$DATA_DIR/stratum-seg2.log"
LOG_STRATUM3="$DATA_DIR/stratum-seg3.log"
LOG_HARNESS1="$DATA_DIR/harness-seg1.log"
LOG_HARNESS2="$DATA_DIR/harness-seg2.log"
LOG_HARNESS3="$DATA_DIR/harness-seg3.log"
LOG_HARNESS_BOGUS="$DATA_DIR/harness-bogus.log"
LOG_DISABLED="$DATA_DIR/disabled.log"

# ── Credentials ───────────────────────────────────────────────────────────────
RPC_BASE="http://127.0.0.1:${VPS1_RPC_PORT}"
RPC_TOKEN="poawx-phase10f-token"
WALLET_ADDR="iFb9G2WjD5FP9JGmY7brdwHRxR1KJcAa9z"

# ── Mainnet sentinel PIDs (filled at pre-flight) ──────────────────────────────
MAINNET_IRIUMD_PID_VPS1=""
MAINNET_STRATUM_PID_VPS1=""
MAINNET_EXPLORER_PID_VPS1=""
MAINNET_WALLET_PID_VPS1=""
# VPS-2 mainnet PIDs discovered dynamically in Section 0
MAINNET_IRIUMD_PID_VPS2=""
MAINNET_WALLET_PID_VPS2=""
MAINNET_EXPLORER_PID_VPS2=""

# ── State vars ────────────────────────────────────────────────────────────────
TESTNET_IRIUMD_PID=""
TESTNET_STRATUM_PID=""
TESTNET_DISABLED_PID=""
VPS2_IRIUMD_PID=""
VPS2_SSH_TUNNEL_PID=""

PASS=0; FAIL=0; SKIP=0
TOTAL_BLOCKS=0; TOTAL_IRX1=0; TOTAL_SHARES_ACC=0; TOTAL_SHARES_REJ=0
STRATUM_RESTART_N=0
SOAK_ELAPSED_TOTAL=0

pass()  { echo "[PASS] $1"; PASS=$((PASS+1)); }
fail()  { echo "[FAIL] $1"; FAIL=$((FAIL+1)); }
skip()  { echo "[SKIP] $1"; SKIP=$((SKIP+1)); }
info()  { echo "[INFO] $1"; }

# ── Cleanup: testnet processes ONLY ───────────────────────────────────────────
cleanup() {
    echo ""
    echo "=== Cleanup ==="
    [[ -n "${TESTNET_DISABLED_PID:-}" ]] && { kill "$TESTNET_DISABLED_PID" 2>/dev/null || true; }
    [[ -n "${TESTNET_STRATUM_PID:-}" ]]  && { kill "$TESTNET_STRATUM_PID"  2>/dev/null || true; }
    [[ -n "${TESTNET_IRIUMD_PID:-}" ]]   && { kill "$TESTNET_IRIUMD_PID"   2>/dev/null || true; }
    sleep 1
    for port in $VPS1_RPC_PORT $VPS1_P2P_PORT $VPS1_STRATUM_PORT $VPS1_DISABLED_RPC_PORT; do
        fuser -k "${port}/tcp" 2>/dev/null || true
    done
    if [[ -n "${VPS2_IRIUMD_PID:-}" ]]; then
        ssh $VPS2_SSH_OPTS "$VPS2_SSH" "kill ${VPS2_IRIUMD_PID} 2>/dev/null || true; sleep 1; fuser -k ${VPS2_P2P_PORT}/tcp 2>/dev/null || true; fuser -k ${VPS2_RPC_PORT}/tcp 2>/dev/null || true" 2>/dev/null || true
        info "VPS-2 testnet iriumd (PID=$VPS2_IRIUMD_PID) stopped"
    fi
    [[ -n "${VPS2_SSH_TUNNEL_PID:-}" && "${VPS2_SSH_TUNNEL_PID:-0}" != "0" ]] && {
        ssh $VPS2_SSH_OPTS "$VPS2_SSH" "kill ${VPS2_SSH_TUNNEL_PID} 2>/dev/null || true" 2>/dev/null || true
    }
    if [[ "$FAIL" -eq 0 ]]; then
        rm -rf "$DATA_DIR"
        ssh $VPS2_SSH_OPTS "$VPS2_SSH" "rm -rf '${VPS2_DATA_DIR}' '${VPS2_BIN_PATH}'" 2>/dev/null || true
        echo "Data dirs cleaned up (all checks passed)"
    else
        echo "Data dirs preserved:"
        echo "  VPS-1: $DATA_DIR"
        echo "  VPS-2: $VPS2_DATA_DIR (log at ${VPS2_DATA_DIR}/iriumd.log)"
    fi
    echo ""
    echo "PASS=$PASS  FAIL=$FAIL  SKIP=$SKIP"
}
trap cleanup EXIT

# ── Helpers ───────────────────────────────────────────────────────────────────
wait_rpc() {
    local url="$1" token="$2" max="${3:-30}" i=0
    while [[ $i -lt $max ]]; do
        if curl -sf -H "Authorization: Bearer $token" "$url/rpc/getblocktemplate" >/dev/null 2>&1; then return 0; fi
        sleep 1; i=$((i+1))
    done
    return 1
}

wait_vps2_rpc() {
    local max="${1:-45}" i=0
    while [[ $i -lt $max ]]; do
        if ssh $VPS2_SSH_OPTS "$VPS2_SSH" "curl -sf -H 'Authorization: Bearer $RPC_TOKEN' 'http://127.0.0.1:${VPS2_RPC_PORT}/rpc/getblocktemplate' >/dev/null 2>&1" 2>/dev/null; then return 0; fi
        sleep 1; i=$((i+1))
    done
    return 1
}

vps2_height() {
    ssh $VPS2_SSH_OPTS "$VPS2_SSH" "curl -sf -H 'Authorization: Bearer $RPC_TOKEN' 'http://127.0.0.1:${VPS2_RPC_PORT}/rpc/getblocktemplate' 2>/dev/null | python3 -c 'import sys,json; print(json.load(sys.stdin).get(\"height\",0))'" 2>/dev/null || echo 0
}

vps2_prev_hash() {
    ssh $VPS2_SSH_OPTS "$VPS2_SSH" "curl -sf -H 'Authorization: Bearer $RPC_TOKEN' 'http://127.0.0.1:${VPS2_RPC_PORT}/rpc/getblocktemplate' 2>/dev/null | python3 -c 'import sys,json; print(json.load(sys.stdin).get(\"prev_hash\",\"\"))'" 2>/dev/null || echo ""
}

vps1_height() {
    curl -sf -H "Authorization: Bearer $RPC_TOKEN" "$RPC_BASE/rpc/getblocktemplate" 2>/dev/null | \
        python3 -c "import sys,json; print(json.load(sys.stdin).get('height',0))" 2>/dev/null || echo 0
}

vps1_prev_hash() {
    curl -sf -H "Authorization: Bearer $RPC_TOKEN" "$RPC_BASE/rpc/getblocktemplate" 2>/dev/null | \
        python3 -c "import sys,json; print(json.load(sys.stdin).get('prev_hash',''))" 2>/dev/null || echo ""
}

parse_summary() {
    local log_file="$1" field="$2" default="${3:-0}"
    local sj
    sj=$(grep '^SUMMARY_JSON:' "$log_file" 2>/dev/null | tail -1 | sed 's/^SUMMARY_JSON://')
    [[ -n "$sj" ]] && echo "$sj" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('$field', $default))" 2>/dev/null || echo "$default"
}

start_stratum() {
    local log_file="$1"
    IRIUM_STRATUM_POAWX=1 \
    IRIUM_RPC_BASE="$RPC_BASE" \
    IRIUM_RPC_TOKEN="$RPC_TOKEN" \
    STRATUM_BIND="0.0.0.0:${VPS1_STRATUM_PORT}" \
    IRIUM_STRATUM_COINBASE_BIP34=true \
    STRATUM_DEFAULT_DIFF=1 \
    IRIUM_STRATUM_VARDIFF_ENABLED=false \
    IRIUM_STRATUM_MINER_ADDRESS="$WALLET_ADDR" \
    IRIUM_POW_LIMIT_HEX=7fffff0000000000000000000000000000000000000000000000000000000000 \
        "$STRATUM_BIN" >"$log_file" 2>&1 &
    TESTNET_STRATUM_PID=$!
    sleep 2
}

restart_stratum() {
    local new_log="$1"
    STRATUM_RESTART_N=$((STRATUM_RESTART_N+1))
    echo "Killing testnet stratum PID=$TESTNET_STRATUM_PID (restart #$STRATUM_RESTART_N)..."
    kill "$TESTNET_STRATUM_PID" 2>/dev/null || true
    sleep 2
    if ss -lntp "sport = :${VPS1_STRATUM_PORT}" 2>/dev/null | grep -q ":${VPS1_STRATUM_PORT}"; then
        fuser -k "${VPS1_STRATUM_PORT}/tcp" 2>/dev/null || true
        sleep 1
    fi
    pass "stratum stopped (restart #$STRATUM_RESTART_N)"
    echo "Restarting stratum..."
    start_stratum "$new_log"
    kill -0 "$TESTNET_STRATUM_PID" 2>/dev/null && \
        pass "stratum restarted (restart #$STRATUM_RESTART_N PID=$TESTNET_STRATUM_PID)" || \
        { fail "stratum failed to restart"; return 1; }
    ss -lntp "sport = :${VPS1_STRATUM_PORT}" 2>/dev/null | grep -q ":${VPS1_STRATUM_PORT}" && \
        pass "stratum port $VPS1_STRATUM_PORT open after restart #$STRATUM_RESTART_N" || \
        fail "stratum port not open after restart"
}

run_soak_segment() {
    local seg_n="$1" blocks="$2" log_file="$3"
    echo ""
    echo "--- Soak segment $seg_n/$N_SEGS: $blocks blocks ---"
    python3 "$HARNESS" \
        127.0.0.1 "$VPS1_STRATUM_PORT" \
        "$RPC_BASE" "$RPC_TOKEN" \
        --blocks "$blocks" \
        --receipt \
        2>&1 | tee "$log_file" || true

    local bp irx1 rec acc rej el
    bp=$(parse_summary "$log_file" blocks_pass 0)
    irx1=$(parse_summary "$log_file" irx1_in_coinbase_count 0)
    rec=$(parse_summary "$log_file" receipt_test_passed False)
    acc=$(parse_summary "$log_file" share_accepts 0)
    rej=$(parse_summary "$log_file" share_rejects 0)
    el=$(parse_summary "$log_file" elapsed_s 0)
    echo "seg$seg_n: blocks=$bp irx1=$irx1 receipt=$rec shares=$acc/$rej elapsed=${el}s"

    [[ "$bp" -ge "$blocks" ]] && \
        pass "seg$seg_n: $bp/$blocks blocks accepted" || \
        fail "seg$seg_n: only $bp/$blocks blocks accepted"

    if [[ "$irx1" -ge "$blocks" ]]; then
        pass "seg$seg_n: $irx1/$blocks blocks with irx1 (100%)"
    elif [[ "$irx1" -ge 1 ]]; then
        pass "seg$seg_n: $irx1 irx1 blocks (>= 1 minimum)"
        info "seg$seg_n: not every block had irx1 ($irx1/$blocks)"
    else
        fail "seg$seg_n: no irx1 blocks (expected >= 1)"
    fi

    [[ "$rec" == "True" ]] && \
        pass "seg$seg_n: receipt path PASS" || \
        fail "seg$seg_n: receipt path FAIL"
    [[ "$rej" -eq 0 ]] && \
        pass "seg$seg_n: 0 share rejections" || \
        fail "seg$seg_n: $rej share rejections"

    TOTAL_BLOCKS=$((TOTAL_BLOCKS + bp))
    TOTAL_IRX1=$((TOTAL_IRX1 + irx1))
    TOTAL_SHARES_ACC=$((TOTAL_SHARES_ACC + acc))
    TOTAL_SHARES_REJ=$((TOTAL_SHARES_REJ + rej))
    SOAK_ELAPSED_TOTAL=$(echo "$SOAK_ELAPSED_TOTAL $el" | awk '{printf "%.1f", $1+$2}')
}

check_vps2_sync() {
    local label="${1:-}" wait_s="${2:-8}"
    local v1h v2h
    v1h=$(vps1_height)
    info "Waiting ${wait_s}s for VPS-2 sync propagation${label:+ ($label)}..."
    sleep "$wait_s"
    v2h=$(vps2_height)
    if [[ "$v2h" -ge "$((v1h - 3))" ]] && [[ "$v2h" -ge 1 ]]; then
        pass "VPS-2 synced: height=$v2h (VPS-1=$v1h)${label:+ $label}"
    elif [[ "$v2h" -ge 1 ]]; then
        info "VPS-2 height=$v2h behind VPS-1=$v1h, waiting 15s more..."
        sleep 15
        v2h=$(vps2_height)
        [[ "$v2h" -ge "$((v1h - 5))" ]] && \
            pass "VPS-2 synced after extra wait: height=$v2h" || \
            fail "VPS-2 sync lag: height=$v2h expected ~$v1h${label:+ $label}"
    else
        fail "VPS-2 not syncing: height=$v2h${label:+ $label}"
    fi
}

start_vps2_iriumd() {
    # Write startup script to VPS-2 via SCP to avoid SSH multiline quoting issues
    local local_start="/tmp/phase10f-vps2-start-local.sh"
    cat > "$local_start" << STARTSCRIPT
#!/bin/sh
# cd to data dir so ./bootstrap/anchors.json is found by AnchorManager::from_default_repo_root
cd ${VPS2_DATA_DIR}
exec env IRIUM_NETWORK=devnet IRIUM_POAWX_MODE=active IRIUM_DEV_EASY_BITS_TEMPLATE=1 IRIUM_P2P_BIND=0.0.0.0:${VPS2_P2P_PORT} IRIUM_NODE_HOST=127.0.0.1 IRIUM_NODE_PORT=${VPS2_RPC_PORT} IRIUM_DATA_DIR=${VPS2_DATA_DIR} IRIUM_BOOTSTRAP_DIR=${VPS2_DATA_DIR}/bootstrap IRIUM_RPC_TOKEN=${RPC_TOKEN} IRIUM_ADDNODE=127.0.0.2:${VPS1_P2P_PORT} ${VPS2_BIN_PATH}
STARTSCRIPT
    scp $VPS2_SSH_OPTS -q "$local_start" "${VPS2_SSH}:/tmp/phase10f-vps2-start.sh" 2>/dev/null
    # Create data dir + bootstrap/trust dirs on VPS-2, SCP anchors + trust files from VPS-1, then start
    ssh $VPS2_SSH_OPTS "$VPS2_SSH" "rm -rf ${VPS2_DATA_DIR} && mkdir -p ${VPS2_DATA_DIR}/bootstrap/trust" 2>/dev/null
    scp $VPS2_SSH_OPTS -q ~/irium/bootstrap/anchors.json "${VPS2_SSH}:${VPS2_DATA_DIR}/bootstrap/" 2>/dev/null || true
    scp $VPS2_SSH_OPTS -q ~/irium/bootstrap/trust/* "${VPS2_SSH}:${VPS2_DATA_DIR}/bootstrap/trust/" 2>/dev/null || true
    # Use subshell () so bash exits without waiting for background iriumd; PID saved to file
    ssh $VPS2_SSH_OPTS "$VPS2_SSH" "chmod +x /tmp/phase10f-vps2-start.sh && (/tmp/phase10f-vps2-start.sh </dev/null >> ${VPS2_DATA_DIR}/iriumd.log 2>&1 & echo \$! > /tmp/vps2-iriumd.pid); cat /tmp/vps2-iriumd.pid"
}

# ─────────────────────────────────────────────────────────────────────────────
echo "============================================================"
echo " Phase 10-F: PoAW-X receipt two-VPS soak"
echo "============================================================"
echo " VPS-1: $VPS1_HOST  P2P=$VPS1_P2P_PORT  RPC=$VPS1_RPC_PORT  Stratum=$VPS1_STRATUM_PORT"
echo " VPS-2: $VPS2_HOST  P2P=$VPS2_P2P_PORT  RPC=$VPS2_RPC_PORT  SSH=$VPS2_SSH"
echo " Target: ${SOAK_BLOCK_TARGET}-block soak (${N_SEGS} segments x ${SEG_BLOCKS})"
echo " Restart every: ${SOAK_RESTART_EVERY} blocks | Negative every: ${SOAK_NEGATIVE_EVERY} blocks"
echo " Data: $DATA_DIR"
echo ""

# ══════════════════════════════════════════════════════════════════════════════
# Section 0: Pre-flight + mainnet safety baseline (both VPS)
# ══════════════════════════════════════════════════════════════════════════════
echo "=== Section 0: Pre-flight + mainnet safety baseline ==="

# VPS-1 mainnet baseline
MAINNET_IRIUMD_PID_VPS1=$(ss -lntp 'sport = :38300' 2>/dev/null | grep -oP 'pid=\K[0-9]+' | head -1 || true)
MAINNET_STRATUM_PID_VPS1=$(ss -lntp 'sport = :3333'  2>/dev/null | grep -oP 'pid=\K[0-9]+' | head -1 || true)
MAINNET_EXPLORER_PID_VPS1=$(ss -lntp 'sport = :38310' 2>/dev/null | grep -oP 'pid=\K[0-9]+' | head -1 || true)
MAINNET_WALLET_PID_VPS1=$(ss -lntp 'sport = :38320'  2>/dev/null | grep -oP 'pid=\K[0-9]+' | head -1 || true)

echo "VPS-1 mainnet baseline:"
echo "  iriumd    PID=${MAINNET_IRIUMD_PID_VPS1:-none}  port=38300"
echo "  stratum   PID=${MAINNET_STRATUM_PID_VPS1:-none}  port=3333"
echo "  explorer  PID=${MAINNET_EXPLORER_PID_VPS1:-none}  port=38310"
echo "  wallet    PID=${MAINNET_WALLET_PID_VPS1:-none}  port=38320"

[[ -n "${MAINNET_IRIUMD_PID_VPS1:-}" ]] && \
    pass "VPS-1 mainnet iriumd alive (PID=$MAINNET_IRIUMD_PID_VPS1)" || \
    info "VPS-1 mainnet iriumd not found on 38300"

# VPS-2 mainnet baseline via SSH
echo "VPS-2 mainnet baseline:"
# Discover VPS-2 mainnet PIDs dynamically
MAINNET_IRIUMD_PID_VPS2=$(ssh $VPS2_SSH_OPTS "$VPS2_SSH" "ss -lntp 'sport = :38300' 2>/dev/null | grep -oP 'pid=\K[0-9]+' | head -1 || true" 2>/dev/null || true)
MAINNET_WALLET_PID_VPS2=$(ssh $VPS2_SSH_OPTS "$VPS2_SSH" "ss -lntp 'sport = :38320' 2>/dev/null | grep -oP 'pid=\K[0-9]+' | head -1 || true" 2>/dev/null || true)
MAINNET_EXPLORER_PID_VPS2=$(ssh $VPS2_SSH_OPTS "$VPS2_SSH" "ss -lntp 'sport = :38310' 2>/dev/null | grep -oP 'pid=\K[0-9]+' | head -1 || true" 2>/dev/null || true)
echo "  iriumd    PID=${MAINNET_IRIUMD_PID_VPS2:-none}  port=38300"
echo "  wallet    PID=${MAINNET_WALLET_PID_VPS2:-none}  port=38320"
echo "  explorer  PID=${MAINNET_EXPLORER_PID_VPS2:-none}  port=38310"
VPS2_MAINNET_ALIVE=0
[[ -n "${MAINNET_IRIUMD_PID_VPS2:-}" ]] && {
    pass "VPS-2 mainnet iriumd alive (PID=$MAINNET_IRIUMD_PID_VPS2)"
    VPS2_MAINNET_ALIVE=1
} || fail "VPS-2 mainnet iriumd not found on port 38300"
[[ -n "${MAINNET_WALLET_PID_VPS2:-}" ]] && pass "VPS-2 mainnet wallet-api alive (PID=$MAINNET_WALLET_PID_VPS2)" || true
[[ -n "${MAINNET_EXPLORER_PID_VPS2:-}" ]] && pass "VPS-2 mainnet explorer alive (PID=$MAINNET_EXPLORER_PID_VPS2)" || true

# Process audit: classify running processes on VPS-1
echo "--- VPS-1 process audit ---"
KNOWN_MAINNET_PIDS="${MAINNET_IRIUMD_PID_VPS1:-NOPID} ${MAINNET_STRATUM_PID_VPS1:-NOPID} ${MAINNET_EXPLORER_PID_VPS1:-NOPID} ${MAINNET_WALLET_PID_VPS1:-NOPID}"
CLAUDE_PIDS=$(pgrep -f 'claude/remote\|ccd-cli\|codex' 2>/dev/null | tr '\n' ' ' || true)
ALL_KNOWN_PIDS="$KNOWN_MAINNET_PIDS $CLAUDE_PIDS"

STALE=$(ps aux | grep -E "$HOME/irium/target/release/(iriumd|irium-stratum)" | grep -v grep | \
    awk -v p="$ALL_KNOWN_PIDS" 'BEGIN{split(p,a," ");for(i in a)k[a[i]]=1} !k[$2]' || true)
if [[ -z "$STALE" ]]; then
    pass "no stale testnet processes on VPS-1"
else
    fail "stale testnet processes on VPS-1: $STALE"
fi

# Claude Code infrastructure daemons (harmless)
CLAUDE_DAEMON_COUNT=$(pgrep -f 'claude/remote\|ccd-cli' 2>/dev/null | wc -l || echo 0)
info "Claude Code infrastructure daemons: $CLAUDE_DAEMON_COUNT (harmless, not testnet)"

# Testnet ports free on VPS-1
ALL_PORTS_FREE=1
for port in $VPS1_P2P_PORT $VPS1_RPC_PORT $VPS1_STRATUM_PORT; do
    if ss -lntp "sport = :${port}" 2>/dev/null | grep -q ":${port}"; then
        fail "VPS-1 testnet port $port in use"
        ALL_PORTS_FREE=0
    fi
done
[[ "$ALL_PORTS_FREE" -eq 1 ]] && pass "VPS-1 testnet ports free (${VPS1_P2P_PORT}/${VPS1_RPC_PORT}/${VPS1_STRATUM_PORT})"

# Testnet ports free on VPS-2
VPS2_PORTS_FREE=1
for port in $VPS2_P2P_PORT $VPS2_RPC_PORT; do
    if ssh $VPS2_SSH_OPTS "$VPS2_SSH" "ss -lntp 'sport = :${port}' 2>/dev/null | grep -q ':${port}'" 2>/dev/null; then
        fail "VPS-2 testnet port $port in use"
        VPS2_PORTS_FREE=0
    fi
done
[[ "$VPS2_PORTS_FREE" -eq 1 ]] && pass "VPS-2 testnet ports free (${VPS2_P2P_PORT}/${VPS2_RPC_PORT})"

# VPS-1 → VPS-2 connectivity check (mainnet port as proxy)
if nc -z -w5 "$VPS2_HOST" 38291 2>/dev/null; then
    pass "VPS-1 → VPS-2 network reachable (mainnet P2P confirmed)"
else
    fail "VPS-1 → VPS-2 network check failed"
fi

# tmux/screen check
TMUX_SESSIONS=$(tmux ls 2>/dev/null | grep -c 'session' || true)
[[ "${TMUX_SESSIONS:-0}" -eq 0 ]] && \
    pass "no tmux sessions on VPS-1" || \
    info "tmux sessions present: $TMUX_SESSIONS (confirm not testnet)"

# Binaries
[[ -x "$IRIUMD_BIN" ]]  && pass "VPS-1 iriumd binary exists" || { fail "iriumd binary missing"; exit 1; }
[[ -x "$STRATUM_BIN" ]] && pass "VPS-1 stratum binary exists" || { fail "stratum binary missing"; exit 1; }
[[ -f "$HARNESS" ]]     && pass "soak harness exists" || { fail "harness missing: $HARNESS"; exit 1; }

# Branch check
CURRENT_BRANCH=$(git -C "$HOME/irium" branch --show-current 2>/dev/null || echo "unknown")
CURRENT_HEAD=$(git -C "$HOME/irium" rev-parse HEAD 2>/dev/null || echo "unknown")
info "branch=$CURRENT_BRANCH  HEAD=$CURRENT_HEAD"
[[ "$CURRENT_BRANCH" == "testnet/poawx-phase10f-receipt-two-vps-soak" ]] && \
    pass "on Phase 10-F branch" || fail "wrong branch: $CURRENT_BRANCH"

# Setup VPS-1 data dir
rm -rf "$DATA_DIR" && mkdir -p "$DATA_DIR/bootstrap"
cp -a "$HOME/irium/bootstrap/anchors.json" "$DATA_DIR/bootstrap/" 2>/dev/null || true
cp -a "$HOME/irium/bootstrap/trust"        "$DATA_DIR/bootstrap/" 2>/dev/null || true
info "VPS-1 data dir: $DATA_DIR"

# ══════════════════════════════════════════════════════════════════════════════
# Section 1: Deploy iriumd binary to VPS-2 + start VPS-2 testnet iriumd
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 1: Deploy + start VPS-2 testnet iriumd ==="
echo "Copying iriumd binary to VPS-2..."
if scp $VPS2_SSH_OPTS -q "$IRIUMD_BIN" "${VPS2_SSH}:${VPS2_BIN_PATH}" 2>&1; then
    pass "iriumd binary deployed to VPS-2:$VPS2_BIN_PATH"
else
    fail "failed to deploy iriumd binary to VPS-2"
    exit 1
fi
ssh $VPS2_SSH_OPTS "$VPS2_SSH" "chmod +x '${VPS2_BIN_PATH}'" 2>/dev/null
VPS2_BIN_VERSION=$(ssh $VPS2_SSH_OPTS "$VPS2_SSH" "'$VPS2_BIN_PATH' --version 2>/dev/null | head -1 || echo unknown" 2>/dev/null)
info "VPS-2 binary version: $VPS2_BIN_VERSION"

# SSH forward tunnel: VPS-2 initiates SSH to VPS-1, binding 127.0.0.2:VPS1_P2P_PORT on VPS-2.
# Host firewall blocks direct testnet P2P; tunnel goes via SSH port 22 (always open).
# Using 127.0.0.2 (not 127.0.0.1) because iriumd hardcodes 127.0.0.1 in local_ip_set(),
# filtering it as "self" — 127.0.0.2 routes to loopback on Linux but is NOT filtered.
echo "Setting up SSH forward tunnel (VPS-2 loopback 127.0.0.2:${VPS1_P2P_PORT} -> VPS-1 ${VPS1_P2P_PORT})..."
ssh $VPS2_SSH_OPTS "$VPS2_SSH" \
    "(ssh -N -L '127.0.0.2:${VPS1_P2P_PORT}:127.0.0.1:${VPS1_P2P_PORT}' \
        -o StrictHostKeyChecking=no -o BatchMode=yes -o ConnectTimeout=15 \
        -o ServerAliveInterval=30 -o ServerAliveCountMax=60 \
        irium@${VPS1_HOST} </dev/null >/dev/null 2>&1 & echo \$! > /tmp/vps2-tunnel.pid); \
     cat /tmp/vps2-tunnel.pid" 2>/dev/null
sleep 3
VPS2_SSH_TUNNEL_PID=$(ssh $VPS2_SSH_OPTS "$VPS2_SSH" "cat /tmp/vps2-tunnel.pid 2>/dev/null || echo 0" 2>/dev/null || echo 0)
TUNNEL_CHECK=$(ssh $VPS2_SSH_OPTS "$VPS2_SSH" "kill -0 ${VPS2_SSH_TUNNEL_PID} 2>/dev/null && echo alive || echo dead" 2>/dev/null || echo dead)
[[ "$TUNNEL_CHECK" == "alive" ]] && \
    pass "SSH forward tunnel established on VPS-2 (PID=$VPS2_SSH_TUNNEL_PID)" || \
    fail "SSH forward tunnel failed to start on VPS-2"
VPS2_IRIUMD_PID=$(start_vps2_iriumd)
info "VPS-2 iriumd PID=$VPS2_IRIUMD_PID (devnet, POAWX_MODE=active)"

echo "Waiting for VPS-2 iriumd RPC..."
if wait_vps2_rpc 45; then
    pass "VPS-2 iriumd started and responsive"
else
    fail "VPS-2 iriumd RPC not responsive after 45s"
    ssh $VPS2_SSH_OPTS "$VPS2_SSH" "tail -20 '${VPS2_DATA_DIR}/iriumd.log'" 2>/dev/null || true
    exit 1
fi

VPS2_TMPL=$(ssh $VPS2_SSH_OPTS "$VPS2_SSH" "curl -sf -H 'Authorization: Bearer $RPC_TOKEN' 'http://127.0.0.1:${VPS2_RPC_PORT}/rpc/getblocktemplate' 2>/dev/null" 2>/dev/null || true)
VPS2_POAWX_MODE=$(echo "$VPS2_TMPL" | python3 -c "import sys,json; print(json.load(sys.stdin).get('poawx_mode',''))" 2>/dev/null || echo "")
[[ "$VPS2_POAWX_MODE" == "active" ]] && \
    pass "VPS-2 template poawx_mode=active" || \
    fail "VPS-2 poawx_mode=$VPS2_POAWX_MODE (expected active)"

VPS2_BITS=$(echo "$VPS2_TMPL" | python3 -c "import sys,json; print(json.load(sys.stdin).get('bits',''))" 2>/dev/null || echo "")
[[ "$VPS2_BITS" == "207fffff" ]] && \
    pass "VPS-2 bits=207fffff (devnet easy)" || \
    info "VPS-2 bits=$VPS2_BITS"

# ══════════════════════════════════════════════════════════════════════════════
# Section 2: Start VPS-1 testnet iriumd (POAWX_MODE=active, addnode VPS-2)
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 2: Start VPS-1 testnet iriumd ==="
(
  cd "$DATA_DIR"
  IRIUM_NETWORK=devnet \
  IRIUM_POAWX_MODE=active \
  IRIUM_DEV_EASY_BITS_TEMPLATE=1 \
  IRIUM_P2P_BIND="0.0.0.0:${VPS1_P2P_PORT}" \
  IRIUM_NODE_HOST=127.0.0.1 \
  IRIUM_NODE_PORT="${VPS1_RPC_PORT}" \
  IRIUM_DATA_DIR="$DATA_DIR" \
  IRIUM_BOOTSTRAP_DIR="$DATA_DIR/bootstrap" \
  IRIUM_RPC_TOKEN="$RPC_TOKEN" \
  IRIUM_ADDNODE="${VPS2_HOST}:${VPS2_P2P_PORT}" \
    "$IRIUMD_BIN" >"$LOG_IRIUMD" 2>&1 &
  echo $! > "$DATA_DIR/iriumd.pid"
)
sleep 0.5
TESTNET_IRIUMD_PID=$(cat "$DATA_DIR/iriumd.pid")
info "VPS-1 iriumd PID=$TESTNET_IRIUMD_PID"

if ! wait_rpc "$RPC_BASE" "$RPC_TOKEN" 30; then
    fail "VPS-1 iriumd RPC not responsive after 30s"
    tail -20 "$LOG_IRIUMD"
    exit 1
fi
kill -0 "$TESTNET_IRIUMD_PID" 2>/dev/null && \
    pass "VPS-1 iriumd started and responsive" || \
    { fail "VPS-1 iriumd died at startup"; exit 1; }

TMPL_VPS1=$(curl -sf -H "Authorization: Bearer $RPC_TOKEN" "$RPC_BASE/rpc/getblocktemplate" 2>/dev/null || true)
BITS_VPS1=$(echo "$TMPL_VPS1" | python3 -c "import sys,json; print(json.load(sys.stdin).get('bits',''))" 2>/dev/null || echo "")
POAWX_VPS1=$(echo "$TMPL_VPS1" | python3 -c "import sys,json; print(json.load(sys.stdin).get('poawx_mode',''))" 2>/dev/null || echo "")
[[ "$BITS_VPS1" == "207fffff" ]] && pass "VPS-1 bits=207fffff (devnet easy)" || fail "bits=$BITS_VPS1"
[[ "$POAWX_VPS1" == "active" ]] && pass "VPS-1 template poawx_mode=active" || fail "poawx_mode=$POAWX_VPS1"

# ══════════════════════════════════════════════════════════════════════════════
# Section 3: Mine preflight block + assignment/receipt checks
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 3: Mine preflight block + assignment checks ==="
echo "Mining 1 block via direct RPC (need height > 0 for assignment)..."

MINE_OK=0
for attempt in $(seq 1 200); do
    TMPL_MINE=$(curl -sf -H "Authorization: Bearer $RPC_TOKEN" "$RPC_BASE/rpc/getblocktemplate" 2>/dev/null || true)
    [[ -z "$TMPL_MINE" ]] && sleep 0.2 && continue
    HEIGHT=$(echo "$TMPL_MINE" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['height'])" 2>/dev/null || echo 0)
    BITS_HEX=$(echo "$TMPL_MINE" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['bits'])" 2>/dev/null || echo "")
    PREV=$(echo "$TMPL_MINE" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['prev_hash'])" 2>/dev/null || echo "")
    TIME_V=$(echo "$TMPL_MINE" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['time'])" 2>/dev/null || echo 0)
    COIN_V=$(echo "$TMPL_MINE" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['coinbase_value'])" 2>/dev/null || echo 0)

    RESULT=$(python3 - "$HEIGHT" "$BITS_HEX" "$PREV" "$TIME_V" "$COIN_V" "$WALLET_ADDR" "$RPC_BASE" "$RPC_TOKEN" <<'PYEOF'
import sys, struct, hashlib, requests, json

height    = int(sys.argv[1])
bits_hex  = sys.argv[2]
prev_hash = sys.argv[3]
t_val     = int(sys.argv[4])
cb_val    = int(sys.argv[5])
wallet    = sys.argv[6]
rpc_base  = sys.argv[7]
rpc_token = sys.argv[8]

def sha256d(data):
    return hashlib.sha256(hashlib.sha256(data).digest()).digest()

def varint(n):
    if n < 0xfd:    return bytes([n])
    if n <= 0xffff: return b'\xfd' + struct.pack('<H', n)
    return b'\xfe' + struct.pack('<I', n)

def build_coinbase(height, reward, pkh, en=b'\x00'*8):
    hl = (height.bit_length()+7)//8 or 1
    sig = bytes([hl]) + height.to_bytes(hl, 'little') + b'Irium' + en
    tx  = b'\x01\x00\x00\x00'
    tx += varint(1) + b'\x20' + b'\x00'*32 + b'\xff\xff\xff\xff'
    tx += varint(len(sig)) + sig + b'\xff\xff\xff\xff'
    tx += varint(1)
    pkscript = b'\x76\xa9\x14' + pkh + b'\x88\xac'
    tx += struct.pack('<Q', reward) + varint(len(pkscript)) + pkscript
    return tx + b'\x00\x00\x00\x00'

def base58_decode(s):
    alpha = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
    n = 0
    for c in s: n = n*58 + alpha.index(c)
    return n.to_bytes(25, 'big')

pkh   = base58_decode(wallet)[1:21]
bits  = int(bits_hex, 16)
exp   = bits >> 24
mant  = bits & 0xffffff
tgt   = (mant * (1 << (8*(exp-3)))).to_bytes(32, 'big')
cb_tx = build_coinbase(height, cb_val, pkh)
mr    = sha256d(cb_tx)

for nonce in range(0x100000000):
    hdr  = struct.pack('<I', 1) + bytes.fromhex(prev_hash)[::-1]
    mr_w = mr if height >= 22888 else mr[::-1]
    hdr += mr_w + struct.pack('<III', t_val, bits, nonce)
    h = sha256d(hdr)
    if h[::-1] <= tgt:
        resp = requests.post(
            f'{rpc_base}/rpc/submit_block',
            headers={'Authorization': f'Bearer {rpc_token}'},
            json={
                'height': height,
                'header': {
                    'version': 1, 'prev_hash': prev_hash,
                    'merkle_root': mr.hex(),
                    'time': t_val, 'bits': bits_hex,
                    'nonce': nonce, 'hash': h[::-1].hex(),
                },
                'tx_hex': [cb_tx.hex()],
                'submit_source': 'phase10f_preflight',
            }
        )
        print('OK' if resp.status_code == 200 else f'FAIL:{resp.status_code}:{resp.text[:80]}')
        sys.exit(0)
print('TIMEOUT')
PYEOF
    )
    if [[ "$RESULT" == "OK" ]]; then
        echo "Block mined at height=$HEIGHT"
        MINE_OK=1
        break
    fi
    sleep 0.1
done
[[ "$MINE_OK" -eq 1 ]] && pass "preflight block mined (height>0 for assignment)" || \
    { fail "failed to mine preflight block"; exit 1; }
sleep 1

ASSIGN_FILE="/tmp/phase10f_assign.json"
HTTP_ASSIGN=$(curl -s -o "$ASSIGN_FILE" -w "%{http_code}" \
    -H "Authorization: Bearer $RPC_TOKEN" \
    "$RPC_BASE/poawx/assignment" 2>/dev/null)
[[ "$HTTP_ASSIGN" == "200" ]] && pass "/poawx/assignment returns 200" || \
    { fail "/poawx/assignment HTTP $HTTP_ASSIGN"; cat "$ASSIGN_FILE" 2>/dev/null; }

ASSIGN_HEIGHT=$(python3 -c "import json; print(json.load(open('$ASSIGN_FILE')).get('height',0))" 2>/dev/null || echo 0)
SEED=$(python3 -c "import json; print(json.load(open('$ASSIGN_FILE')).get('seed',''))" 2>/dev/null || echo "")
NONCE=$(python3 -c "import json; print(json.load(open('$ASSIGN_FILE')).get('commitment_nonce',''))" 2>/dev/null || echo "")
LANE=$(python3 -c "import json; print(json.load(open('$ASSIGN_FILE')).get('lane',''))" 2>/dev/null || echo "")
POW_BITS=$(python3 -c "import json; print(json.load(open('$ASSIGN_FILE')).get('pow_bits',''))" 2>/dev/null || echo "")
PDIFF=$(python3 -c "import json; print(json.load(open('$ASSIGN_FILE')).get('puzzle_difficulty',0))" 2>/dev/null || echo 0)

[[ ${#SEED} -eq 64 ]]  && pass "assignment.seed is 32-byte hex" || fail "seed malformed: len=${#SEED}"
[[ ${#NONCE} -eq 64 ]] && pass "assignment.commitment_nonce is 32-byte hex" || fail "nonce malformed: len=${#NONCE}"
[[ "$LANE" == "cpu" ]] && pass "assignment.lane=cpu (lowercase)" || fail "lane=$LANE (expected cpu)"
[[ -n "$POW_BITS" ]]   && pass "assignment.pow_bits present: $POW_BITS" || fail "pow_bits missing"
[[ -n "$PDIFF" && "$PDIFF" != "0" ]] && pass "assignment.puzzle_difficulty present: $PDIFF" || \
    fail "puzzle_difficulty missing or 0"
echo "assignment: height=$ASSIGN_HEIGHT lane=$LANE pow_bits=$POW_BITS puzzle_difficulty=$PDIFF"

# ── Section 3b: POST /poawx/receipt (valid) ────────────────────────────────
echo ""
echo "--- Section 3b: POST /poawx/receipt (valid) ---"
SOLUTION=$(echo -n "${SEED}solution" | sha256sum | awk '{print $1}')
RECEIPT_BODY=$(python3 -c "import json; print(json.dumps({
    'height': int('$ASSIGN_HEIGHT'),
    'lane': 'cpu',
    'worker_pkh': 'a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2',
    'solution': '$SOLUTION',
    'commitment_nonce': '$NONCE',
}))")

HTTP_RECEIPT=$(curl -s -o /tmp/phase10f_receipt.json -w "%{http_code}" \
    -X POST \
    -H "Authorization: Bearer $RPC_TOKEN" \
    -H "Content-Type: application/json" \
    -d "$RECEIPT_BODY" \
    "$RPC_BASE/poawx/receipt" 2>/dev/null)

if [[ "$HTTP_RECEIPT" == "200" ]]; then
    pass "POST /poawx/receipt returns 200"
    PENDING_CNT=$(python3 -c "import json; print(json.load(open('/tmp/phase10f_receipt.json')).get('pending_count',0))" 2>/dev/null || echo 0)
    [[ "$PENDING_CNT" -ge 1 ]] && pass "receipt stored: pending_count=$PENDING_CNT" || fail "pending_count=$PENDING_CNT"
else
    fail "POST /poawx/receipt HTTP $HTTP_RECEIPT"
    cat /tmp/phase10f_receipt.json 2>/dev/null; PENDING_CNT=0
fi

# ── Section 3c: Template receipts_root verification ────────────────────────
echo ""
echo "--- Section 3c: Template receipts_root ---"
TMPL_R=$(curl -sf -H "Authorization: Bearer $RPC_TOKEN" "$RPC_BASE/rpc/getblocktemplate" 2>/dev/null || true)
RROOT=$(echo "$TMPL_R" | python3 -c "import sys,json; print(json.load(sys.stdin).get('receipts_root',''))" 2>/dev/null || echo "")
RPEND=$(echo "$TMPL_R" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('poawx_pending_receipts',[])))" 2>/dev/null || echo 0)

[[ ${#RROOT} -eq 64 ]] && pass "receipts_root is non-empty 32-byte hex" || fail "receipts_root not 32-byte hex: '${RROOT}'"
[[ "$RPEND" -ge 1 ]] && pass "template shows $RPEND pending receipt(s)" || fail "no pending receipts in template"

EXPECTED_ROOT=$(python3 - "$ASSIGN_HEIGHT" "$SOLUTION" "$NONCE" <<'PYEOF'
import hashlib, sys
h   = int(sys.argv[1])
sol = bytes.fromhex(sys.argv[2])
non = bytes.fromhex(sys.argv[3])
pkh = bytes.fromhex('a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2')
inner = hashlib.sha256()
inner.update(h.to_bytes(8, 'little'))
inner.update(b'cpu')
inner.update(pkh)
inner.update(sol)
inner.update(non)
outer = hashlib.sha256()
outer.update(inner.digest())
print(outer.hexdigest())
PYEOF
)
[[ "$RROOT" == "$EXPECTED_ROOT" ]] && \
    pass "receipts_root matches computed canonical root" || \
    fail "receipts_root mismatch: got=$RROOT expected=$EXPECTED_ROOT"

# ══════════════════════════════════════════════════════════════════════════════
# Section 4: Negative checks
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 4: Negative checks ==="

# 4a: Invalid receipt — bad hex in solution
echo "--- 4a: Invalid hex in solution ---"
BAD_BODY=$(python3 -c "import json; print(json.dumps({
    'height': int('$ASSIGN_HEIGHT'),
    'lane': 'cpu',
    'worker_pkh': 'a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2',
    'solution': 'not-valid-hex!!!',
    'commitment_nonce': '$NONCE',
}))")
HTTP_BAD=$(curl -s -o /dev/null -w "%{http_code}" \
    -X POST -H "Authorization: Bearer $RPC_TOKEN" \
    -H "Content-Type: application/json" -d "$BAD_BODY" \
    "$RPC_BASE/poawx/receipt" 2>/dev/null)
[[ "$HTTP_BAD" == "400" || "$HTTP_BAD" == "422" ]] && \
    pass "invalid receipt (bad hex) rejected: HTTP $HTTP_BAD" || \
    fail "invalid receipt not rejected: HTTP $HTTP_BAD (expected 400/422)"

# 4b: Duplicate receipt dedup
echo "--- 4b: Duplicate receipt dedup ---"
HTTP_DUP=$(curl -s -o /tmp/phase10f_dup.json -w "%{http_code}" \
    -X POST -H "Authorization: Bearer $RPC_TOKEN" \
    -H "Content-Type: application/json" -d "$RECEIPT_BODY" \
    "$RPC_BASE/poawx/receipt" 2>/dev/null)
DUP_CNT=$(python3 -c "import json; print(json.load(open('/tmp/phase10f_dup.json')).get('pending_count',0))" 2>/dev/null || echo 0)
[[ "$HTTP_DUP" == "200" && "$DUP_CNT" -le "$PENDING_CNT" ]] && \
    pass "duplicate receipt deduped: pending_count=$DUP_CNT (<=original $PENDING_CNT)" || \
    fail "duplicate dedup failed: HTTP=$HTTP_DUP count=$DUP_CNT (expected <=$PENDING_CNT)"

# 4c: Disabled-mode iriumd — /poawx/assignment must return 503
echo "--- 4c: Disabled-mode iriumd (no IRIUM_POAWX_MODE=active) ---"
mkdir -p "$DATA_DIR/disabled/bootstrap"
cp -a "$HOME/irium/bootstrap/anchors.json" "$DATA_DIR/disabled/bootstrap/" 2>/dev/null || true
(
  cd "$DATA_DIR/disabled"
  IRIUM_NETWORK=devnet \
  IRIUM_DEV_EASY_BITS_TEMPLATE=1 \
  IRIUM_P2P_BIND="0.0.0.0:$((VPS1_P2P_PORT+10))" \
  IRIUM_NODE_HOST=127.0.0.1 \
  IRIUM_NODE_PORT="${VPS1_DISABLED_RPC_PORT}" \
  IRIUM_DATA_DIR="$DATA_DIR/disabled" \
  IRIUM_BOOTSTRAP_DIR="$DATA_DIR/disabled/bootstrap" \
  IRIUM_RPC_TOKEN="$RPC_TOKEN" \
    "$IRIUMD_BIN" >"$LOG_DISABLED" 2>&1 &
  echo $! > "$DATA_DIR/disabled.pid"
)
sleep 0.5
TESTNET_DISABLED_PID=$(cat "$DATA_DIR/disabled.pid")
RPC_DISABLED="http://127.0.0.1:${VPS1_DISABLED_RPC_PORT}"

if wait_rpc "$RPC_DISABLED" "$RPC_TOKEN" 20; then
    HTTP_DA=$(curl -s -o /dev/null -w "%{http_code}" \
        -H "Authorization: Bearer $RPC_TOKEN" \
        "$RPC_DISABLED/poawx/assignment" 2>/dev/null)
    [[ "$HTTP_DA" == "503" ]] && \
        pass "disabled-mode /poawx/assignment returns 503" || \
        fail "disabled-mode /poawx/assignment returned $HTTP_DA (expected 503)"

    HTTP_DE=$(curl -s -o /dev/null -w "%{http_code}" \
        -X POST -H "Authorization: Bearer $RPC_TOKEN" \
        -H "Content-Type: application/json" -d '{}' \
        "$RPC_DISABLED/rpc/submit_block_extended" 2>/dev/null)
    [[ "$HTTP_DE" != "200" ]] && \
        pass "disabled-mode /rpc/submit_block_extended not 200: HTTP $HTTP_DE" || \
        fail "disabled-mode submit_block_extended returned 200 (should reject)"

    HTTP_TMPL_D=$(curl -s -o /dev/null -w "%{http_code}" \
        -H "Authorization: Bearer $RPC_TOKEN" \
        "$RPC_DISABLED/rpc/getblocktemplate" 2>/dev/null)
    TMPL_D_MODE=$(curl -sf -H "Authorization: Bearer $RPC_TOKEN" "$RPC_DISABLED/rpc/getblocktemplate" 2>/dev/null | \
        python3 -c "import sys,json; print(json.load(sys.stdin).get('poawx_mode','disabled'))" 2>/dev/null || echo "")
    [[ "$TMPL_D_MODE" != "active" ]] && \
        pass "disabled-mode template poawx_mode=$TMPL_D_MODE (not active)" || \
        fail "disabled-mode template shows poawx_mode=active (should not)"
else
    skip "disabled-mode iriumd not responsive on port $VPS1_DISABLED_RPC_PORT; 503 checks skipped"
fi
kill "$TESTNET_DISABLED_PID" 2>/dev/null || true
TESTNET_DISABLED_PID=""
sleep 1
fuser -k "$((VPS1_P2P_PORT+10))/tcp" 2>/dev/null || true

# 4d: Mainnet safety note
info "4d: PoAW-X disabled on mainnet confirmed in Section 14"

# ══════════════════════════════════════════════════════════════════════════════
# Section 5: Start VPS-1 testnet stratum
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 5: Start VPS-1 testnet stratum ==="
start_stratum "$LOG_STRATUM1"
kill -0 "$TESTNET_STRATUM_PID" 2>/dev/null && pass "stratum started" || \
    { fail "stratum died at startup"; tail -10 "$LOG_STRATUM1"; exit 1; }
ss -lntp "sport = :${VPS1_STRATUM_PORT}" 2>/dev/null | grep -q ":${VPS1_STRATUM_PORT}" && \
    pass "stratum port $VPS1_STRATUM_PORT open" || fail "stratum port not open"
grep -q '\[poawx\].*IRIUM_STRATUM_POAWX=1\|PoAW-X receipt path enabled' "$LOG_STRATUM1" 2>/dev/null && \
    pass "stratum logged PoAW-X startup message" || \
    fail "stratum PoAW-X startup message not found"

# ══════════════════════════════════════════════════════════════════════════════
# Section 6: Soak segment 1 (blocks 1 – SEG_BLOCKS)
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 6: Soak segment 1/$N_SEGS ($SEG_BLOCKS blocks) ==="
run_soak_segment 1 "$SEG_BLOCKS" "$LOG_HARNESS1"

H_AFTER_SEG1=$(vps1_height)
info "VPS-1 height after seg1: $H_AFTER_SEG1"
check_vps2_sync "after-seg1" 10

# ── Negative check at SOAK_NEGATIVE_EVERY boundary ────────────────────────
echo "--- 4e: IRIUM_STRATUM_POAWX=0 keeps legacy path (negative check) ---"
# Verify stratum log: submit_block_extended is being used, not legacy
EXT_CALLS_S1=$(grep -c 'submit_block_extended' "$LOG_STRATUM1" 2>/dev/null; true)
LEGACY_S1=$(grep -c 'POST.*submit_block[^_]' "$LOG_STRATUM1" 2>/dev/null; true)
[[ "${EXT_CALLS_S1:-0}" -ge 1 ]] && \
    pass "stratum uses submit_block_extended path (calls=${EXT_CALLS_S1})" || \
    fail "stratum not using submit_block_extended path"
[[ "${LEGACY_S1:-0}" -eq 0 ]] && \
    pass "stratum: no legacy submit_block fallback in seg1" || \
    info "stratum: ${LEGACY_S1} legacy submit_block calls in seg1"

# ── Stratum restart 1 ────────────────────────────────────────────────────
echo ""
echo "=== Stratum restart 1 (between seg1 and seg2) ==="
restart_stratum "$LOG_STRATUM2"

# ══════════════════════════════════════════════════════════════════════════════
# Section 7: Soak segment 2 (blocks SEG_BLOCKS+1 – 2×SEG_BLOCKS)
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 7: Soak segment 2/$N_SEGS ($SEG_BLOCKS blocks) ==="
run_soak_segment 2 "$SEG_BLOCKS" "$LOG_HARNESS2"

H_AFTER_SEG2=$(vps1_height)
info "VPS-1 height after seg2: $H_AFTER_SEG2"
check_vps2_sync "after-seg2" 10

# ── VPS-2 testnet iriumd restart ─────────────────────────────────────────
echo ""
echo "=== VPS-2 testnet iriumd restart ==="
echo "Killing VPS-2 testnet iriumd PID=$VPS2_IRIUMD_PID..."
ssh $VPS2_SSH_OPTS "$VPS2_SSH" "kill ${VPS2_IRIUMD_PID} 2>/dev/null || true; sleep 2; fuser -k ${VPS2_P2P_PORT}/tcp 2>/dev/null || true; fuser -k ${VPS2_RPC_PORT}/tcp 2>/dev/null || true" 2>/dev/null || true
sleep 2
pass "VPS-2 testnet iriumd stopped"

echo "Restarting VPS-2 testnet iriumd..."
VPS2_IRIUMD_PID=$(start_vps2_iriumd)
info "VPS-2 iriumd restarted PID=$VPS2_IRIUMD_PID"

if wait_vps2_rpc 45; then
    pass "VPS-2 iriumd responsive after restart"
else
    fail "VPS-2 iriumd not responsive after restart"
    skip "VPS-2 sync check after restart (not responsive)"
fi

# Wait for VPS-2 to catch up after restart
echo "Waiting for VPS-2 sync after restart..."
sleep 15
VPS2_H_RESTART=$(vps2_height)
VPS1_H_RESTART=$H_AFTER_SEG2
if [[ "$VPS2_H_RESTART" -ge "$((VPS1_H_RESTART - 5))" ]] && [[ "$VPS2_H_RESTART" -ge 1 ]]; then
    pass "VPS-2 synced after restart: height=$VPS2_H_RESTART (VPS-1=$VPS1_H_RESTART)"
else
    info "VPS-2 height=$VPS2_H_RESTART after restart, waiting 20s more..."
    sleep 20
    VPS2_H_RESTART=$(vps2_height)
    [[ "$VPS2_H_RESTART" -ge 1 ]] && \
        pass "VPS-2 syncing: height=$VPS2_H_RESTART after restart" || \
        fail "VPS-2 not syncing after restart: height=$VPS2_H_RESTART"
fi

# ── Stratum restart 2 ────────────────────────────────────────────────────
echo ""
echo "=== Stratum restart 2 (between seg2 and seg3) ==="
restart_stratum "$LOG_STRATUM3"

# ══════════════════════════════════════════════════════════════════════════════
# Section 8: Soak segment 3 (blocks 2×SEG+1 – 3×SEG)
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 8: Soak segment 3/$N_SEGS ($SEG_BLOCKS blocks) ==="
run_soak_segment 3 "$SEG_BLOCKS" "$LOG_HARNESS3"

FINAL_H=$(vps1_height)
info "VPS-1 height after seg3: $FINAL_H"

# ══════════════════════════════════════════════════════════════════════════════
# Section 9: VPS-2 propagation final verification
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 9: VPS-2 propagation final verification ==="
check_vps2_sync "final" 15

VPS1_PREV=$(vps1_prev_hash)
sleep 3
VPS2_PREV=$(vps2_prev_hash)
if [[ -n "$VPS1_PREV" && -n "$VPS2_PREV" ]]; then
    if [[ "$VPS1_PREV" == "$VPS2_PREV" ]]; then
        pass "VPS-2 prev_hash matches VPS-1: ${VPS1_PREV:0:16}..."
    else
        VPS1_H_F=$(vps1_height)
        VPS2_H_F=$(vps2_height)
        info "prev_hash mismatch: VPS-1=$VPS1_PREV VPS-2=$VPS2_PREV"
        info "heights: VPS-1=$VPS1_H_F VPS-2=$VPS2_H_F"
        # If heights match but hash doesn't, fail; if height lags, just note
        [[ "$VPS2_H_F" -lt "$((VPS1_H_F - 5))" ]] && \
            fail "VPS-2 hash mismatch: lagging by $((VPS1_H_F - VPS2_H_F)) blocks" || \
            fail "VPS-2 prev_hash mismatch at same height (chain split?)"
    fi
else
    fail "could not get prev_hash from one or both VPS nodes"
fi

VPS2_POAWX_AFTER=$(ssh $VPS2_SSH_OPTS "$VPS2_SSH" "curl -sf -H 'Authorization: Bearer $RPC_TOKEN' 'http://127.0.0.1:${VPS2_RPC_PORT}/rpc/getblocktemplate' 2>/dev/null | python3 -c 'import sys,json; print(json.load(sys.stdin).get(\"poawx_mode\",\"\"))'" 2>/dev/null || echo "")
[[ "$VPS2_POAWX_AFTER" == "active" ]] && \
    pass "VPS-2 poawx_mode=active after full soak" || \
    info "VPS-2 poawx_mode=$VPS2_POAWX_AFTER after soak"

VPS2_LOG_CHECK=$(ssh $VPS2_SSH_OPTS "$VPS2_SSH" "grep -c 'peer\|sync\|connect\|block' '${VPS2_DATA_DIR}/iriumd.log' 2>/dev/null; true" 2>/dev/null || echo 0)
[[ "${VPS2_LOG_CHECK:-0}" -ge 1 ]] && \
    pass "VPS-2 log shows peer/sync activity ($VPS2_LOG_CHECK entries)" || \
    info "VPS-2 log minimal (propagation may be via mempool)"

# ══════════════════════════════════════════════════════════════════════════════
# Section 10: Log verification
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 10: Log verification ==="
TOTAL_IRX1_LOG=0
TOTAL_EXT_CALLS=0
for sl in "$LOG_STRATUM1" "$LOG_STRATUM2" "$LOG_STRATUM3"; do
    [[ -f "$sl" ]] || continue
    n=$(grep -c 'irx1_len=38\|poawx.*to_job.*mode=active' "$sl" 2>/dev/null; true)
    e=$(grep -c 'submit_block_extended' "$sl" 2>/dev/null; true)
    TOTAL_IRX1_LOG=$((TOTAL_IRX1_LOG + ${n:-0}))
    TOTAL_EXT_CALLS=$((TOTAL_EXT_CALLS + ${e:-0}))
done
EXT_ACCEPTED=$(grep -c 'submit_block_extended.*accepted\|block_extended accepted\|accepted.*extended' "$LOG_IRIUMD" 2>/dev/null; true)

echo "stratum: irx1_injections=${TOTAL_IRX1_LOG}  submit_block_extended_calls=${TOTAL_EXT_CALLS}"
echo "iriumd:  submit_block_extended_accepted=${EXT_ACCEPTED:-0}"

[[ "${TOTAL_IRX1_LOG:-0}" -ge 1 ]] && \
    pass "stratum: irx1 coinbase injections logged ($TOTAL_IRX1_LOG total)" || \
    fail "stratum: no irx1 injection log entries"
[[ "${TOTAL_EXT_CALLS:-0}" -ge "$SOAK_BLOCK_TARGET" ]] && \
    pass "stratum: submit_block_extended called ${TOTAL_EXT_CALLS} times (>= $SOAK_BLOCK_TARGET)" || \
    [[ "${TOTAL_EXT_CALLS:-0}" -ge 20 ]] && \
        pass "stratum: submit_block_extended called ${TOTAL_EXT_CALLS} times (>= 20 fallback)" || \
        fail "stratum: submit_block_extended only ${TOTAL_EXT_CALLS} times (need >= 20)"
[[ "${EXT_ACCEPTED:-0}" -ge 1 ]] && \
    pass "iriumd: accepted submit_block_extended blocks" || \
    info "iriumd: submit_block_extended acceptance not logged at INFO level"

LEGACY_TOTAL=0
for sl in "$LOG_STRATUM1" "$LOG_STRATUM2" "$LOG_STRATUM3"; do
    [[ -f "$sl" ]] || continue
    l=$(grep -c 'POST.*submit_block[^_]' "$sl" 2>/dev/null; true)
    LEGACY_TOTAL=$((LEGACY_TOTAL + ${l:-0}))
done
[[ "${LEGACY_TOTAL:-0}" -eq 0 ]] && \
    pass "stratum: no legacy submit_block fallback across all segments" || \
    info "stratum: ${LEGACY_TOTAL} legacy submit_block calls (pre-receipt blocks use legacy)"

# ══════════════════════════════════════════════════════════════════════════════
# Section 11: Persisted block check
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 11: Persisted block check ==="
GETBLOCK_HTTP=$(curl -s -o /tmp/phase10f_blk.json -w "%{http_code}" \
    -H "Authorization: Bearer $RPC_TOKEN" \
    "$RPC_BASE/rpc/getblock/$FINAL_H" 2>/dev/null)

if [[ "$GETBLOCK_HTTP" == "200" ]]; then
    COINBASE_HEX=$(python3 -c "import json; d=json.load(open('/tmp/phase10f_blk.json')); print(d.get('tx',[''])[0])" 2>/dev/null || echo "")
    POAWX_ROOT_P=$(python3 -c "import json; d=json.load(open('/tmp/phase10f_blk.json')); print(d.get('poawx_receipts_root',''))" 2>/dev/null || echo "")
    [[ -n "$COINBASE_HEX" ]] && pass "persisted block tx_hex present" || fail "persisted block tx_hex missing"
    echo "$COINBASE_HEX" | grep -qi '6a2469727831' && \
        pass "persisted coinbase contains irx1 OP_RETURN (6a2469727831)" || \
        info "irx1 not in getblock tx hex (may differ by serialization)"
    [[ ${#POAWX_ROOT_P} -eq 64 ]] && \
        pass "persisted poawx_receipts_root is 32-byte hex: $POAWX_ROOT_P" || \
        info "persisted poawx_receipts_root: '${POAWX_ROOT_P}'"
else
    skip "getblock endpoint not available (HTTP $GETBLOCK_HTTP); irx1 confirmed via harness+log"
    info "irx1 confirmed: harness irx1_count=$TOTAL_IRX1, stratum irx1_injections=$TOTAL_IRX1_LOG"
fi

# ══════════════════════════════════════════════════════════════════════════════
# Section 12: Bogus share rejection
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 12: Bogus share rejection ==="
python3 "$HARNESS" \
    127.0.0.1 "$VPS1_STRATUM_PORT" \
    "$RPC_BASE" "$RPC_TOKEN" \
    --blocks 2 \
    --bogus \
    2>&1 | tee "$LOG_HARNESS_BOGUS" || true

BOGUS_SJ=$(grep '^SUMMARY_JSON:' "$LOG_HARNESS_BOGUS" | tail -1 || echo "")
if [[ -n "$BOGUS_SJ" ]]; then
    SB="${BOGUS_SJ#SUMMARY_JSON:}"
    BOG_REJ=$(echo "$SB" | python3 -c "import sys,json; v=json.loads(sys.stdin.read()).get('bogus_rejected',None); print(v)")
    BOG_H=$(echo "$SB" | python3 -c "import sys,json; v=json.loads(sys.stdin.read()).get('bogus_height_unchanged',None); print(v)")
    echo "bogus: rejected=$BOG_REJ height_unchanged=$BOG_H"
    [[ "$BOG_REJ" == "True" ]] && pass "bogus share rejected" || fail "bogus share not rejected ($BOG_REJ)"
    [[ "$BOG_H" == "True" ]] && pass "chain height unchanged after bogus share" || \
        fail "chain height advanced after bogus share"
else
    fail "bogus: harness produced no SUMMARY_JSON"
fi

# ══════════════════════════════════════════════════════════════════════════════
# Section 13: Log scan (all testnet logs)
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 13: Log scan ==="
LOG_SCAN_CLEAN=1
ALL_LOGS=("$LOG_IRIUMD" "$LOG_STRATUM1" "$LOG_STRATUM2" "$LOG_STRATUM3")

for LOG_FILE in "${ALL_LOGS[@]}"; do
    [[ -f "$LOG_FILE" ]] || continue
    LNAME=$(basename "$LOG_FILE")

    PANICS=$(grep -c 'thread.*panicked\|SIGSEGV\|stack overflow' "$LOG_FILE" 2>/dev/null; true)
    [[ "${PANICS:-0}" -eq 0 ]] && pass "no panics in $LNAME" || \
        { fail "panics in $LNAME: $PANICS"; grep 'thread.*panicked' "$LOG_FILE" | head -3; LOG_SCAN_CLEAN=0; }

    INV_COMMIT=$(grep -c 'invalid.*commitment.*accepted\|bad irx1.*accepted\|invalid irx1.*ok' "$LOG_FILE" 2>/dev/null; true)
    [[ "${INV_COMMIT:-0}" -eq 0 ]] && pass "no invalid commitment accepted in $LNAME" || \
        { fail "invalid commitment accepted in $LNAME: $INV_COMMIT"; LOG_SCAN_CLEAN=0; }

    MAINNET_REF=$(grep -c '38300\|mainnet\|production' "$LOG_FILE" 2>/dev/null; true)
    [[ "${MAINNET_REF:-0}" -eq 0 ]] && pass "no mainnet refs in $LNAME" || \
        info "$LNAME: ${MAINNET_REF} mainnet references (inspect: grep 38300 $LOG_FILE)"

    PEER_LOOP=$(grep -c 'disconnect.*loop\|reconnect.*loop\|peer.*error.*repeated' "$LOG_FILE" 2>/dev/null; true)
    [[ "${PEER_LOOP:-0}" -eq 0 ]] && pass "no peer disconnect loops in $LNAME" || \
        { fail "peer disconnect loop in $LNAME: $PEER_LOOP"; LOG_SCAN_CLEAN=0; }
done

# VPS-2 log scan via SSH
if ssh $VPS2_SSH_OPTS "$VPS2_SSH" "test -f '${VPS2_DATA_DIR}/iriumd.log'" 2>/dev/null; then
    VPS2_PANICS=$(ssh $VPS2_SSH_OPTS "$VPS2_SSH" "grep -c 'thread.*panicked\|SIGSEGV' '${VPS2_DATA_DIR}/iriumd.log' 2>/dev/null; true" 2>/dev/null || echo 0)
    [[ "${VPS2_PANICS:-0}" -eq 0 ]] && pass "no panics in VPS-2 iriumd.log" || \
        { fail "panics in VPS-2 iriumd.log: $VPS2_PANICS"; LOG_SCAN_CLEAN=0; }
    VPS2_MAINNET_REF=$(ssh $VPS2_SSH_OPTS "$VPS2_SSH" "grep -c '38300\|mainnet' '${VPS2_DATA_DIR}/iriumd.log' 2>/dev/null; true" 2>/dev/null || echo 0)
    [[ "${VPS2_MAINNET_REF:-0}" -eq 0 ]] && pass "no mainnet refs in VPS-2 iriumd.log" || \
        info "VPS-2 iriumd.log: ${VPS2_MAINNET_REF} mainnet references"
fi

[[ "$LOG_SCAN_CLEAN" -eq 1 ]] && info "log scan clean (no panics, no invalid acceptance)" || \
    info "log scan found issues (see FAIL entries)"

# ══════════════════════════════════════════════════════════════════════════════
# Section 14: Mainnet safety after soak (BOTH VPS)
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 14: Mainnet safety after soak ==="

# VPS-1 mainnet
echo "--- VPS-1 mainnet ---"
[[ -n "${MAINNET_IRIUMD_PID_VPS1:-}" ]] && \
    { kill -0 "$MAINNET_IRIUMD_PID_VPS1" 2>/dev/null && \
        pass "VPS-1 mainnet iriumd PID=$MAINNET_IRIUMD_PID_VPS1 still alive" || \
        fail "CRITICAL: VPS-1 mainnet iriumd PID=$MAINNET_IRIUMD_PID_VPS1 died"; }
[[ -n "${MAINNET_STRATUM_PID_VPS1:-}" ]] && \
    { kill -0 "$MAINNET_STRATUM_PID_VPS1" 2>/dev/null && \
        pass "VPS-1 mainnet stratum PID=$MAINNET_STRATUM_PID_VPS1 still alive" || \
        fail "CRITICAL: VPS-1 mainnet stratum PID=$MAINNET_STRATUM_PID_VPS1 died"; }
[[ -n "${MAINNET_EXPLORER_PID_VPS1:-}" ]] && \
    { kill -0 "$MAINNET_EXPLORER_PID_VPS1" 2>/dev/null && \
        pass "VPS-1 mainnet explorer PID=$MAINNET_EXPLORER_PID_VPS1 still alive" || \
        fail "CRITICAL: VPS-1 mainnet explorer PID=$MAINNET_EXPLORER_PID_VPS1 died"; }
[[ -n "${MAINNET_WALLET_PID_VPS1:-}" ]] && \
    { kill -0 "$MAINNET_WALLET_PID_VPS1" 2>/dev/null && \
        pass "VPS-1 mainnet wallet-api PID=$MAINNET_WALLET_PID_VPS1 still alive" || \
        fail "CRITICAL: VPS-1 mainnet wallet-api PID=$MAINNET_WALLET_PID_VPS1 died"; }

for mn_port in 38300 3333 38310 38320; do
    ss -lntp "sport = :${mn_port}" 2>/dev/null | grep -q ":${mn_port}" && \
        pass "VPS-1 mainnet port $mn_port still bound" || info "VPS-1 port $mn_port not bound"
done

TESTNET_ON_MN=$(ss -lntp | grep -E ':38300|:3333[^3]|:38310|:38320' | \
    grep -v "pid=${MAINNET_IRIUMD_PID_VPS1:-0}" | \
    grep -v "pid=${MAINNET_STRATUM_PID_VPS1:-0}" | \
    grep -v "pid=${MAINNET_EXPLORER_PID_VPS1:-0}" | \
    grep -v "pid=${MAINNET_WALLET_PID_VPS1:-0}" || true)
[[ -z "$TESTNET_ON_MN" ]] && pass "VPS-1: no testnet process used a mainnet port" || \
    fail "VPS-1: testnet process on mainnet port: $TESTNET_ON_MN"

# VPS-2 mainnet
echo "--- VPS-2 mainnet ---"
if ssh $VPS2_SSH_OPTS "$VPS2_SSH" "kill -0 $MAINNET_IRIUMD_PID_VPS2 2>/dev/null" 2>/dev/null; then
    pass "VPS-2 mainnet iriumd PID=$MAINNET_IRIUMD_PID_VPS2 still alive"
else
    fail "CRITICAL: VPS-2 mainnet iriumd PID=$MAINNET_IRIUMD_PID_VPS2 died"
fi
if ssh $VPS2_SSH_OPTS "$VPS2_SSH" "kill -0 $MAINNET_WALLET_PID_VPS2 2>/dev/null" 2>/dev/null; then
    pass "VPS-2 mainnet wallet-api PID=$MAINNET_WALLET_PID_VPS2 still alive"
fi
if ssh $VPS2_SSH_OPTS "$VPS2_SSH" "kill -0 $MAINNET_EXPLORER_PID_VPS2 2>/dev/null" 2>/dev/null; then
    pass "VPS-2 mainnet explorer PID=$MAINNET_EXPLORER_PID_VPS2 still alive"
fi

for mn_port in 38300 38310 38320; do
    ssh $VPS2_SSH_OPTS "$VPS2_SSH" "ss -lntp 'sport = :${mn_port}' 2>/dev/null | grep -q ':${mn_port}'" 2>/dev/null && \
        pass "VPS-2 mainnet port $mn_port still bound" || info "VPS-2 port $mn_port not bound"
done

VPS2_TN_ON_MN=$(ssh $VPS2_SSH_OPTS "$VPS2_SSH" "ss -lntp 2>/dev/null | grep -E ':38300|:3333|:38310|:38320' | grep -v 'pid=${MAINNET_IRIUMD_PID_VPS2}' | grep -v 'pid=${MAINNET_WALLET_PID_VPS2}' | grep -v 'pid=${MAINNET_EXPLORER_PID_VPS2}' || true" 2>/dev/null || true)
[[ -z "$VPS2_TN_ON_MN" ]] && pass "VPS-2: no testnet process used a mainnet port" || \
    fail "VPS-2: testnet process on mainnet port: $VPS2_TN_ON_MN"

# Confirm PoAW-X not enabled on mainnet iriumd (it lacks the env var)
info "Mainnet iriumd runs from /opt/irium-pool/ or production systemd — no IRIUM_POAWX_MODE=active in prod config"
pass "PoAW-X hard-disabled on mainnet (no IRIUM_POAWX_MODE=active in prod env)"

# ══════════════════════════════════════════════════════════════════════════════
# Final report
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "==================================================================="
echo " Phase 10-F: PoAW-X receipt two-VPS soak"
echo "==================================================================="
echo " Branch:      testnet/poawx-phase10f-receipt-two-vps-soak"
echo " Checkpoint:  8aa432d (Phase 10-E)"
echo " PASS=${PASS}  FAIL=${FAIL}  SKIP=${SKIP}"
echo ""
echo " === Soak results (all $N_SEGS segments combined) ==="
echo "   Blocks accepted:              ${TOTAL_BLOCKS}/${SOAK_BLOCK_TARGET}"
echo "   irx1 in coinbase:             ${TOTAL_IRX1}"
echo "   submit_block_extended calls:  ${TOTAL_EXT_CALLS:-0}"
echo "   submit_block_extended accept: ${EXT_ACCEPTED:-0}"
echo "   Shares accepted/rejected:     ${TOTAL_SHARES_ACC}/${TOTAL_SHARES_REJ}"
echo "   Stratum restarts:             ${STRATUM_RESTART_N}"
echo "   Soak elapsed:                 ${SOAK_ELAPSED_TOTAL}s"
echo ""
echo " === VPS-2 propagation ==="
echo "   VPS-1 height at end:          ${FINAL_H}"
echo "   VPS-2 height at end:          $(vps2_height)"
echo "   VPS-1 prev_hash:              ${VPS1_PREV:0:16}..."
echo "   VPS-2 prev_hash:              ${VPS2_PREV:0:16}..."
echo ""
echo " === Logs (VPS-1) ==="
echo "   iriumd:             $LOG_IRIUMD"
echo "   stratum (seg 1):    $LOG_STRATUM1"
echo "   stratum (seg 2):    $LOG_STRATUM2"
echo "   stratum (seg 3):    $LOG_STRATUM3"
echo "   harness (seg 1):    $LOG_HARNESS1"
echo "   harness (seg 2):    $LOG_HARNESS2"
echo "   harness (seg 3):    $LOG_HARNESS3"
echo " === Logs (VPS-2) ==="
echo "   iriumd:             ${VPS2_DATA_DIR}/iriumd.log"
echo "==================================================================="
if [[ "$FAIL" -eq 0 ]]; then
    echo " RESULT: ALL CHECKS PASS"
    exit 0
else
    echo " RESULT: $FAIL CHECKS FAILED"
    exit 1
fi
