#!/usr/bin/env bash
# poawx-pool-identity-package.sh — print the PUBLIC pool identity package an operator sends
# out-of-band to a trusted miner so the miner can run `poawx-register --emit-only`.
#
# SAFETY: read-only. Only does a loopback HTTP GET to the pool's delegation endpoint.
# No sudo, no firewall, no services started, binds nothing, prints no secrets (the pool
# identity is public: pool_pubkey, network_id, fee_bps, domain).
#
# Usage:  poawx-pool-identity-package.sh --port <delegation-port> [--host 127.0.0.1] \
#                                        [--stratum-host <host:port>] [--worker w1] [--expiry <N>]
#         poawx-pool-identity-package.sh --help
set -euo pipefail
HOST=127.0.0.1; PORT=""; STRATUM="<pool-host>:<stratum-port>"; WORKER="w1"; EXPIRY="<future-height>"
while [ $# -gt 0 ]; do case "$1" in
  --help|-h) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0;;
  --port) PORT="${2:?}"; shift 2;;
  --host) HOST="${2:?}"; shift 2;;
  --stratum-host) STRATUM="${2:?}"; shift 2;;
  --worker) WORKER="${2:?}"; shift 2;;
  --expiry) EXPIRY="${2:?}"; shift 2;;
  *) echo "unknown arg $1" >&2; exit 1;; esac; done
[ -n "$PORT" ] || { echo "error: --port <delegation-port> required" >&2; exit 1; }
case "$HOST" in 127.0.0.1|localhost|::1) ;; *) echo "error: identity GET must be loopback (got $HOST)" >&2; exit 1;; esac

ID=$(curl -sS --max-time 5 "http://${HOST}:${PORT}/poawx/pool-identity") || { echo "error: could not reach loopback delegation endpoint" >&2; exit 1; }
PPK=$(printf '%s' "$ID" | python3 -c 'import sys,json;print(json.load(sys.stdin)["pool_pubkey"])')
NID=$(printf '%s' "$ID" | python3 -c 'import sys,json;print(json.load(sys.stdin)["network_id"])')
FEE=$(printf '%s' "$ID" | python3 -c 'import sys,json;print(json.load(sys.stdin).get("fee_bps",0))')
[ "$FEE" = "0" ] || { echo "REFUSING: pool reports fee_bps=$FEE; official pool must be 0%" >&2; exit 1; }

cat <<EOF
=== PoAW-X trusted-miner identity package (PUBLIC — safe to send out-of-band) ===
pool_pubkey : $PPK
network_id  : $NID   (1=testnet, 2=devnet)
fee_bps     : 0      (official pool is 0%)
stratum     : $STRATUM   (operator-provided; source-restricted to your IP)

Miner runs locally (private key never leaves the wallet):
  irium-wallet poawx-register --emit-only \\
    --pool-pubkey $PPK --network-id $NID \\
    --addr <YOUR-TESTNET-ADDRESS> --worker $WORKER --expiry-height $EXPIRY --fee-bps 0 \\
    > poawx-delegation.json
Then send ONLY poawx-delegation.json back (never your seed/private key).

Miner mines (stock cpuminer, unchanged):
  minerd -a sha256d -o stratum+tcp://$STRATUM -u <YOUR-TESTNET-ADDRESS>.$WORKER -p x -t 3
EOF
