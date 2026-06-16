#!/usr/bin/env bash
# poawx-delegation-validate.sh — validate a poawx-delegation.json payload produced by
#   `irium-wallet poawx-register --emit-only` BEFORE the operator POSTs it to the
#   loopback-only /poawx/delegation endpoint.
#
# SAFETY: read-only. No network calls, no services started, no sudo/firewall/systemd,
# binds nothing. It only inspects a local JSON file and prints the loopback curl command
# for you to run yourself. It NEVER posts anything and NEVER prints secrets.
#
# Usage:
#   poawx-delegation-validate.sh <poawx-delegation.json> [--port <delegation-port>]
#   poawx-delegation-validate.sh --help
#
# Exit codes: 0 = valid payload, 1 = invalid/usage error.
set -euo pipefail

usage() {
  sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
}

[ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ] && { usage; exit 0; }
[ $# -lt 1 ] && { echo "error: missing <poawx-delegation.json>" >&2; usage; exit 1; }

FILE="$1"; shift
PORT="<delegation-port>"
while [ $# -gt 0 ]; do
  case "$1" in
    --port) PORT="${2:?--port needs a value}"; shift 2;;
    *) echo "error: unknown arg $1" >&2; exit 1;;
  esac
done

[ -f "$FILE" ] || { echo "error: file not found: $FILE" >&2; exit 1; }

# All validation is done in python3 (read-only). No secrets are printed.
python3 - "$FILE" <<'PY'
import json, sys, re
path = sys.argv[1]
try:
    raw = open(path, "r", encoding="utf-8").read()
    obj = json.loads(raw)
except Exception as e:
    print(f"FAIL: not valid JSON: {e}"); sys.exit(1)

if not isinstance(obj, dict):
    print("FAIL: payload must be a JSON object"); sys.exit(1)

# Exactly the three public fields the pool endpoint reads — nothing else may leak.
keys = sorted(obj.keys())
if keys != ["delegation", "miner_pkh", "worker"]:
    print(f"FAIL: unexpected keys {keys}; expected exactly delegation,miner_pkh,worker")
    sys.exit(1)

deleg = obj["delegation"]; pkh = obj["miner_pkh"]; worker = obj["worker"]
hexre = re.compile(r"^[0-9a-fA-F]+$")
# Canonical Delegation is 226 bytes => 452 hex chars.
if not (isinstance(deleg, str) and hexre.match(deleg) and len(deleg) == 452):
    print(f"FAIL: delegation must be 452 hex chars (226 bytes); got len={len(deleg) if isinstance(deleg,str) else 'n/a'}")
    sys.exit(1)
# miner_pkh is HASH160 => 20 bytes => 40 hex chars.
if not (isinstance(pkh, str) and hexre.match(pkh) and len(pkh) == 40):
    print(f"FAIL: miner_pkh must be 40 hex chars (20 bytes); got len={len(pkh) if isinstance(pkh,str) else 'n/a'}")
    sys.exit(1)
if not isinstance(worker, str) or worker == "":
    print("FAIL: worker must be a non-empty string"); sys.exit(1)

# Defensive secret scan: the payload must NOT contain anything that looks like a
# private key, seed, or mnemonic field. (emit-only never emits these, but verify.)
low = raw.lower()
for bad in ("privkey", "private_key", "secret", "seed", "mnemonic"):
    if bad in low:
        print(f"FAIL: payload contains forbidden token '{bad}' — do NOT post this file")
        sys.exit(1)

print(f"PASS: valid delegation payload (worker={worker}, miner_pkh={pkh})")
PY

echo
echo "Next (operator, loopback only — run yourself; this script does NOT post):"
echo "  curl -sS -X POST http://127.0.0.1:${PORT}/poawx/delegation \\"
echo "    -H 'content-type: application/json' --data @${FILE}"
