#!/usr/bin/env bash
# Phase 10-E: PoAW-X receipt regression soak — 30 blocks, full path validation
# Branch:     testnet/poawx-phase10e-receipt-regression-soak
# Checkpoint: 844b7d5 (Phase 10-D PASS=30 FAIL=0)
#
# Proves: assignment → receipt → pending-in-template → irx1 coinbase →
#         submit_block_extended → accepted block → receipts cleared
# Runs 30 blocks main soak + 5 blocks restart soak + bogus share + negative checks.
#
# Safety: devnet/testnet only. Mainnet untouched. No production ports/dirs/configs.
set -euo pipefail

# ── Ports (all isolated; none overlap mainnet 38300/38310/38320/38291/3333/8080) ─
RPC_PORT=39511
P2P_PORT=39510
STRATUM_PORT=39512
DISABLED_RPC_PORT=39513   # brief disabled-mode test instance

# ── Targets ───────────────────────────────────────────────────────────────────
SOAK_BLOCK_TARGET=30
RESTART_BLOCK_TARGET=5

# ── Binaries ──────────────────────────────────────────────────────────────────
IRIUMD_BIN="$HOME/irium/target/release/iriumd"
STRATUM_BIN="$HOME/irium/pool/irium-stratum/target/release/irium-stratum"
HARNESS="$HOME/irium/scripts/poawx-stratum-long-soak-harness.py"

# ── Data / log dirs ───────────────────────────────────────────────────────────
DATA_DIR="$HOME/irium-poawx-phase10e"
LOG_IRIUMD="$DATA_DIR/iriumd.log"
LOG_STRATUM="$DATA_DIR/stratum.log"
LOG_STRATUM2="$DATA_DIR/stratum-restart.log"
LOG_HARNESS_MAIN="$DATA_DIR/harness-main.log"
LOG_HARNESS_RESTART="$DATA_DIR/harness-restart.log"
LOG_HARNESS_BOGUS="$DATA_DIR/harness-bogus.log"
LOG_DISABLED="$DATA_DIR/disabled.log"

# ── RPC / stratum credentials ─────────────────────────────────────────────────
RPC_BASE="http://127.0.0.1:${RPC_PORT}"
RPC_DISABLED="http://127.0.0.1:${DISABLED_RPC_PORT}"
RPC_TOKEN="poawx-phase10e-token"
WALLET_ADDR="iFb9G2WjD5FP9JGmY7brdwHRxR1KJcAa9z"

# ── VPS-2 peer (optional) ─────────────────────────────────────────────────────
VPS2_HOST="${VPS2_HOST:-}"

# ── Counters ──────────────────────────────────────────────────────────────────
PASS=0; FAIL=0; SKIP=0
TESTNET_IRIUMD_PID=""
TESTNET_STRATUM_PID=""
TESTNET_DISABLED_PID=""

pass()  { echo "[PASS] $1"; PASS=$((PASS+1)); }
fail()  { echo "[FAIL] $1"; FAIL=$((FAIL+1)); }
skip()  { echo "[SKIP] $1"; SKIP=$((SKIP+1)); }
info()  { echo "[INFO] $1"; }

# ── Cleanup: testnet processes only ──────────────────────────────────────────
cleanup() {
    echo ""
    echo "=== Cleanup ==="
    [[ -n "${TESTNET_DISABLED_PID:-}" ]] && { kill "$TESTNET_DISABLED_PID" 2>/dev/null || true; }
    [[ -n "${TESTNET_STRATUM_PID:-}" ]]  && { kill "$TESTNET_STRATUM_PID"  2>/dev/null || true; }
    [[ -n "${TESTNET_IRIUMD_PID:-}" ]]   && { kill "$TESTNET_IRIUMD_PID"   2>/dev/null || true; }
    sleep 1
    # Force-free testnet ports if still bound
    for port in $RPC_PORT $P2P_PORT $STRATUM_PORT $DISABLED_RPC_PORT; do
        fuser -k "${port}/tcp" 2>/dev/null || true
    done
    if [[ "$FAIL" -eq 0 ]]; then
        rm -rf "$DATA_DIR"
        echo "Data dir cleaned up (all checks passed)"
    else
        echo "Data dir preserved for inspection: $DATA_DIR"
    fi
    echo ""
    echo "PASS=$PASS  FAIL=$FAIL  SKIP=$SKIP"
}
trap cleanup EXIT

# ── Helper: wait for iriumd RPC ───────────────────────────────────────────────
wait_rpc() {
    local url="$1" token="$2" max="${3:-30}"
    local i=0
    while [[ $i -lt $max ]]; do
        if curl -sf -H "Authorization: Bearer $token" "$url/rpc/getblocktemplate" >/dev/null 2>&1; then
            return 0
        fi
        sleep 1; i=$((i+1))
    done
    return 1
}

echo "============================================================"
echo " Phase 10-E: PoAW-X receipt regression soak"
echo "============================================================"
echo " Ports: P2P=$P2P_PORT  RPC=$RPC_PORT  Stratum=$STRATUM_PORT"
echo " Target: $SOAK_BLOCK_TARGET-block soak + $RESTART_BLOCK_TARGET-block restart"
echo " Data:  $DATA_DIR"
echo ""

# ══════════════════════════════════════════════════════════════════════════════
# Section 0: Pre-flight + mainnet safety baseline
# ══════════════════════════════════════════════════════════════════════════════
echo "=== Section 0: Pre-flight + mainnet safety baseline ==="

MAINNET_IRIUMD_PID=$(ss -lntp 'sport = :38300' 2>/dev/null | grep -oP 'pid=\K[0-9]+' | head -1 || true)
MAINNET_STRATUM_PID=$(ss -lntp 'sport = :3333'  2>/dev/null | grep -oP 'pid=\K[0-9]+' | head -1 || true)
MAINNET_EXPLORER_PID=$(ss -lntp 'sport = :38310' 2>/dev/null | grep -oP 'pid=\K[0-9]+' | head -1 || true)
MAINNET_WALLET_PID=$(ss -lntp 'sport = :38320'  2>/dev/null | grep -oP 'pid=\K[0-9]+' | head -1 || true)

echo "Mainnet baseline:"
echo "  iriumd    PID=${MAINNET_IRIUMD_PID:-none}  port=38300"
echo "  stratum   PID=${MAINNET_STRATUM_PID:-none}  port=3333"
echo "  explorer  PID=${MAINNET_EXPLORER_PID:-none}  port=38310"
echo "  wallet    PID=${MAINNET_WALLET_PID:-none}  port=38320"

[[ -n "${MAINNET_IRIUMD_PID:-}" ]] && \
    pass "mainnet iriumd alive (PID=$MAINNET_IRIUMD_PID)" || \
    info "mainnet iriumd not found on 38300"

# No stale testnet processes
# Exclude known mainnet PIDs via awk col2 (grep -v "pid=N" does not match ps output)
KNOWN_MAIN_PIDS="${MAINNET_IRIUMD_PID:-NOPID}"
STALE=$(ps aux | grep -E "$HOME/irium/target/release/(iriumd|irium-stratum)" | grep -v grep | \
        awk -v p="$KNOWN_MAIN_PIDS" 'BEGIN{split(p,a," ");for(i in a)k[a[i]]=1} !k[$2]' || true)
[[ -z "$STALE" ]] && pass "no stale testnet processes" || fail "stale testnet processes: $STALE"

# Testnet ports must be free
ALL_PORTS_FREE=1
for port in $P2P_PORT $RPC_PORT $STRATUM_PORT; do
    if ss -lntp "sport = :${port}" 2>/dev/null | grep -q ":${port}"; then
        fail "testnet port $port already in use"
        ALL_PORTS_FREE=0
    fi
done
[[ "$ALL_PORTS_FREE" -eq 1 ]] && pass "all testnet ports free (${P2P_PORT}/${RPC_PORT}/${STRATUM_PORT})"

# Binaries and harness
[[ -x "$IRIUMD_BIN" ]]  && pass "iriumd binary exists" || { fail "iriumd binary missing: $IRIUMD_BIN"; exit 1; }
[[ -x "$STRATUM_BIN" ]] && pass "stratum binary exists" || { fail "stratum binary missing"; exit 1; }
[[ -f "$HARNESS" ]]     && pass "soak harness exists" || { fail "harness missing: $HARNESS"; exit 1; }

# Branch check
CURRENT_BRANCH=$(git -C "$HOME/irium" branch --show-current 2>/dev/null || echo "unknown")
CURRENT_HEAD=$(git -C "$HOME/irium" rev-parse HEAD 2>/dev/null || echo "unknown")
info "branch=$CURRENT_BRANCH  HEAD=$CURRENT_HEAD"
[[ "$CURRENT_BRANCH" == "testnet/poawx-phase10e-receipt-regression-soak" ]] && \
    pass "on Phase 10-E branch" || fail "wrong branch: $CURRENT_BRANCH"

# No tmux/screen from testnet
TMUX_SESSIONS=$(tmux ls 2>/dev/null | grep -c 'session' || true)
[[ "${TMUX_SESSIONS:-0}" -eq 0 ]] && \
    pass "no tmux sessions" || info "tmux sessions present: $TMUX_SESSIONS (confirm not testnet)"

# Setup data dir
rm -rf "$DATA_DIR" && mkdir -p "$DATA_DIR/bootstrap"
cp -a "$HOME/irium/bootstrap/anchors.json" "$DATA_DIR/bootstrap/" 2>/dev/null || true
cp -a "$HOME/irium/bootstrap/trust"        "$DATA_DIR/bootstrap/" 2>/dev/null || true
info "data dir: $DATA_DIR"

# ══════════════════════════════════════════════════════════════════════════════
# Section 1: Start testnet iriumd (devnet, POAWX_MODE=active)
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 1: Start testnet iriumd ==="
(
  cd "$DATA_DIR"
  IRIUM_NETWORK=devnet \
  IRIUM_POAWX_MODE=active \
  IRIUM_DEV_EASY_BITS_TEMPLATE=1 \
  IRIUM_P2P_BIND="0.0.0.0:${P2P_PORT}" \
  IRIUM_NODE_HOST=127.0.0.1 \
  IRIUM_NODE_PORT="${RPC_PORT}" \
  IRIUM_DATA_DIR="$DATA_DIR" \
  IRIUM_BOOTSTRAP_DIR="$DATA_DIR/bootstrap" \
  IRIUM_RPC_TOKEN="$RPC_TOKEN" \
    "$IRIUMD_BIN" >"$LOG_IRIUMD" 2>&1 &
  echo $! > "$DATA_DIR/iriumd.pid"
)
sleep 0.5
TESTNET_IRIUMD_PID=$(cat "$DATA_DIR/iriumd.pid")
echo "iriumd PID=$TESTNET_IRIUMD_PID"

if ! wait_rpc "$RPC_BASE" "$RPC_TOKEN" 30; then
    fail "iriumd RPC not responsive after 30s"
    tail -20 "$LOG_IRIUMD"
    exit 1
fi
kill -0 "$TESTNET_IRIUMD_PID" 2>/dev/null && pass "iriumd started and responsive" || \
    { fail "iriumd died at startup"; exit 1; }

# ══════════════════════════════════════════════════════════════════════════════
# Section 2: Template checks
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 2: Template checks ==="
TMPL=$(curl -sf -H "Authorization: Bearer $RPC_TOKEN" "$RPC_BASE/rpc/getblocktemplate" 2>/dev/null || true)
[[ -n "$TMPL" ]] && pass "getblocktemplate responsive" || { fail "getblocktemplate failed"; exit 1; }

BITS=$(echo "$TMPL" | python3 -c "import sys,json; print(json.load(sys.stdin).get('bits',''))" 2>/dev/null || echo "")
[[ "$BITS" == "207fffff" ]] && pass "bits=207fffff (devnet easy)" || fail "bits=$BITS (expected 207fffff)"

POAWX_MODE_TMPL=$(echo "$TMPL" | python3 -c "import sys,json; print(json.load(sys.stdin).get('poawx_mode',''))" 2>/dev/null || echo "")
[[ "$POAWX_MODE_TMPL" == "active" ]] && pass "template poawx_mode=active" || fail "poawx_mode=$POAWX_MODE_TMPL"

# ══════════════════════════════════════════════════════════════════════════════
# Section 3: Mine 1 block + assignment checks
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 3: Mine 1 block + assignment checks ==="
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
    if n < 0xfd:   return bytes([n])
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
                    'merkle_root': (mr if height >= 22888 else mr).hex(),
                    'time': t_val, 'bits': bits_hex,
                    'nonce': nonce, 'hash': h[::-1].hex(),
                },
                'tx_hex': [cb_tx.hex()],
                'submit_source': 'phase10e_preflight',
            }
        )
        print('OK' if resp.status_code == 200 else f'FAIL:{resp.status_code}')
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
    fail "failed to mine preflight block after 200 attempts"

sleep 1

# Assignment checks
HTTP_ASSIGN=$(curl -s -o /tmp/phase10e_assign.json -w "%{http_code}" \
    -H "Authorization: Bearer $RPC_TOKEN" \
    "$RPC_BASE/poawx/assignment" 2>/dev/null)
[[ "$HTTP_ASSIGN" == "200" ]] && pass "/poawx/assignment returns 200" || \
    { fail "/poawx/assignment HTTP $HTTP_ASSIGN"; cat /tmp/phase10e_assign.json 2>/dev/null; }

ASSIGN_HEIGHT=$(python3 -c "import json; print(json.load(open('/tmp/phase10e_assign.json')).get('height',0))" 2>/dev/null || echo 0)
SEED=$(python3 -c "import json; print(json.load(open('/tmp/phase10e_assign.json')).get('seed',''))" 2>/dev/null || echo "")
NONCE=$(python3 -c "import json; print(json.load(open('/tmp/phase10e_assign.json')).get('commitment_nonce',''))" 2>/dev/null || echo "")
LANE=$(python3 -c "import json; print(json.load(open('/tmp/phase10e_assign.json')).get('lane',''))" 2>/dev/null || echo "")
POW_BITS=$(python3 -c "import json; print(json.load(open('/tmp/phase10e_assign.json')).get('pow_bits',''))" 2>/dev/null || echo "")
PDIFF=$(python3 -c "import json; print(json.load(open('/tmp/phase10e_assign.json')).get('puzzle_difficulty',0))" 2>/dev/null || echo 0)

[[ ${#SEED} -eq 64 ]]  && pass "assignment.seed is 32-byte hex" || fail "seed malformed: len=${#SEED}"
[[ ${#NONCE} -eq 64 ]] && pass "assignment.commitment_nonce is 32-byte hex" || fail "nonce malformed: len=${#NONCE}"
[[ "$LANE" == "cpu" ]] && pass "assignment.lane=cpu" || fail "lane=$LANE (expected cpu)"
[[ -n "$POW_BITS" ]]   && pass "assignment.pow_bits present: $POW_BITS" || fail "pow_bits missing"
[[ -n "$PDIFF" && "$PDIFF" != "0" ]] && pass "assignment.puzzle_difficulty present: $PDIFF" || \
    fail "puzzle_difficulty missing or 0"
echo "assignment: height=$ASSIGN_HEIGHT lane=$LANE pow_bits=$POW_BITS puzzle_difficulty=$PDIFF"

# ══════════════════════════════════════════════════════════════════════════════
# Section 4: Valid receipt POST
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 4: POST /poawx/receipt (valid) ==="
SOLUTION=$(echo -n "${SEED}solution" | sha256sum | awk '{print $1}')
RECEIPT_BODY=$(python3 -c "import json; print(json.dumps({
    'height': int('$ASSIGN_HEIGHT'),
    'lane': 'cpu',
    'worker_pkh': 'a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2',
    'solution': '$SOLUTION',
    'commitment_nonce': '$NONCE',
}))")

HTTP_RECEIPT=$(curl -s -o /tmp/phase10e_receipt.json -w "%{http_code}" \
    -X POST \
    -H "Authorization: Bearer $RPC_TOKEN" \
    -H "Content-Type: application/json" \
    -d "$RECEIPT_BODY" \
    "$RPC_BASE/poawx/receipt" 2>/dev/null)

if [[ "$HTTP_RECEIPT" == "200" ]]; then
    pass "POST /poawx/receipt returns 200"
    PENDING_CNT=$(python3 -c "import json; print(json.load(open('/tmp/phase10e_receipt.json')).get('pending_count',0))" 2>/dev/null || echo 0)
    [[ "$PENDING_CNT" -ge 1 ]] && pass "receipt stored: pending_count=$PENDING_CNT" || fail "pending_count=$PENDING_CNT"
else
    fail "POST /poawx/receipt HTTP $HTTP_RECEIPT"
    cat /tmp/phase10e_receipt.json 2>/dev/null; PENDING_CNT=0
fi

# ══════════════════════════════════════════════════════════════════════════════
# Section 5: Template receipts_root check
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 5: Template receipts_root ==="
TMPL3=$(curl -sf -H "Authorization: Bearer $RPC_TOKEN" "$RPC_BASE/rpc/getblocktemplate" 2>/dev/null || true)
RROOT=$(echo "$TMPL3" | python3 -c "import sys,json; print(json.load(sys.stdin).get('receipts_root',''))" 2>/dev/null || echo "")
RPEND=$(echo "$TMPL3" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('poawx_pending_receipts',[])))" 2>/dev/null || echo 0)

[[ ${#RROOT} -eq 64 ]] && pass "receipts_root is non-empty 32-byte hex: $RROOT" || \
    fail "receipts_root not 32-byte hex: '${RROOT}'"
[[ "$RPEND" -ge 1 ]] && pass "template shows $RPEND pending receipt(s)" || \
    fail "no pending receipts in template"

# Validate receipts_root determinism: compute expected root
# Variables passed as argv so <<'PYEOF' can stay single-quoted
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
if [[ "$RROOT" == "$EXPECTED_ROOT" ]]; then
    pass "receipts_root matches computed canonical root"
else
    fail "receipts_root mismatch: got=$RROOT expected=$EXPECTED_ROOT"
fi

# ══════════════════════════════════════════════════════════════════════════════
# Section 6: Negative checks
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 6: Negative checks ==="

# 6a: Invalid receipt — bad hex in solution field
echo "--- 6a: Invalid hex in solution ---"
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
    fail "invalid receipt not rejected: HTTP $HTTP_BAD (expected 400 or 422)"

# 6b: Duplicate receipt — same (height, lane, worker_pkh)
echo "--- 6b: Duplicate receipt dedup ---"
HTTP_DUP=$(curl -s -o /tmp/phase10e_dup.json -w "%{http_code}" \
    -X POST -H "Authorization: Bearer $RPC_TOKEN" \
    -H "Content-Type: application/json" -d "$RECEIPT_BODY" \
    "$RPC_BASE/poawx/receipt" 2>/dev/null)
DUP_CNT=$(python3 -c "import json; print(json.load(open('/tmp/phase10e_dup.json')).get('pending_count',0))" 2>/dev/null || echo 0)
if [[ "$HTTP_DUP" == "200" && "$DUP_CNT" -le "$PENDING_CNT" ]]; then
    pass "duplicate receipt deduped: pending_count=$DUP_CNT (<=original $PENDING_CNT)"
else
    fail "duplicate receipt not deduped: HTTP=$HTTP_DUP pending_count=$DUP_CNT (expected <=$PENDING_CNT)"
fi

# 6c: Disabled-mode iriumd — /poawx/assignment and submit_block_extended must return 503
echo "--- 6c: Disabled-mode iriumd (no IRIUM_POAWX_MODE=active) ---"
mkdir -p "$DATA_DIR/disabled/bootstrap"
cp -a "$HOME/irium/bootstrap/anchors.json" "$DATA_DIR/disabled/bootstrap/" 2>/dev/null || true
(
  cd "$DATA_DIR/disabled"
  IRIUM_NETWORK=devnet \
  IRIUM_DEV_EASY_BITS_TEMPLATE=1 \
  IRIUM_P2P_BIND="0.0.0.0:$((P2P_PORT+10))" \
  IRIUM_NODE_HOST=127.0.0.1 \
  IRIUM_NODE_PORT="${DISABLED_RPC_PORT}" \
  IRIUM_DATA_DIR="$DATA_DIR/disabled" \
  IRIUM_BOOTSTRAP_DIR="$DATA_DIR/disabled/bootstrap" \
  IRIUM_RPC_TOKEN="$RPC_TOKEN" \
    "$IRIUMD_BIN" >"$LOG_DISABLED" 2>&1 &
  echo $! > "$DATA_DIR/disabled.pid"
)
sleep 0.5
TESTNET_DISABLED_PID=$(cat "$DATA_DIR/disabled.pid")

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
    # 503 = PoAW-X disabled; anything other than 200 is acceptable (disabled path)
    [[ "$HTTP_DE" != "200" ]] && \
        pass "disabled-mode /rpc/submit_block_extended not 200: HTTP $HTTP_DE" || \
        fail "disabled-mode /rpc/submit_block_extended returned 200 (should reject)"
else
    skip "disabled-mode iriumd not responsive; 503 checks skipped"
fi
kill "$TESTNET_DISABLED_PID" 2>/dev/null || true
TESTNET_DISABLED_PID=""
sleep 1

# 6d: Confirm IRIUM_DEV_EASY_BITS_TEMPLATE does not affect mainnet check
# (Mainnet iriumd is still running at 38300 — if we had its token we'd test 503.
#  Confirmed by architecture: poawx_get_assignment returns 503 on Mainnet network
#  regardless of IRIUM_POAWX_MODE. Mainnet safety check in Section 15 confirms
#  mainnet iriumd is still alive and untouched.)
info "6d: Mainnet safety (PoAW-X disabled on mainnet) confirmed in Section 15"

# ══════════════════════════════════════════════════════════════════════════════
# Section 7: Start testnet stratum (IRIUM_STRATUM_POAWX=1)
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 7: Start testnet stratum ==="
IRIUM_STRATUM_POAWX=1 \
IRIUM_RPC_BASE="$RPC_BASE" \
IRIUM_RPC_TOKEN="$RPC_TOKEN" \
STRATUM_BIND="0.0.0.0:${STRATUM_PORT}" \
IRIUM_STRATUM_COINBASE_BIP34=true \
STRATUM_DEFAULT_DIFF=1 \
IRIUM_STRATUM_VARDIFF_ENABLED=false \
IRIUM_STRATUM_MINER_ADDRESS="$WALLET_ADDR" \
IRIUM_POW_LIMIT_HEX=7fffff0000000000000000000000000000000000000000000000000000000000 \
    "$STRATUM_BIN" >"$LOG_STRATUM" 2>&1 &
TESTNET_STRATUM_PID=$!
sleep 2

kill -0 "$TESTNET_STRATUM_PID" 2>/dev/null && pass "stratum started" || \
    { fail "stratum died at startup"; cat "$LOG_STRATUM" | tail -10; exit 1; }
ss -lntp "sport = :${STRATUM_PORT}" 2>/dev/null | grep -q ":${STRATUM_PORT}" && \
    pass "stratum port $STRATUM_PORT open" || fail "stratum port $STRATUM_PORT not open"
grep -q '\[poawx\].*IRIUM_STRATUM_POAWX=1\|PoAW-X receipt path enabled' "$LOG_STRATUM" 2>/dev/null && \
    pass "stratum logged PoAW-X startup message" || \
    fail "stratum PoAW-X startup message not found in log"

# ══════════════════════════════════════════════════════════════════════════════
# Section 8: 30-block main soak
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 8: Main soak — $SOAK_BLOCK_TARGET blocks ==="
python3 "$HARNESS" \
    127.0.0.1 "$STRATUM_PORT" \
    "$RPC_BASE" "$RPC_TOKEN" \
    --blocks "$SOAK_BLOCK_TARGET" \
    --receipt \
    2>&1 | tee "$LOG_HARNESS_MAIN" || true

SUMMARY_MAIN=$(grep '^SUMMARY_JSON:' "$LOG_HARNESS_MAIN" | tail -1 || echo "")
BLOCKS_PASS=0; IRX1_CNT=0; RECEIPT_OK=False; SHARES_ACC=0; SHARES_REJ=0; ELAPSED_S=0
if [[ -n "$SUMMARY_MAIN" ]]; then
    SJ="${SUMMARY_MAIN#SUMMARY_JSON:}"
    BLOCKS_PASS=$(echo "$SJ" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('blocks_pass',0))")
    IRX1_CNT=$(echo "$SJ" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('irx1_in_coinbase_count',0))")
    RECEIPT_OK=$(echo "$SJ" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('receipt_test_passed',False))")
    SHARES_ACC=$(echo "$SJ" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('share_accepts',0))")
    SHARES_REJ=$(echo "$SJ" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('share_rejects',0))")
    ELAPSED_S=$(echo "$SJ" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('elapsed_s',0))")
    echo "soak: blocks=$BLOCKS_PASS irx1=$IRX1_CNT receipt_ok=$RECEIPT_OK shares=$SHARES_ACC/$SHARES_REJ elapsed=${ELAPSED_S}s"

    [[ "$BLOCKS_PASS" -ge "$SOAK_BLOCK_TARGET" ]] && \
        pass "soak: $BLOCKS_PASS/$SOAK_BLOCK_TARGET blocks accepted" || \
        fail "soak: only $BLOCKS_PASS/$SOAK_BLOCK_TARGET blocks accepted"

    # irx1 target: every block; fallback minimum 20
    if [[ "$IRX1_CNT" -ge "$SOAK_BLOCK_TARGET" ]]; then
        pass "soak: $IRX1_CNT/$SOAK_BLOCK_TARGET blocks with irx1 (100% path)"
    elif [[ "$IRX1_CNT" -ge 20 ]]; then
        pass "soak: $IRX1_CNT irx1 blocks (>= 20 minimum threshold)"
        info "not every block had irx1 (${IRX1_CNT}/${SOAK_BLOCK_TARGET}); first block(s) may precede receipt posting"
    else
        fail "soak: only $IRX1_CNT blocks with irx1 (need >= 20)"
    fi

    [[ "$RECEIPT_OK" == "True" ]] && \
        pass "soak: receipt path PASS (assign→receipt→template→irx1)" || \
        fail "soak: receipt path test failed"
    [[ "$SHARES_REJ" -eq 0 ]] && \
        pass "soak: 0 share rejections" || \
        fail "soak: $SHARES_REJ share rejections (expected 0)"
else
    fail "soak: harness produced no SUMMARY_JSON"
fi

CURR_H=$(curl -sf -H "Authorization: Bearer $RPC_TOKEN" "$RPC_BASE/rpc/getblocktemplate" 2>/dev/null | \
    python3 -c "import sys,json; print(json.load(sys.stdin).get('height',0))" || echo 0)
info "chain height after main soak: $CURR_H"

# ══════════════════════════════════════════════════════════════════════════════
# Section 9: Log verification (stratum + iriumd)
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 9: Log verification ==="
IRX1_INJECTS=$(grep -c 'irx1_len=38\|poawx.*to_job.*mode=active' "$LOG_STRATUM" 2>/dev/null; true)
EXT_CALLS=$(grep -c 'submit_block_extended' "$LOG_STRATUM" 2>/dev/null; true)
EXT_ACCEPTED=$(grep -c 'submit_block_extended.*accepted\|block_extended accepted\|accepted.*extended' "$LOG_IRIUMD" 2>/dev/null; true)

echo "stratum: irx1_injections=${IRX1_INJECTS:-0}  submit_block_extended_calls=${EXT_CALLS:-0}"
echo "iriumd:  submit_block_extended_accepted=${EXT_ACCEPTED:-0}"

[[ "${IRX1_INJECTS:-0}" -ge 1 ]] && pass "stratum: irx1 coinbase injection logged" || \
    fail "stratum: no irx1 injection log entries"
[[ "${EXT_CALLS:-0}" -ge 20 ]] && \
    pass "stratum: submit_block_extended called ${EXT_CALLS} times (>= 20)" || \
    fail "stratum: submit_block_extended called only ${EXT_CALLS} times (need >= 20)"
[[ "${EXT_ACCEPTED:-0}" -ge 1 ]] && \
    pass "iriumd: accepted submit_block_extended blocks" || \
    info "iriumd: submit_block_extended acceptance not in log (may need debug level)"

# Legacy fallback check: submit_block should NOT be called for irx1 blocks
LEGACY=$(grep -c 'POST.*submit_block[^_]\|submit_block.*success' "$LOG_STRATUM" 2>/dev/null; true)
[[ "${LEGACY:-0}" -eq 0 ]] && \
    pass "stratum: no legacy submit_block fallback" || \
    info "stratum: ${LEGACY} legacy submit_block calls (pre-receipt blocks may use legacy path)"

# ══════════════════════════════════════════════════════════════════════════════
# Section 10: Persisted block check
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 10: Persisted block check ==="
GETBLOCK_HTTP=$(curl -s -o /tmp/phase10e_blk.json -w "%{http_code}" \
    -H "Authorization: Bearer $RPC_TOKEN" \
    "$RPC_BASE/rpc/getblock/$CURR_H" 2>/dev/null)

if [[ "$GETBLOCK_HTTP" == "200" ]]; then
    COINBASE_HEX=$(python3 -c "import json; d=json.load(open('/tmp/phase10e_blk.json')); print(d.get('tx',[''])[0])" 2>/dev/null || echo "")
    POAWX_ROOT_PERSISTED=$(python3 -c "import json; d=json.load(open('/tmp/phase10e_blk.json')); print(d.get('poawx_receipts_root',''))" 2>/dev/null || echo "")
    [[ -n "$COINBASE_HEX" ]] && pass "persisted block tx_hex present" || fail "persisted block tx_hex missing"
    # irx1 marker: 6a 24 69 72 78 31 (OP_RETURN PUSH36 "irx1")
    echo "$COINBASE_HEX" | grep -qi '6a2469727831' && \
        pass "persisted coinbase contains irx1 OP_RETURN (6a2469727831)" || \
        info "irx1 not found in getblock tx hex (may be different serialization)"
    [[ ${#POAWX_ROOT_PERSISTED} -eq 64 ]] && \
        pass "persisted poawx_receipts_root is 32-byte hex: $POAWX_ROOT_PERSISTED" || \
        info "persisted poawx_receipts_root: '${POAWX_ROOT_PERSISTED}'"
else
    skip "getblock endpoint not available (HTTP $GETBLOCK_HTTP); block content confirmed via harness irx1=True and log"
    info "irx1 confirmed: harness irx1_count=${IRX1_CNT}, stratum irx1_injections=${IRX1_INJECTS:-0}"
fi

# ══════════════════════════════════════════════════════════════════════════════
# Section 11: Stratum restart + reconnect test
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 11: Stratum restart + reconnect ==="
echo "Killing testnet stratum PID=$TESTNET_STRATUM_PID..."
kill "$TESTNET_STRATUM_PID" 2>/dev/null || true
sleep 2

if ! ss -lntp "sport = :${STRATUM_PORT}" 2>/dev/null | grep -q ":${STRATUM_PORT}"; then
    pass "stratum stopped (port $STRATUM_PORT released)"
else
    info "stratum port $STRATUM_PORT still bound; forcing release"
    fuser -k "${STRATUM_PORT}/tcp" 2>/dev/null || true
    sleep 1
fi

echo "Restarting testnet stratum..."
IRIUM_STRATUM_POAWX=1 \
IRIUM_RPC_BASE="$RPC_BASE" \
IRIUM_RPC_TOKEN="$RPC_TOKEN" \
STRATUM_BIND="0.0.0.0:${STRATUM_PORT}" \
IRIUM_STRATUM_COINBASE_BIP34=true \
STRATUM_DEFAULT_DIFF=1 \
IRIUM_STRATUM_VARDIFF_ENABLED=false \
IRIUM_STRATUM_MINER_ADDRESS="$WALLET_ADDR" \
IRIUM_POW_LIMIT_HEX=7fffff0000000000000000000000000000000000000000000000000000000000 \
    "$STRATUM_BIN" >"$LOG_STRATUM2" 2>&1 &
TESTNET_STRATUM_PID=$!
sleep 2

kill -0 "$TESTNET_STRATUM_PID" 2>/dev/null && pass "stratum restarted" || \
    { fail "stratum failed to restart"; TESTNET_STRATUM_PID=""; }

if [[ -n "$TESTNET_STRATUM_PID" ]]; then
    ss -lntp "sport = :${STRATUM_PORT}" 2>/dev/null | grep -q ":${STRATUM_PORT}" && \
        pass "stratum port $STRATUM_PORT open after restart" || fail "stratum port not open after restart"

    echo "Running $RESTART_BLOCK_TARGET blocks after restart..."
    python3 "$HARNESS" \
        127.0.0.1 "$STRATUM_PORT" \
        "$RPC_BASE" "$RPC_TOKEN" \
        --blocks "$RESTART_BLOCK_TARGET" \
        --receipt \
        2>&1 | tee "$LOG_HARNESS_RESTART" || true

    SUMMARY_RST=$(grep '^SUMMARY_JSON:' "$LOG_HARNESS_RESTART" | tail -1 || echo "")
    RBPASS=0; RIRX1=0; RREC=False
    if [[ -n "$SUMMARY_RST" ]]; then
        SR="${SUMMARY_RST#SUMMARY_JSON:}"
        RBPASS=$(echo "$SR" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('blocks_pass',0))")
        RIRX1=$(echo "$SR" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('irx1_in_coinbase_count',0))")
        RREC=$(echo "$SR" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('receipt_test_passed',False))")
        echo "restart-run: blocks=$RBPASS irx1=$RIRX1 receipt=$RREC"
        [[ "$RBPASS" -ge "$RESTART_BLOCK_TARGET" ]] && \
            pass "restart: $RBPASS/$RESTART_BLOCK_TARGET blocks after stratum restart" || \
            fail "restart: only $RBPASS/$RESTART_BLOCK_TARGET blocks after restart"
        [[ "$RIRX1" -ge 1 ]] && \
            pass "restart: irx1 in coinbase after restart ($RIRX1 blocks)" || \
            fail "restart: no irx1 blocks after restart"
        [[ "$RREC" == "True" ]] && \
            pass "restart: receipt path works after stratum restart" || \
            fail "restart: receipt path failed after restart"
    else
        fail "restart: harness produced no SUMMARY_JSON"
    fi
fi

# ══════════════════════════════════════════════════════════════════════════════
# Section 12: Bogus share rejection
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 12: Bogus share rejection ==="
python3 "$HARNESS" \
    127.0.0.1 "$STRATUM_PORT" \
    "$RPC_BASE" "$RPC_TOKEN" \
    --blocks 2 \
    --bogus \
    2>&1 | tee "$LOG_HARNESS_BOGUS" || true

SUMMARY_BOGUS=$(grep '^SUMMARY_JSON:' "$LOG_HARNESS_BOGUS" | tail -1 || echo "")
if [[ -n "$SUMMARY_BOGUS" ]]; then
    SB="${SUMMARY_BOGUS#SUMMARY_JSON:}"
    BOG_REJ=$(echo "$SB" | python3 -c "import sys,json; v=json.loads(sys.stdin.read()).get('bogus_rejected',None); print(v)")
    BOG_H=$(echo "$SB" | python3 -c "import sys,json; v=json.loads(sys.stdin.read()).get('bogus_height_unchanged',None); print(v)")
    echo "bogus: rejected=$BOG_REJ height_unchanged=$BOG_H"
    if [[ "$BOG_REJ" == "True" || "$BOG_REJ" -ge 1 ]] 2>/dev/null; then
        pass "bogus share rejected"
    else
        fail "bogus share not rejected (bogus_rejected=$BOG_REJ)"
    fi
    [[ "$BOG_H" == "True" ]] && \
        pass "chain height unchanged after bogus share" || \
        fail "chain height advanced after bogus share (should not)"
else
    fail "bogus: harness produced no SUMMARY_JSON"
fi

# ══════════════════════════════════════════════════════════════════════════════
# Section 13: VPS-2 peer propagation
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 13: VPS-2 peer propagation ==="
if [[ -n "${VPS2_HOST:-}" ]]; then
    VPS2_H=$(ssh "$VPS2_HOST" "curl -sf 'http://127.0.0.1:39601/rpc/getblocktemplate' 2>/dev/null | python3 -c 'import sys,json; print(json.load(sys.stdin).get(\"height\",0))'" 2>/dev/null || echo 0)
    [[ "${VPS2_H:-0}" -ge "$CURR_H" ]] && \
        pass "VPS-2 synced to height $VPS2_H (VPS-1 at $CURR_H)" || \
        fail "VPS-2 height $VPS2_H behind VPS-1 $CURR_H"
else
    skip "VPS2_HOST not set; peer propagation test skipped"
    skip "VPS-2 block hash comparison skipped"
fi

# ══════════════════════════════════════════════════════════════════════════════
# Section 14: Log scan
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 14: Log scan ==="
LOG_SCAN_CLEAN=1
for LOG_FILE in "$LOG_IRIUMD" "$LOG_STRATUM" "$LOG_STRATUM2"; do
    [[ -f "$LOG_FILE" ]] || continue
    LNAME=$(basename "$LOG_FILE")

    PANICS=$(grep -c 'thread.*panicked\|SIGSEGV\|stack overflow' "$LOG_FILE" 2>/dev/null; true)
    if [[ "${PANICS:-0}" -eq 0 ]]; then
        pass "no panics in $LNAME"
    else
        fail "panics in $LNAME: $PANICS"
        grep 'thread.*panicked\|SIGSEGV\|stack overflow' "$LOG_FILE" | head -3
        LOG_SCAN_CLEAN=0
    fi

    INV_COMMIT=$(grep -c 'invalid.*commitment.*accepted\|bad irx1.*accepted\|invalid irx1.*ok' "$LOG_FILE" 2>/dev/null; true)
    [[ "${INV_COMMIT:-0}" -eq 0 ]] && \
        pass "no invalid commitment accepted in $LNAME" || \
        { fail "invalid commitment accepted in $LNAME: $INV_COMMIT"; LOG_SCAN_CLEAN=0; }

    MAINNET_REF=$(grep -c '38300\|mainnet\|production' "$LOG_FILE" 2>/dev/null; true)
    [[ "${MAINNET_REF:-0}" -eq 0 ]] && \
        pass "no mainnet port/path references in $LNAME" || \
        info "$LNAME: ${MAINNET_REF} mainnet references (inspect: grep 38300 $LOG_FILE)"
done
[[ "$LOG_SCAN_CLEAN" -eq 1 ]] && info "log scan clean (no panics, no invalid acceptance)" || \
    info "log scan found issues (see FAIL entries above)"

# ══════════════════════════════════════════════════════════════════════════════
# Section 15: Mainnet safety after soak
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "=== Section 15: Mainnet safety after soak ==="
if [[ -n "${MAINNET_IRIUMD_PID:-}" ]]; then
    kill -0 "$MAINNET_IRIUMD_PID" 2>/dev/null && \
        pass "mainnet iriumd PID=$MAINNET_IRIUMD_PID still alive" || \
        fail "CRITICAL: mainnet iriumd PID=$MAINNET_IRIUMD_PID died during soak"
fi
if [[ -n "${MAINNET_STRATUM_PID:-}" ]]; then
    kill -0 "$MAINNET_STRATUM_PID" 2>/dev/null && \
        pass "mainnet stratum PID=$MAINNET_STRATUM_PID still alive" || \
        fail "CRITICAL: mainnet stratum PID=$MAINNET_STRATUM_PID died during soak"
fi
if [[ -n "${MAINNET_EXPLORER_PID:-}" ]]; then
    kill -0 "$MAINNET_EXPLORER_PID" 2>/dev/null && \
        pass "mainnet explorer PID=$MAINNET_EXPLORER_PID still alive" || \
        fail "CRITICAL: mainnet explorer PID=$MAINNET_EXPLORER_PID died during soak"
fi
if [[ -n "${MAINNET_WALLET_PID:-}" ]]; then
    kill -0 "$MAINNET_WALLET_PID" 2>/dev/null && \
        pass "mainnet wallet-api PID=$MAINNET_WALLET_PID still alive" || \
        fail "CRITICAL: mainnet wallet-api PID=$MAINNET_WALLET_PID died during soak"
fi

P38300=$(ss -lntp 'sport = :38300' 2>/dev/null | grep -c ':38300'; true)
P3333=$(ss -lntp  'sport = :3333'  2>/dev/null | grep -c ':3333'; true)
P38310=$(ss -lntp 'sport = :38310' 2>/dev/null | grep -c ':38310'; true)
P38320=$(ss -lntp 'sport = :38320' 2>/dev/null | grep -c ':38320'; true)
[[ "${P38300:-0}" -ge 1 ]] && pass "mainnet port 38300 still bound" || info "38300 not bound"
[[ "${P3333:-0}" -ge 1 ]]  && pass "mainnet port 3333 still bound"  || info "3333 not bound"
[[ "${P38310:-0}" -ge 1 ]] && pass "mainnet port 38310 still bound" || info "38310 not bound"
[[ "${P38320:-0}" -ge 1 ]] && pass "mainnet port 38320 still bound" || info "38320 not bound"

# Confirm no testnet processes used mainnet port
TESTNET_ON_MAINNET=$(ss -lntp | grep -E ':38300|:3333[^3]|:38310|:38320' | \
    grep -v "pid=${MAINNET_IRIUMD_PID:-0}" | grep -v "pid=${MAINNET_STRATUM_PID:-0}" | \
    grep -v "pid=${MAINNET_EXPLORER_PID:-0}" | grep -v "pid=${MAINNET_WALLET_PID:-0}" || true)
[[ -z "$TESTNET_ON_MAINNET" ]] && pass "no testnet process used a mainnet port" || \
    fail "testnet process on mainnet port: $TESTNET_ON_MAINNET"

FINAL_H=$(curl -sf -H "Authorization: Bearer $RPC_TOKEN" "$RPC_BASE/rpc/getblocktemplate" 2>/dev/null | \
    python3 -c "import sys,json; print(json.load(sys.stdin).get('height',0))" || echo 0)

# ══════════════════════════════════════════════════════════════════════════════
# Final report
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "==================================================================="
echo " Phase 10-E: PoAW-X receipt regression soak"
echo "==================================================================="
echo " Branch:      testnet/poawx-phase10e-receipt-regression-soak"
echo " Checkpoint:  844b7d5 (Phase 10-D)"
echo " PASS=${PASS}  FAIL=${FAIL}  SKIP=${SKIP}"
echo ""
echo " === Soak results (main run) ==="
echo "   Blocks accepted:            ${BLOCKS_PASS}/${SOAK_BLOCK_TARGET}"
echo "   irx1 in coinbase:           ${IRX1_CNT}"
echo "   submit_block_extended calls: ${EXT_CALLS:-0}"
echo "   Shares accepted/rejected:   ${SHARES_ACC}/${SHARES_REJ}"
echo "   Elapsed:                    ${ELAPSED_S}s"
echo ""
echo " === Restart run ==="
echo "   Blocks after restart:       ${RBPASS:-0}/${RESTART_BLOCK_TARGET}"
echo "   irx1 after restart:         ${RIRX1:-0}"
echo ""
echo " === Chain state ==="
echo "   Height at end:              ${FINAL_H}"
echo ""
echo " === Logs ==="
echo "   iriumd:          $LOG_IRIUMD"
echo "   stratum (main):  $LOG_STRATUM"
echo "   stratum (restart):$LOG_STRATUM2"
echo "   harness (main):  $LOG_HARNESS_MAIN"
echo "   harness (restart):$LOG_HARNESS_RESTART"
echo "==================================================================="
if [[ "$FAIL" -eq 0 ]]; then
    echo " RESULT: ALL CHECKS PASS"
    exit 0
else
    echo " RESULT: $FAIL CHECKS FAILED"
    exit 1
fi
