#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BOOTSTRAP_DIR="$ROOT_DIR/bootstrap"
VERIFY_BIN="python3 -m irium.tools.verify_bootstrap"

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is required" >&2
  exit 1
fi

python3 -m irium.tools.verify_bootstrap \
  --seed "$BOOTSTRAP_DIR/seedlist.txt" \
  --runtime "$BOOTSTRAP_DIR/seedlist.runtime" \
  --seed-sig "$BOOTSTRAP_DIR/seedlist.txt.sig" \
  --anchors "$BOOTSTRAP_DIR/anchors.json"

printf '\nBootstrap material verified. Exporting seeds...\n'
python3 - "$BOOTSTRAP_DIR" <<'PY'
import sys
from pathlib import Path

from irium.tools.verify_bootstrap import verify_seedlist

root = Path(sys.argv[1])
for entry in verify_seedlist(root / "seedlist.txt", root / "seedlist.runtime"):
    print(entry)
PY
