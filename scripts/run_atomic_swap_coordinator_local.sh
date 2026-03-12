#!/usr/bin/env bash
set -euo pipefail

source "$HOME/.cargo/env" 2>/dev/null || true

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COORD_DIR="$ROOT/tools/atomic-swap-coordinator"

export COORDINATOR_BIND="${COORDINATOR_BIND:-0.0.0.0:8088}"
export COORDINATOR_DB="${COORDINATOR_DB:-$COORD_DIR/swap-coordinator.db}"
export COORDINATOR_OPERATOR_TOKEN="${COORDINATOR_OPERATOR_TOKEN:-pilot-operator-change-me}"
export COORDINATOR_INVITE_CODES="${COORDINATOR_INVITE_CODES:-pilot-invite-1,pilot-invite-2}"
export COORDINATOR_EXPECTED_AMOUNT_SATS="${COORDINATOR_EXPECTED_AMOUNT_SATS:-100000}"
export COORDINATOR_BTC_MIN_CONFIRMATIONS="${COORDINATOR_BTC_MIN_CONFIRMATIONS:-1}"
export COORDINATOR_AUTO_DETECT_BTC="${COORDINATOR_AUTO_DETECT_BTC:-true}"
export COORDINATOR_AUTO_CREATE_IRIUM_HTLC="${COORDINATOR_AUTO_CREATE_IRIUM_HTLC:-false}"

cd "$COORD_DIR"
cargo run --release
