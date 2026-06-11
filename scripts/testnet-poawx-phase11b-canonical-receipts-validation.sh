#!/usr/bin/env bash
# Phase 11-B regression soak: canonical receipts_root + full solution validation.
#
# This wrapper delegates to the self-contained Python soak script which:
#   - Spawns isolated testnet iriumd (port 39511) and stratum (port 39512)
#   - Mines a warmup block to advance past genesis
#   - Runs T1-T6 (receipt acceptance, rejection, canonical root, SBE)
#   - Restarts stratum so its first job includes the pending receipts
#   - Runs T7 (10-block harness: submit_block_extended path + irx1)
#   - Verifies mainnet PIDs/ports are untouched before and after
#   - Cleans up all testnet processes and data on exit
#
# Usage (from repo root):
#   bash scripts/testnet-poawx-phase11b-canonical-receipts-validation.sh
#
# No env vars required — the Python script generates a fresh RPC token each run
# and uses ~/poawx-phase11b-soak as an isolated data directory.
#
# Safety properties:
#   - Never touches mainnet iriumd (port 38300), stratum (port 3333), or ~/.irium
#   - DATA_DIR is ~/poawx-phase11b-soak (under $HOME, isolated from ~/.irium)
#   - All testnet processes are killed and data dir removed on exit
#   - Mainnet PIDs checked at start and end; fail if changed
#   - No secrets committed: token is generated fresh via secrets.token_hex(16)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOAK_PY="${SCRIPT_DIR}/poawx-phase11b-canonical-receipts-validation.py"

if [[ ! -f "${SOAK_PY}" ]]; then
    echo "[ERROR] soak script not found: ${SOAK_PY}" >&2
    exit 1
fi

exec python3 "${SOAK_PY}" "$@"
