from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path


SCRIPTS_DIR = Path(__file__).resolve().parents[1] / "scripts"


def _exec_script(script: str, extra: list[str]) -> None:
    script_path = SCRIPTS_DIR / script
    if not script_path.exists():
        raise SystemExit(f"Unable to locate {script_path}")
    os.execv(sys.executable, [sys.executable, str(script_path), *extra])


def main() -> None:
    parser = argparse.ArgumentParser(
        prog="irium",
        description="Irium mainnet CLI utilities",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    node_cmd = sub.add_parser("node", help="Start the full node service")
    node_cmd.add_argument("--port", type=int, default=38291, help="P2P port to bind (default: 38291)")

    miner_cmd = sub.add_parser("miner", help="Launch the reference miner")
    miner_cmd.add_argument("port", type=int, nargs="?", default=38292, help="Outbound P2P port (default: 38292)")

    sub.add_parser("explorer", help="Start the explorer REST API")
    sub.add_parser("wallet-api", help="Start the wallet HTTP API")
    sub.add_parser("verify-genesis", help="Recompute and display locked-genesis hash")

    args = parser.parse_args()

    if args.command == "node":
        _exec_script("irium-node.py", ["--port", str(args.port)])
    elif args.command == "miner":
        _exec_script("irium-miner.py", [str(args.port)])
    elif args.command == "explorer":
        _exec_script("irium-explorer-api.py", [])
    elif args.command == "wallet-api":
        _exec_script("irium-wallet-api-ssl.py", [])
    elif args.command == "verify-genesis":
        _exec_script("verify_genesis.py", [])


if __name__ == "__main__":
    main()
