#!/usr/bin/env bash
set -euo pipefail

VERSION="1.0.0"

CMD="diagnose"
DO_FIX=0

# Define paths early (used by default var expansion)
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"

RPC_URL="${IRIUM_RPC_URL:-https://127.0.0.1:38300}"
RPC_TOKEN="${IRIUM_RPC_TOKEN:-}"
SHIM_RPC_URL="${IRIUM_SHIM_RPC_URL:-http://127.0.0.1:8332}"
EXPLORER_STATUS_URL="${IRIUM_EXPLORER_STATUS_URL:-}"
DATA_DIR="${IRIUM_DATA_DIR:-$HOME/.irium}"
SHIM_FILE="${IRIUM_SHIM_FILE:-/opt/irium-pool/shim/shim_async.py}"
SHIM_TEMPLATE_FILE="${IRIUM_SHIM_TEMPLATE_FILE:-$SCRIPT_DIR/pool/shim_async.py}"

RPC_DRIFT_MAX="${IRIUM_TEMPLATE_HEIGHT_DRIFT_MAX:-2}"

have() { command -v "$1" >/dev/null 2>&1; }

need_bin() {
  local b="$1"
  if ! have "$b"; then
    echo "[FATAL] missing required binary: $b" >&2
    exit 2
  fi
}

say() { printf "%s\n" "$*"; }
section() { printf "\n========== %s ==========%s" "$*" "\n"; }
warn() { printf "[WARN] %s\n" "$*" >&2; }
info() { printf "[INFO] %s\n" "$*"; }

usage() {
  cat <<USAGE
Usage:
  $0 diagnose [--rpc-url URL] [--rpc-token TOKEN] [--shim-rpc-url URL] [--explorer-status-url URL]
  $0 fix      [--rpc-url URL] [--rpc-token TOKEN] [--shim-rpc-url URL] [--explorer-status-url URL]

Environment:
  IRIUM_RPC_URL                 default: https://127.0.0.1:38300
  IRIUM_RPC_TOKEN               bearer token for iriumd RPC
  IRIUM_SHIM_RPC_URL            default: http://127.0.0.1:8332
  IRIUM_EXPLORER_STATUS_URL     optional explorer status endpoint for cross-check
  IRIUM_DATA_DIR                default: ~/.irium
  IRIUM_SHIM_FILE               default: /opt/irium-pool/shim/shim_async.py
  IRIUM_TEMPLATE_HEIGHT_DRIFT_MAX default: 2
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    diagnose) CMD="diagnose"; shift ;;
    fix|--fix) CMD="fix"; DO_FIX=1; shift ;;
    --rpc-url) RPC_URL="$2"; shift 2 ;;
    --rpc-token) RPC_TOKEN="$2"; shift 2 ;;
    --shim-rpc-url) SHIM_RPC_URL="$2"; shift 2 ;;
    --explorer-status-url) EXPLORER_STATUS_URL="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

if [[ "$CMD" == "fix" ]]; then
  DO_FIX=1
fi

need_bin bash
need_bin curl
need_bin jq
need_bin ss
need_bin ps
need_bin systemctl
need_bin journalctl

RPC_HOST_PORT="$(echo "$RPC_URL" | sed -E 's#^[a-zA-Z]+://##' | sed -E 's#/.*$##')"
RPC_PORT="$(echo "$RPC_HOST_PORT" | awk -F: '{print $2}')"
if [[ -z "$RPC_PORT" ]]; then
  RPC_PORT=38300
fi

AUTH_HEADER=()
if [[ -n "$RPC_TOKEN" ]]; then
  AUTH_HEADER=(-H "Authorization: Bearer $RPC_TOKEN")
fi

TMP_DIR="$(mktemp -d /tmp/pool-height-doctor.XXXXXX)"
trap 'rm -rf "$TMP_DIR"' EXIT

# Globals filled during diagnose
NODE_HEIGHT=""
NODE_BEST_HASH=""
NODE_TEMPLATE_HEIGHT=""
NODE_TEMPLATE_PREV=""
SHIM_HEIGHT=""
SHIM_TEMPLATE_HEIGHT=""
SHIM_TEMPLATE_PREV=""
SAMPLE_HEIGHT=""
NODE_SAMPLE_HASH=""
SHIM_SAMPLE_HASH=""
EXPLORER_HEIGHT=""
SHIM_RPC_RESPONDED=0

MULTI_IRIUMD=0
RPC_LISTENER_MISMATCH=0
SHIM_TEMPLATE_DRIFT=0
FORK_SUSPECT=0

run_curl_json() {
  local url="$1"
  curl -ksS --max-time 15 "${AUTH_HEADER[@]}" "$url"
}

rpc_call_node() {
  local method="$1"
  local params_json="${2:-[]}"
  local payload
  payload="$(jq -cn --arg m "$method" --argjson p "$params_json" '{jsonrpc:"2.0",id:1,method:$m,params:$p}')"
  curl -ksS --max-time 20 "${AUTH_HEADER[@]}" -H "Content-Type: application/json" -d "$payload" "$RPC_URL"
}

node_getblockcount() { rpc_call_node getblockcount '[]'; }
node_getblocktemplate() { rpc_call_node getblocktemplate '[{}]'; }
node_getbestblockhash() { rpc_call_node getbestblockhash '[]' || true; }
node_getblockhash_by_height() {
  local h="$1"
  rpc_call_node getblockhash "[$h]" | jq -r '.result // empty'
}

shim_rpc_call() {
  local method="$1"
  local params_json="${2:-[]}"
  local payload
  payload="$(jq -cn --arg m "$method" --argjson p "$params_json" '{jsonrpc:"2.0",id:1,method:$m,params:$p}')"
  curl -ksS --max-time 20 -H "Content-Type: application/json" -d "$payload" "$SHIM_RPC_URL" || return 1
}

abs_diff() {
  local a="$1" b="$2"
  if (( a >= b )); then
    echo $((a-b))
  else
    echo $((b-a))
  fi
}

install_dropins() {
  section "Installing systemd drop-ins"
  sudo mkdir -p /etc/systemd/system/iriumd.service.d
  sudo mkdir -p /etc/systemd/system/irium-pool-shim.service.d
  sudo mkdir -p /etc/systemd/system/irium-stratum.service.d

  sudo cp -f "$REPO_ROOT/systemd/pool/iriumd-single-instance.conf" /etc/systemd/system/iriumd.service.d/30-single-instance-guard.conf
  sudo cp -f "$REPO_ROOT/systemd/pool/irium-pool-shim.override.conf" /etc/systemd/system/irium-pool-shim.service.d/20-rpc-pin.conf
  sudo cp -f "$REPO_ROOT/systemd/pool/irium-stratum.override.conf" /etc/systemd/system/irium-stratum.service.d/20-rpc-pin.conf

  sudo systemctl daemon-reload
}

patch_live_shim() {
  section "Patching live pool shim"
  if [[ ! -f "$SHIM_FILE" ]]; then
    warn "shim file not found at $SHIM_FILE (skipping live patch)"
    return 0
  fi

  local ts backup
  ts="$(date +%s)"
  backup="${SHIM_FILE}.bak.${ts}"
  sudo cp -a "$SHIM_FILE" "$backup"
  info "shim backup: $backup"

  if [[ -f "$SHIM_TEMPLATE_FILE" ]]; then
    sudo cp -f "$SHIM_TEMPLATE_FILE" "$SHIM_FILE"
  else
    warn "shim template file not found: $SHIM_TEMPLATE_FILE"
    return 1
  fi
}


backup_chain_data() {
  section "Backing up chain data"
  local ts bdir
  ts="$(date +%Y%m%d-%H%M%S)"
  bdir="$DATA_DIR/backup-$ts"
  mkdir -p "$bdir"

  if [[ -d "$DATA_DIR/blocks" ]]; then
    cp -a "$DATA_DIR/blocks" "$bdir/blocks"
    info "backed up $DATA_DIR/blocks"
  fi
  if [[ -d "$DATA_DIR/chainstate" ]]; then
    cp -a "$DATA_DIR/chainstate" "$bdir/chainstate"
    info "backed up $DATA_DIR/chainstate"
  fi
  if [[ -f "$DATA_DIR/peer_reputation.json" ]]; then
    cp -a "$DATA_DIR/peer_reputation.json" "$bdir/peer_reputation.json"
    info "backed up $DATA_DIR/peer_reputation.json"
  fi

  echo "$bdir"
}

diagnose() {
  section "Environment"
  say "doctor_version=$VERSION"
  say "cmd=$CMD"
  say "rpc_url=$RPC_URL"
  say "shim_rpc_url=$SHIM_RPC_URL"
  say "data_dir=$DATA_DIR"
  if [[ -n "$EXPLORER_STATUS_URL" ]]; then
    say "explorer_status_url=$EXPLORER_STATUS_URL"
  else
    say "explorer_status_url=(not set)"
  fi

  section "A) iriumd process inventory"
  local procs pcount
  procs="$(pgrep -a iriumd || true)"
  if [[ -z "$procs" ]]; then
    warn "No running iriumd process found"
    pcount=0
  else
    say "$procs"
    pcount="$(printf "%s\n" "$procs" | wc -l | tr -d ' ')"
  fi
  if (( pcount > 1 )); then
    MULTI_IRIUMD=1
    warn "Multiple iriumd processes detected: $pcount"
  fi

  say "--datadir extraction:"
  if [[ -n "$procs" ]]; then
    while IFS= read -r line; do
      [[ -z "$line" ]] && continue
      pid="$(awk '{print $1}' <<<"$line")"
      cmdline="$(tr '\0' ' ' </proc/$pid/cmdline 2>/dev/null || true)"
      datadir="$(sed -n 's/.*--datadir=\([^ ]*\).*/\1/p' <<<"$cmdline")"
      [[ -z "$datadir" ]] && datadir="(default)"
      say "pid=$pid datadir=$datadir cmd=$cmdline"
    done <<<"$procs"
  fi

  section "B) RPC listener ownership"
  local listeners listener_pid
  listeners="$(ss -lntp "sport = :$RPC_PORT" 2>/dev/null || true)"
  if [[ -z "$listeners" ]]; then
    warn "Nothing listening on RPC port $RPC_PORT"
  else
    say "$listeners"
    listener_pid="$(echo "$listeners" | sed -n 's/.*pid=\([0-9]\+\).*/\1/p' | head -n1)"
    if [[ -n "$listener_pid" ]]; then
      say "listener_pid=$listener_pid"
      if [[ -n "$procs" ]] && ! echo "$procs" | awk '{print $1}' | grep -qx "$listener_pid"; then
        RPC_LISTENER_MISMATCH=1
        warn "RPC listener PID does not match known iriumd process list"
      fi
    fi
  fi

  section "C) RPC direct node checks"
  local node_count_json tpl_json best_json
  if ! node_count_json="$(node_getblockcount 2>/dev/null)"; then
    warn "Failed to query JSON-RPC getblockcount at $RPC_URL"
  else
    say "node_getblockcount=$node_count_json"
    NODE_HEIGHT="$(jq -r '.result // 0' <<<"$node_count_json")"
  fi

  best_json="$(node_getbestblockhash 2>/dev/null || true)"
  [[ -n "$best_json" ]] && say "node_getbestblockhash=$best_json"
  NODE_BEST_HASH="$(jq -r '.result // empty' <<<"${best_json:-{}}")"

  if ! tpl_json="$(node_getblocktemplate 2>/dev/null)"; then
    warn "Failed to query JSON-RPC getblocktemplate at $RPC_URL"
  else
    say "node_getblocktemplate=$tpl_json"
    NODE_TEMPLATE_HEIGHT="$(jq -r '.result.height // 0' <<<"$tpl_json")"
    NODE_TEMPLATE_PREV="$(jq -r '.result.previousblockhash // empty' <<<"$tpl_json")"
  fi

  if [[ -n "${NODE_HEIGHT:-}" && "$NODE_HEIGHT" =~ ^[0-9]+$ ]]; then
    if (( NODE_HEIGHT >= 15080 )); then
      SAMPLE_HEIGHT=15080
    else
      SAMPLE_HEIGHT=$(( NODE_HEIGHT / 2 ))
    fi
    NODE_SAMPLE_HASH="$(node_getblockhash_by_height "$SAMPLE_HEIGHT" 2>/dev/null || true)"
    say "sample_height=$SAMPLE_HEIGHT node_blockhash=$NODE_SAMPLE_HASH"
  fi

  section "C2) Shim JSON-RPC checks"
  local shim_count_json shim_tpl_json shim_best_json shim_hash_json
  shim_count_json="$(shim_rpc_call getblockcount '[]' 2>/dev/null || true)"
  shim_tpl_json="$(shim_rpc_call getblocktemplate '[{}]' 2>/dev/null || true)"
  shim_best_json="$(shim_rpc_call getbestblockhash '[]' 2>/dev/null || true)"
  if [[ -n "$SAMPLE_HEIGHT" ]]; then
    shim_hash_json="$(shim_rpc_call getblockhash "[$SAMPLE_HEIGHT]" 2>/dev/null || true)"
  else
    shim_hash_json=""
  fi

  [[ -n "$shim_count_json" ]] && SHIM_RPC_RESPONDED=1 && say "shim_getblockcount=$shim_count_json"
  [[ -n "$shim_tpl_json" ]] && say "shim_getblocktemplate=$shim_tpl_json"
  [[ -n "$shim_best_json" ]] && say "shim_getbestblockhash=$shim_best_json"
  [[ -n "$shim_hash_json" ]] && say "shim_getblockhash($SAMPLE_HEIGHT)=$shim_hash_json"

  SHIM_HEIGHT="$(jq -r '.result // .height // 0' <<<"${shim_count_json:-0}" 2>/dev/null || echo 0)"
  SHIM_TEMPLATE_HEIGHT="$(jq -r '.result.height // .height // 0' <<<"${shim_tpl_json:-{}}" 2>/dev/null || echo 0)"
  SHIM_TEMPLATE_PREV="$(jq -r '.result.previousblockhash // .result.prev_hash // .previousblockhash // .prev_hash // empty' <<<"${shim_tpl_json:-{}}" 2>/dev/null || true)"
  SHIM_SAMPLE_HASH="$(jq -r '.result // empty' <<<"${shim_hash_json:-{}}" 2>/dev/null || true)"

  if (( SHIM_RPC_RESPONDED )) && [[ "$NODE_HEIGHT" =~ ^[0-9]+$ && "$SHIM_TEMPLATE_HEIGHT" =~ ^[0-9]+$ ]]; then
    local d
    d="$(abs_diff "$NODE_HEIGHT" "$SHIM_TEMPLATE_HEIGHT")"
    if (( d > 2 )); then
      SHIM_TEMPLATE_DRIFT=1
      warn "Template drift detected: node_height=$NODE_HEIGHT shim_template_height=$SHIM_TEMPLATE_HEIGHT drift=$d"
    fi
  fi

  section "D) Height consistency checks"
  say "node_height=$NODE_HEIGHT"
  say "node_template_height=$NODE_TEMPLATE_HEIGHT"
  say "node_template_prev=$NODE_TEMPLATE_PREV"
  say "shim_height=$SHIM_HEIGHT"
  say "shim_template_height=$SHIM_TEMPLATE_HEIGHT"
  say "shim_template_prev=$SHIM_TEMPLATE_PREV"

  if [[ "$NODE_HEIGHT" =~ ^[0-9]+$ && "$NODE_TEMPLATE_HEIGHT" =~ ^[0-9]+$ ]]; then
    local d2
    d2="$(abs_diff "$NODE_HEIGHT" "$NODE_TEMPLATE_HEIGHT")"
    if (( d2 > 2 )); then
      warn "Direct node template differs from node height by >2 (d=$d2)."
    fi
  fi

  # Do not mark fork based only on absolute height (network grows). Fork detection should rely on
  # explorer divergence (when configured) or strong hash mismatch evidence.

  section "E) Optional explorer cross-check"
  if [[ -n "$EXPLORER_STATUS_URL" ]]; then
    local ex_json
    ex_json="$(curl -ksS --max-time 15 "$EXPLORER_STATUS_URL" || true)"
    if [[ -n "$ex_json" ]]; then
      EXPLORER_HEIGHT="$(jq -r '.height // .result.height // 0' <<<"$ex_json" 2>/dev/null || echo 0)"
      say "explorer_height=$EXPLORER_HEIGHT"
      if [[ "$NODE_HEIGHT" =~ ^[0-9]+$ && "$EXPLORER_HEIGHT" =~ ^[0-9]+$ ]]; then
        local de
        de="$(abs_diff "$NODE_HEIGHT" "$EXPLORER_HEIGHT")"
        say "node_vs_explorer_diff=$de"
        if (( de > 1000 )); then
          FORK_SUSPECT=1
          warn "Large node/explorer height divergence (>1000)."
        fi
      fi
    else
      warn "Explorer cross-check URL set but no response"
    fi
  else
    say "Explorer cross-check skipped (IRIUM_EXPLORER_STATUS_URL not set)"
  fi

  section "F) Data dir consistency"
  say "expected_data_dir=$DATA_DIR"
  if [[ -d "$DATA_DIR" ]]; then
    ls -ld "$DATA_DIR" "$DATA_DIR/blocks" "$DATA_DIR/chainstate" 2>/dev/null || true
  else
    warn "Data directory missing: $DATA_DIR"
  fi

  section "G) Pool/shim config scan"
  for f in /etc/irium-pool/shim.env /etc/irium-pool/stratum.env /etc/irium-pool/ckpool.conf; do
    if [[ -f "$f" ]]; then
      say "--- $f"
      sed -E 's/(IRIUM_RPC_TOKEN=).*/\1<redacted>/; s/("pass"[[:space:]]*:[[:space:]]*")[^"]*/\1<redacted>/g' "$f" || true
    fi
  done
  section "systemd unit snippets"
  systemctl cat iriumd 2>/dev/null || true
  systemctl cat irium-pool-shim 2>/dev/null || true
  systemctl cat irium-stratum 2>/dev/null || true

  section "Summary"
  local cause next
  cause="No critical mismatch detected"
  next="Keep monitoring. If miners still report wrong heights, run fix mode with explorer cross-check configured."

  if (( MULTI_IRIUMD )); then
    cause="Multiple iriumd instances"
    next="Stop all extra iriumd processes and keep only systemd-managed iriumd."
  elif (( RPC_LISTENER_MISMATCH )); then
    cause="RPC port listener mismatch"
    next="Rebind shim/stratum to the intended local iriumd at $RPC_URL and restart services."
  elif (( SHIM_TEMPLATE_DRIFT )); then
    cause="Shim template drift/caching or wrong upstream"
    next="Patch shim checks, pin RPC URL/token, and restart iriumd -> shim -> stratum."
  elif (( ! SHIM_RPC_RESPONDED )); then
    cause="Pool shim not reachable on this host"
    next="If this is not a pool host, ignore. If this is a pool host, start/repair irium-pool-shim and rerun diagnose."
  elif (( FORK_SUSPECT )); then
    cause="Forked/wrong chain data"
    next="Backup chain data, wipe blocks+chainstate, and resync from peers."
  fi

  say "LIKELY CAUSE: $cause"
  say "NEXT ACTION: $next"
}

fix() {
  section "FIX MODE"
  info "Applying safe repair workflow"

  section "Stopping pool-facing services"
  for s in irium-stratum irium-pool-shim ckpool; do
    if systemctl list-unit-files | awk '{print $1}' | grep -qx "${s}.service"; then
      sudo systemctl stop "$s" || true
    fi
  done

  section "Multiple iriumd cleanup"
  if pgrep -x iriumd >/dev/null; then
    sudo systemctl stop iriumd || true
    pkill -TERM -x iriumd || true
    sleep 2
    pkill -KILL -x iriumd || true
  fi

  install_dropins
  patch_live_shim

  if (( FORK_SUSPECT )); then
    local bdir
    bdir="$(backup_chain_data)"
    info "backup_dir=$bdir"
    rm -rf "$DATA_DIR/blocks" "$DATA_DIR/chainstate"
    rm -f "$DATA_DIR/peer_reputation.json"
    info "removed blocks/chainstate/peer_reputation.json for clean resync"
  fi

  section "Restart order"
  sudo systemctl start iriumd
  sleep 5
  sudo systemctl start irium-pool-shim || true
  sleep 2
  sudo systemctl start irium-stratum || true

  section "Post-fix quick verify"
  diagnose
}

main() {
  diagnose
  if (( DO_FIX )); then
    fix
  fi
}

main
