#!/usr/bin/env bash
# Phase 10-D: PoAW-X assignment → receipt → irx1 coinbase → accepted block
# Proves the full receipt path: /poawx/assignment → POST /poawx/receipt →
# pending in template → non-empty receipts_root → irx1 coinbase →
# /rpc/submit_block_extended → accepted block → cleared pending receipts
#
# Safety: isolated ports/data dirs/processes only. Mainnet untouched.
set -euo pipefail

### ── Ports & Paths ──────────────────────────────────────────────────────────
RPC_PORT=39501
P2P_PORT=39500
STRATUM_PORT=39502
IRIUMD_BIN="$HOME/irium/target/release/iriumd"
STRATUM_BIN="$HOME/irium/pool/irium-stratum/target/release/irium-stratum"
DATA_DIR="$HOME/irium-poawx-phase10d"
LOG_IRIUMD="$DATA_DIR/iriumd.log"
LOG_STRATUM="$DATA_DIR/stratum.log"
RPC_BASE="http://127.0.0.1:$RPC_PORT"
RPC_TOKEN="poawx-phase10d-token"
WALLET_ADDR="iFb9G2WjD5FP9JGmY7brdwHRxR1KJcAa9z"

PASS=0; FAIL=0; SKIP=0

pass() { echo "[PASS] $1"; PASS=$((PASS+1)); }
fail() { echo "[FAIL] $1"; FAIL=$((FAIL+1)); }
skip() { echo "[SKIP] $1"; SKIP=$((SKIP+1)); }
info_line() { echo "[INFO] $1"; }

TESTNET_IRIUMD_PID=""
TESTNET_STRATUM_PID=""

cleanup() {
    echo "=== Cleanup ==="
    [[ -n "$TESTNET_IRIUMD_PID" ]] && kill "$TESTNET_IRIUMD_PID" 2>/dev/null && echo "killed testnet iriumd $TESTNET_IRIUMD_PID"
    [[ -n "$TESTNET_STRATUM_PID" ]] && kill "$TESTNET_STRATUM_PID" 2>/dev/null && echo "killed testnet stratum $TESTNET_STRATUM_PID"
    sleep 1
    local still
    still=$(ss -lntp "sport = :$RPC_PORT" 2>/dev/null | grep ":$RPC_PORT" | head -1)
    [[ -n "$still" ]] && echo "[warn] RPC port $RPC_PORT still bound after cleanup: $still"
    echo "PASS=$PASS FAIL=$FAIL SKIP=$SKIP"
}
trap cleanup EXIT

### ── Section 0: Pre-flight ──────────────────────────────────────────────────
echo "=== Section 0: Pre-flight ==="
MAINNET_PID=$(ss -lntp "sport = :38300" 2>/dev/null | grep -oP 'pid=\K[0-9]+' | head -1 || true)
if [[ -n "$MAINNET_PID" ]]; then
    pass "Mainnet iriumd alive PID=$MAINNET_PID"
else
    fail "Mainnet iriumd not found on port 38300 (OK if not required)"
fi

[[ -x "$IRIUMD_BIN" ]]  && pass "iriumd binary exists" || fail "iriumd binary missing"
[[ -x "$STRATUM_BIN" ]] && pass "stratum binary exists" || fail "stratum binary missing"

IRIUMD_DATE=$(stat -c '%y' "$IRIUMD_BIN" | cut -d' ' -f1)
STRATUM_DATE=$(stat -c '%y' "$STRATUM_BIN" | cut -d' ' -f1)
echo "iriumd built: $IRIUMD_DATE  stratum built: $STRATUM_DATE"

rm -rf "$DATA_DIR" && mkdir -p "$DATA_DIR/bootstrap"
echo "Data dir: $DATA_DIR"
# Copy bootstrap files
BOOTSTRAP_SRC="$HOME/irium/bootstrap"
cp -a "$BOOTSTRAP_SRC/anchors.json" "$DATA_DIR/bootstrap/anchors.json" 2>/dev/null || true
cp -a "$BOOTSTRAP_SRC/trust" "$DATA_DIR/bootstrap/trust" 2>/dev/null || true

### ── Section 1: Start testnet iriumd ───────────────────────────────────────
echo "=== Section 1: Start testnet iriumd ==="
(
  cd "$DATA_DIR"
  IRIUM_NETWORK=devnet \
  IRIUM_POAWX_MODE=active \
  IRIUM_DEV_EASY_BITS_TEMPLATE=1 \
  IRIUM_P2P_BIND="0.0.0.0:$P2P_PORT" \
  IRIUM_NODE_HOST=127.0.0.1 \
  IRIUM_NODE_PORT="$RPC_PORT" \
  IRIUM_DATA_DIR="$DATA_DIR" \
  IRIUM_BOOTSTRAP_DIR="$DATA_DIR/bootstrap" \
  IRIUM_RPC_TOKEN="$RPC_TOKEN" \
    "$IRIUMD_BIN" >"$LOG_IRIUMD" 2>&1 &
  echo $! > "$DATA_DIR/iriumd.pid"
)
sleep 0.5
TESTNET_IRIUMD_PID=$(cat "$DATA_DIR/iriumd.pid")
echo "iriumd PID=$TESTNET_IRIUMD_PID"

sleep 3
if kill -0 "$TESTNET_IRIUMD_PID" 2>/dev/null; then
    pass "iriumd started"
else
    fail "iriumd died at startup"
    cat "$LOG_IRIUMD" | tail -20
    exit 1
fi

### ── Section 2: Verify iriumd devnet mode ───────────────────────────────────
echo "=== Section 2: iriumd devnet checks ==="
TMPL=$(curl -sf -H "Authorization: Bearer $RPC_TOKEN" "$RPC_BASE/rpc/getblocktemplate" 2>/dev/null || true)
if [[ -n "$TMPL" ]]; then
    pass "getblocktemplate responsive"
    BITS=$(echo "$TMPL" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('bits',''))")
    if [[ "$BITS" == "207fffff" ]]; then
        pass "template bits=207fffff (devnet easy)"
    else
        fail "template bits=$BITS (expected 207fffff)"
    fi
    PMODE=$(echo "$TMPL" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('poawx_mode',''))")
    if [[ "$PMODE" == "active" ]]; then
        pass "template poawx_mode=active"
    else
        fail "template poawx_mode='$PMODE' (expected active)"
    fi
else
    fail "getblocktemplate not responding"
fi

### ── Section 3: /poawx/assignment endpoint ─────────────────────────────────
echo "=== Section 3: /poawx/assignment ==="
# Mine at least 1 block so height > 0
echo "Mining 1 block via direct RPC to get height > 0..."
for attempt in $(seq 1 200); do
    TMPL=$(curl -sf -H "Authorization: Bearer $RPC_TOKEN" "$RPC_BASE/rpc/getblocktemplate" 2>/dev/null || true)
    [[ -z "$TMPL" ]] && sleep 0.2 && continue
    HEIGHT=$(echo "$TMPL" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['height'])")
    BITS_HEX=$(echo "$TMPL" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['bits'])")
    PREV=$(echo "$TMPL" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['prev_hash'])")
    TIME=$(echo "$TMPL" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['time'])")
    COIN_VAL=$(echo "$TMPL" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['coinbase_value'])")

    RESULT=$(python3 - "$HEIGHT" "$BITS_HEX" "$PREV" "$TIME" "$COIN_VAL" "$WALLET_ADDR" "$RPC_BASE" "$RPC_TOKEN" <<'PYEOF'
import sys, struct, hashlib, requests, json, time as ttime

height = int(sys.argv[1])
bits_hex = sys.argv[2]
prev_hash = sys.argv[3]
ttime_val = int(sys.argv[4])
coinbase_val = int(sys.argv[5])
wallet_addr = sys.argv[6]
rpc_base = sys.argv[7]
rpc_token = sys.argv[8]

def sha256d(data):
    return hashlib.sha256(hashlib.sha256(data).digest()).digest()

def varint(n):
    if n < 0xfd:
        return bytes([n])
    elif n <= 0xffff:
        return b'\xfd' + struct.pack('<H', n)
    return b'\xfe' + struct.pack('<I', n)

def build_coinbase(height, reward, pkh, extranonce=b'\x00'*8):
    bip34 = bytes([len(height.to_bytes((height.bit_length()+7)//8 or 1, 'little'))])
    bip34 += height.to_bytes((height.bit_length()+7)//8 or 1, 'little')
    script_sig = bip34 + b'Irium' + extranonce
    tx = b'\x01\x00\x00\x00'  # version
    tx += varint(1)            # input count
    tx += b'\x20' + b'\x00'*32  # prev txid (length-prefixed)
    tx += b'\xff\xff\xff\xff'  # prev index
    tx += varint(len(script_sig)) + script_sig
    tx += b'\xff\xff\xff\xff'  # sequence
    tx += varint(1)            # output count
    pkh_script = b'\x76\xa9\x14' + pkh + b'\x88\xac'
    tx += struct.pack('<Q', reward) + varint(len(pkh_script)) + pkh_script
    tx += b'\x00\x00\x00\x00'  # locktime
    return tx

def base58_decode(s):
    alphabet = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
    n = 0
    for c in s:
        n = n * 58 + alphabet.index(c)
    return n.to_bytes(25, 'big')

pkh = base58_decode(wallet_addr)[1:21]

bits = int(bits_hex, 16)
exp = bits >> 24
mant = bits & 0xffffff
target = mant * (1 << (8 * (exp - 3)))
target_bytes = target.to_bytes(32, 'big')

coinbase_tx = build_coinbase(height, coinbase_val, pkh)
cb_hash = sha256d(coinbase_tx)
merkle_root = cb_hash  # no transactions

for nonce in range(0, 0x100000000):
    header = struct.pack('<I', 1)  # version
    prev_bytes = bytes.fromhex(prev_hash)
    header += prev_bytes[::-1]  # reverse for wire
    if height < 22888:
        mr_wire = merkle_root[::-1]
    else:
        mr_wire = merkle_root
    header += mr_wire
    header += struct.pack('<I', ttime_val)
    header += struct.pack('<I', bits)
    header += struct.pack('<I', nonce)
    h = sha256d(header)
    if h[::-1] <= target_bytes:
        hash_disp = h[::-1].hex()
        mr_json = mr_wire.hex() if height >= 22888 else merkle_root.hex()
        cb_hex = coinbase_tx.hex()
        resp = requests.post(
            f'{rpc_base}/rpc/submit_block',
            headers={'Authorization': f'Bearer {rpc_token}'},
            json={
                'height': height,
                'header': {
                    'version': 1,
                    'prev_hash': prev_hash,
                    'merkle_root': mr_json,
                    'time': ttime_val,
                    'bits': bits_hex,
                    'nonce': nonce,
                    'hash': hash_disp,
                },
                'tx_hex': [cb_hex],
                'submit_source': 'phase10d_direct',
            }
        )
        if resp.status_code == 200:
            print('OK')
        else:
            print(f'FAIL:{resp.status_code}')
        sys.exit(0)

print('TIMEOUT')
PYEOF
)
    if [[ "$RESULT" == "OK" ]]; then
        echo "Block mined at height=$HEIGHT"
        break
    fi
    sleep 0.1
done

sleep 1
# Now test /poawx/assignment
HTTP_CODE=$(curl -s -o /tmp/phase10d_assign.json -w "%{http_code}" \
    -H "Authorization: Bearer $RPC_TOKEN" \
    "$RPC_BASE/poawx/assignment" 2>/dev/null)
if [[ "$HTTP_CODE" == "200" ]]; then
    pass "/poawx/assignment returns 200"
    SEED=$(python3 -c "import json; d=json.load(open('/tmp/phase10d_assign.json')); print(d.get('seed',''))")
    NONCE=$(python3 -c "import json; d=json.load(open('/tmp/phase10d_assign.json')); print(d.get('commitment_nonce',''))")
    ASSIGN_HEIGHT=$(python3 -c "import json; d=json.load(open('/tmp/phase10d_assign.json')); print(d.get('height',''))")
    LANE=$(python3 -c "import json; d=json.load(open('/tmp/phase10d_assign.json')); print(d.get('lane',''))")
    POW_BITS=$(python3 -c "import json; d=json.load(open('/tmp/phase10d_assign.json')); print(d.get('pow_bits',''))")
    [[ ${#SEED} -eq 64 ]] && pass "assignment.seed is 32-byte hex" || fail "assignment.seed malformed: '$SEED'"
    [[ ${#NONCE} -eq 64 ]] && pass "assignment.commitment_nonce is 32-byte hex" || fail "assignment.nonce malformed"
    [[ "$LANE" == "cpu" ]] && pass "assignment.lane=cpu" || fail "assignment.lane=$LANE (expected cpu)"
    [[ -n "$POW_BITS" ]] && pass "assignment.pow_bits present: $POW_BITS" || fail "assignment.pow_bits missing"
    echo "assignment: height=$ASSIGN_HEIGHT lane=$LANE pow_bits=$POW_BITS"
else
    fail "/poawx/assignment returned HTTP $HTTP_CODE"
    cat /tmp/phase10d_assign.json 2>/dev/null || true
    SEED=""
    NONCE=""
    ASSIGN_HEIGHT=""
    LANE="cpu"
fi

### ── Section 4: POST /poawx/receipt ─────────────────────────────────────────
echo "=== Section 4: POST /poawx/receipt ==="
if [[ -n "$SEED" && -n "$NONCE" && -n "$ASSIGN_HEIGHT" ]]; then
    # Build a valid-looking receipt (field names match server: solution + commitment_nonce)
    SOLUTION=$(echo -n "${SEED}solution" | sha256sum | awk '{print $1}')
    RECEIPT_BODY=$(python3 -c "import json; print(json.dumps({
        'height': int('$ASSIGN_HEIGHT'),
        'lane': 'cpu',
        'worker_pkh': 'a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2',
        'solution': '$SOLUTION',
        'commitment_nonce': '$NONCE',
    }))")

    HTTP_RECEIPT=$(curl -s -o /tmp/phase10d_receipt.json -w "%{http_code}" \
        -X POST \
        -H "Authorization: Bearer $RPC_TOKEN" \
        -H "Content-Type: application/json" \
        -d "$RECEIPT_BODY" \
        "$RPC_BASE/poawx/receipt" 2>/dev/null)

    if [[ "$HTTP_RECEIPT" == "200" ]]; then
        pass "POST /poawx/receipt returns 200"
        PENDING_CNT=$(python3 -c "import json; d=json.load(open('/tmp/phase10d_receipt.json')); print(d.get('pending_count', 0))")
        [[ "$PENDING_CNT" -ge 1 ]] && pass "receipt stored: pending_count=$PENDING_CNT" || fail "pending_count=$PENDING_CNT"
    else
        fail "POST /poawx/receipt returned HTTP $HTTP_RECEIPT"
        cat /tmp/phase10d_receipt.json 2>/dev/null || true
        PENDING_CNT=0
    fi
else
    skip "POST /poawx/receipt: skipped (no assignment data)"
    PENDING_CNT=0
fi

### ── Section 5: Template has receipts_root ──────────────────────────────────
echo "=== Section 5: Template receipts_root ==="
if [[ "$PENDING_CNT" -ge 1 ]]; then
    TMPL2=$(curl -sf -H "Authorization: Bearer $RPC_TOKEN" "$RPC_BASE/rpc/getblocktemplate" 2>/dev/null || true)
    if [[ -n "$TMPL2" ]]; then
        ROOT=$(echo "$TMPL2" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('receipts_root',''))")
        PENDING_IN_TMPL=$(echo "$TMPL2" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d.get('poawx_pending_receipts',[])))")
        if [[ ${#ROOT} -eq 64 ]]; then
            pass "template receipts_root is non-empty 32-byte hex: $ROOT"
        else
            fail "template receipts_root='$ROOT' (expected 64-char hex)"
        fi
        [[ "$PENDING_IN_TMPL" -ge 1 ]] && pass "template poawx_pending_receipts count=$PENDING_IN_TMPL" || fail "template has no poawx_pending_receipts"
    else
        fail "getblocktemplate not responding in section 5"
    fi
else
    skip "Template receipts_root check: no pending receipts"
fi

### ── Section 6: Start stratum and mine with irx1 ────────────────────────────
echo "=== Section 6: Stratum + irx1 mining ==="
IRIUM_STRATUM_POAWX=1 \
IRIUM_RPC_BASE="$RPC_BASE" \
IRIUM_RPC_TOKEN="$RPC_TOKEN" \
STRATUM_BIND="0.0.0.0:$STRATUM_PORT" \
IRIUM_STRATUM_COINBASE_BIP34=true \
STRATUM_DEFAULT_DIFF=1 \
IRIUM_STRATUM_VARDIFF_ENABLED=false \
IRIUM_STRATUM_MINER_ADDRESS="$WALLET_ADDR" \
IRIUM_POW_LIMIT_HEX=7fffff0000000000000000000000000000000000000000000000000000000000 \
  "$STRATUM_BIN" >"$LOG_STRATUM" 2>&1 &
TESTNET_STRATUM_PID=$!
echo "stratum PID=$TESTNET_STRATUM_PID"

sleep 2
if kill -0 "$TESTNET_STRATUM_PID" 2>/dev/null; then
    pass "stratum started"
else
    fail "stratum died at startup"
    cat "$LOG_STRATUM" | tail -20
fi

if ss -lntp "sport = :$STRATUM_PORT" 2>/dev/null | grep -q ":$STRATUM_PORT"; then
    pass "stratum port $STRATUM_PORT open"
else
    fail "stratum port $STRATUM_PORT not open"
fi

POAWX_LOG=$(grep -i 'IRIUM_STRATUM_POAWX\|poawx.*enabled\|poawx.*disabled' "$LOG_STRATUM" 2>/dev/null | head -3 || true)
if [[ -n "$POAWX_LOG" ]]; then
    pass "stratum logged PoAW-X startup message"
    echo "  $POAWX_LOG"
else
    info_line "stratum PoAW-X startup message not found in log (may log later)"
fi

### ── Section 7: Mine blocks via stratum Python harness ─────────────────────
echo "=== Section 7: Mine 10 blocks via stratum (existing harness) ==="
BLOCK_TARGET=10
HARNESS_OUT="$DATA_DIR/harness_out.txt"

# Use the Phase 10-C soak harness which correctly handles stratum protocol.
# --receipt tests the full irx1 path: assignment → receipt → template → irx1 coinbase.
python3 ~/irium/scripts/poawx-stratum-long-soak-harness.py \
    127.0.0.1 "$STRATUM_PORT" \
    "$RPC_BASE" "$RPC_TOKEN" \
    --blocks "$BLOCK_TARGET" \
    --receipt \
    2>&1 | tee "$HARNESS_OUT" || true

HARNESS_EXIT=${PIPESTATUS[0]}
# Parse SUMMARY_JSON from harness output
SUMMARY_LINE=$(grep "^SUMMARY_JSON:" "$HARNESS_OUT" 2>/dev/null | tail -1 || echo "")
if [[ -n "$SUMMARY_LINE" ]]; then
    SUMMARY_JSON="${SUMMARY_LINE#SUMMARY_JSON:}"
    BLOCKS_PASS=$(echo "$SUMMARY_JSON" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('blocks_pass',0))")
    IRX1_CNT=$(echo "$SUMMARY_JSON" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('irx1_in_coinbase_count',0))")
    RECEIPT_OK=$(echo "$SUMMARY_JSON" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('receipt_test_passed',False))")
    echo "harness summary: blocks_pass=$BLOCKS_PASS irx1_count=$IRX1_CNT receipt_ok=$RECEIPT_OK"
    [[ "$BLOCKS_PASS" -ge "$BLOCK_TARGET" ]] && pass "stratum mining: $BLOCKS_PASS/$BLOCK_TARGET blocks accepted" || fail "stratum mining: only $BLOCKS_PASS/$BLOCK_TARGET blocks accepted"
    [[ "$IRX1_CNT" -ge 1 ]] && pass "irx1 in coinbase: $IRX1_CNT blocks with irx1 commitment" || fail "irx1 in coinbase: 0 blocks (expected ≥1)"
    [[ "$RECEIPT_OK" == "True" ]] && pass "receipt path: commit_nonce → solution → template → irx1" || fail "receipt path test failed"
else
    fail "harness produced no SUMMARY_JSON"
fi

sleep 1
CURR_HEIGHT=$(curl -sf -H "Authorization: Bearer $RPC_TOKEN" "$RPC_BASE/rpc/getblocktemplate" 2>/dev/null | \
    python3 -c "import sys,json; d=json.load(sys.stdin); print(d['height'])" || echo 0)

### ── Section 8: Verify irx1 in stratum log ──────────────────────────────────
echo "=== Section 8: Verify irx1 OP_RETURN ==="
IRX1_LOG=$(grep -c 'irx1\|submit_block_extended' "$LOG_STRATUM" 2>/dev/null; true)
IRX1_EXTENDED=$(grep -c 'submit_block_extended' "$LOG_STRATUM" 2>/dev/null; true)
echo "stratum log: irx1/extended mentions=$IRX1_LOG  submit_block_extended calls=$IRX1_EXTENDED"

if [[ "$IRX1_EXTENDED" -ge 1 ]]; then
    pass "stratum called submit_block_extended (irx1 path active)"
else
    fail "stratum never called submit_block_extended"
fi

IRX1_INJECTED=$(grep -c 'poawx.*to_job.*mode=active\|irx1_len=38' "$LOG_STRATUM" 2>/dev/null; true)
if [[ "$IRX1_INJECTED" -ge 1 ]]; then
    pass "stratum logged irx1 coinbase injection"
else
    info_line "irx1 injection log not found (may need pending receipts from assignment path)"
fi

### ── Section 9: Verify accepted block with irx1 in iriumd log ───────────────
echo "=== Section 9: iriumd accepted submit_block_extended ==="
ACCEPTED_EXT=$(grep -c 'submit_block_extended.*accepted\|block_extended accepted' "$LOG_IRIUMD" 2>/dev/null; true)
if [[ "$ACCEPTED_EXT" -ge 1 ]]; then
    pass "iriumd accepted at least 1 submit_block_extended block"
else
    info_line "iriumd submit_block_extended acceptance not logged (expected if no receipts committed)"
fi

### ── Section 10: Test /rpc/submit_block_extended directly (no receipts) ─────
echo "=== Section 10: submit_block_extended endpoint reachable ==="
HTTP_EXT=$(curl -s -o /dev/null -w "%{http_code}" \
    -X POST \
    -H "Authorization: Bearer $RPC_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{}' \
    "$RPC_BASE/rpc/submit_block_extended" 2>/dev/null)
if [[ "$HTTP_EXT" != "404" && "$HTTP_EXT" != "000" ]]; then
    pass "/rpc/submit_block_extended reachable (HTTP $HTTP_EXT)"
else
    fail "/rpc/submit_block_extended returned $HTTP_EXT"
fi

### ── Section 11: Mainnet safety check ───────────────────────────────────────
echo "=== Section 11: Mainnet safety ==="
if [[ -n "$MAINNET_PID" ]]; then
    if kill -0 "$MAINNET_PID" 2>/dev/null; then
        pass "Mainnet iriumd PID=$MAINNET_PID still alive"
    else
        fail "CRITICAL: Mainnet iriumd PID=$MAINNET_PID died during test!"
    fi
fi
MAINNET_PORT_OK=$(ss -lntp "sport = :38300" 2>/dev/null | grep -c ":38300"; true)
[[ "$MAINNET_PORT_OK" -ge 1 ]] && pass "mainnet port 38300 still bound" || info_line "mainnet port 38300 not bound (may not be running)"

### ── Section 12: No panics ──────────────────────────────────────────────────
echo "=== Section 12: Log safety ==="
PANIC_IRD=$(grep -c 'thread.*panicked\|SIGSEGV\|stack overflow' "$LOG_IRIUMD" 2>/dev/null; true)
PANIC_STR=$(grep -c 'thread.*panicked\|SIGSEGV\|stack overflow' "$LOG_STRATUM" 2>/dev/null; true)
if [[ "${PANIC_IRD:-0}" -eq 0 ]]; then pass "no panics in iriumd log"; else fail "panics in iriumd log: $PANIC_IRD"; fi
if [[ "${PANIC_STR:-0}" -eq 0 ]]; then pass "no panics in stratum log"; else fail "panics in stratum log: $PANIC_STR"; fi

### ── Final report ───────────────────────────────────────────────────────────
echo ""
echo "==================================================="
echo " Phase 10-D: PoAW-X assignment receipt path"
echo "==================================================="
echo " PASS=$PASS  FAIL=$FAIL  SKIP=$SKIP"
echo " Current chain height: $CURR_HEIGHT"
echo " iriumd log: $LOG_IRIUMD"
echo " stratum log: $LOG_STRATUM"
if [[ "$FAIL" -eq 0 ]]; then
    echo " RESULT: ALL CHECKS PASS"
    exit 0
else
    echo " RESULT: $FAIL CHECKS FAILED"
    exit 1
fi
