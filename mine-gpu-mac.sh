#!/usr/bin/env bash
# mine-gpu-mac.sh — friendly entry point for macOS GPU miners.
#
# Drop next to irium-miner-gpu and run: ./mine-gpu-mac.sh
# First run handles the macOS gatekeeper quarantine attribute, prompts
# for your Irium wallet address, and saves it to mine-config.txt for
# next time. Connects to the official Irium pool at
# pool.iriumlabs.org:3335 in SOLO payout mode. Auto-restarts on crash
# with a 5s cool-down. Ctrl+C to stop.
#
# If you double-clicked and macOS blocked the script with "cannot be
# opened because Apple cannot check it for malicious software", run
# this in Terminal first:
#   xattr -d com.apple.quarantine mine-gpu-mac.sh irium-miner-gpu
# OR right-click mine-gpu-mac.sh in Finder → Open → Open anyway.

set -u
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MINER_BIN="${SCRIPT_DIR}/irium-miner-gpu"
CONFIG_FILE="${SCRIPT_DIR}/mine-config.txt"
POOL_URL="stratum+tcp://pool.iriumlabs.org:3335"

# Strip macOS quarantine attribute set by Gatekeeper on downloaded files.
# Without this the unsigned irium-miner-gpu binary refuses to launch and
# the user has to manually right-click → Open in Finder.
if [ -f "${MINER_BIN}" ]; then
    xattr -d com.apple.quarantine "${MINER_BIN}" 2>/dev/null || true
    chmod +x "${MINER_BIN}" 2>/dev/null || true
fi

if [ ! -f "${MINER_BIN}" ]; then
    echo
    echo "ERROR: irium-miner-gpu not found in this folder."
    echo "       expected at: ${MINER_BIN}"
    echo
    exit 1
fi

if [ ! -x "${MINER_BIN}" ]; then
    echo
    echo "ERROR: irium-miner-gpu is not executable."
    echo "       run: chmod +x \"${MINER_BIN}\""
    echo "       then re-run this script."
    echo
    exit 1
fi

WALLET=""
if [ -f "${CONFIG_FILE}" ]; then
    WALLET="$(head -n 1 "${CONFIG_FILE}" | tr -d '[:space:]')"
fi

if [ -z "${WALLET}" ]; then
    echo
    echo "----------------------------------------------------------------"
    echo "                Welcome to Irium GPU Mining (macOS)"
    echo "----------------------------------------------------------------"
    echo
    echo "You will mine SHA-256d shares against the Irium official pool"
    echo "(pool.iriumlabs.org). When one of your shares meets the network"
    echo "target, the FULL block reward goes to YOUR Irium address."
    echo "There is no pool fee."
    echo
    echo "Note: macOS may show a Gatekeeper warning for unsigned binaries."
    echo "If you see one, this script tried to remove the quarantine"
    echo "attribute automatically. If it still fails, run:"
    echo "    xattr -d com.apple.quarantine irium-miner-gpu"
    echo "or right-click the binary in Finder → Open → Open Anyway."
    echo
    read -rp "Enter your Irium wallet address (P or Q prefix): " WALLET
    WALLET="$(printf '%s' "${WALLET}" | tr -d '[:space:]')"
    if [ -z "${WALLET}" ]; then
        echo "No address entered. Aborting."
        exit 1
    fi
    printf '%s\n' "${WALLET}" > "${CONFIG_FILE}"
    echo "Saved to ${CONFIG_FILE} — delete it to re-enter the address."
fi

echo
echo "----------------------------------------------------------------"
echo "                  Starting Irium GPU Miner (macOS)"
echo "----------------------------------------------------------------"
echo "  Pool:    ${POOL_URL}"
echo "  Wallet:  ${WALLET}"
echo "  Worker:  ${WALLET}.rig1"
echo
echo "  Auto-restart on crash. Press Ctrl+C to stop."
echo "----------------------------------------------------------------"
echo

while true; do
    echo "[$(date +%T)] launching irium-miner-gpu..."
    "${MINER_BIN}" --wallet "${WALLET}" --pool "${POOL_URL}" --intensity 50 || true
    echo "[$(date +%T)] miner exited. restarting in 5s (Ctrl+C to stop)..."
    sleep 5
done
