#!/usr/bin/env python3
"""
Submit Litecoin headers to a devnet/testnet iriumd via
POST /rpc/submitltcheaders. Mirrors submit_btc_headers.sh but for LTC
and targets a local litecoind regtest instance rather than a public
header API.

Two modes:
  --source litecoind   Pull headers directly from a running litecoind
                       via getblockhash + getblockheader.
  --source file        Read pre-fetched headers (one 160-char-hex line
                       per header) from a file.

For regtest devnet testing the litecoind source is the canonical path;
the file source is provided for offline rehearsals.

Env vars (consumed):
  IRIUMD_RPC_URL       default http://127.0.0.1:38400  (devnet iriumd)
  IRIUMD_RPC_TOKEN     required if iriumd has IRIUM_RPC_TOKEN set
  LTCD_RPC_URL         default http://127.0.0.1:19443  (litecoind regtest)
  LTCD_RPC_USER        default iriumtest
  LTCD_RPC_PASSWORD    default iriumtest

Exit codes:
  0  submission accepted
  1  any failure (RPC error, header gap, hex parse, network)
"""

import argparse
import base64
import json
import os
import sys
import urllib.error
import urllib.request


def env(key: str, default: str = "") -> str:
    return os.environ.get(key, default)


def basic_auth(user: str, password: str) -> str:
    raw = f"{user}:{password}".encode("utf-8")
    return "Basic " + base64.b64encode(raw).decode("ascii")


def ltcd_rpc(method: str, params=None) -> dict:
    """Call litecoind JSON-RPC."""
    url = env("LTCD_RPC_URL", "http://127.0.0.1:19443")
    user = env("LTCD_RPC_USER", "iriumtest")
    password = env("LTCD_RPC_PASSWORD", "iriumtest")
    body = json.dumps({
        "jsonrpc": "1.0",
        "id": "submit_ltc_headers",
        "method": method,
        "params": params or [],
    }).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=body,
        headers={
            "Content-Type": "application/json",
            "Authorization": basic_auth(user, password),
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            data = json.loads(resp.read())
    except urllib.error.HTTPError as e:
        raise RuntimeError(
            f"litecoind RPC {method} HTTP {e.code}: {e.read().decode('utf-8', 'replace')}"
        )
    except urllib.error.URLError as e:
        raise RuntimeError(f"litecoind RPC {method} URL error: {e}")
    if data.get("error"):
        raise RuntimeError(f"litecoind RPC {method} error: {data['error']}")
    return data["result"]


def fetch_headers_from_litecoind(start_height: int, count: int) -> str:
    """Return hex-concatenated 80-byte headers for [start_height,
    start_height + count)."""
    chunks = []
    for h in range(start_height, start_height + count):
        block_hash = ltcd_rpc("getblockhash", [h])
        # getblockheader with verbose=False returns the 160-char hex
        # serialized 80-byte header.
        header_hex = ltcd_rpc("getblockheader", [block_hash, False])
        if len(header_hex) != 160:
            raise RuntimeError(
                f"litecoind getblockheader at h={h} returned len={len(header_hex)} chars, expected 160"
            )
        chunks.append(header_hex)
    return "".join(chunks)


def fetch_headers_from_file(path: str) -> str:
    """Concatenate hex-only lines from `path`; tolerate blanks and
    comments. Verify the total length is a multiple of 160."""
    out = []
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            out.append(line)
    joined = "".join(out)
    if len(joined) % 160 != 0:
        raise RuntimeError(
            f"file {path}: total hex length {len(joined)} not a multiple of 160"
        )
    return joined


def submit_to_iriumd(headers_hex: str, fee_per_byte: int, broadcast: bool) -> dict:
    url = env("IRIUMD_RPC_URL", "http://127.0.0.1:38400") + "/rpc/submitltcheaders"
    token = env("IRIUMD_RPC_TOKEN", "")
    if not token:
        # Allowed only on a fresh devnet that hasn't set IRIUM_RPC_TOKEN.
        # The require_rpc_auth helper short-circuits to Ok when no token
        # is set in iriumd's process env.
        sys.stderr.write("[warn] IRIUMD_RPC_TOKEN unset — submission will work only if iriumd has no token configured\n")
    body = json.dumps({
        "headers_hex": headers_hex,
        "broadcast": broadcast,
        "fee_per_byte": fee_per_byte,
    }).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=body,
        headers={
            "Content-Type": "application/json",
            **({"Authorization": f"Bearer {token}"} if token else {}),
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            return json.loads(resp.read())
    except urllib.error.HTTPError as e:
        raise RuntimeError(
            f"iriumd /rpc/submitltcheaders HTTP {e.code}: {e.read().decode('utf-8', 'replace')}"
        )
    except urllib.error.URLError as e:
        raise RuntimeError(f"iriumd RPC URL error: {e}")


def main() -> int:
    parser = argparse.ArgumentParser(description="Submit LTC headers to iriumd")
    parser.add_argument(
        "--source",
        choices=["litecoind", "file"],
        default="litecoind",
        help="header source (default: litecoind)",
    )
    parser.add_argument("--from", dest="from_height", type=int, default=1,
                        help="first LTC block height (litecoind source; default 1)")
    parser.add_argument("--count", type=int, default=144,
                        help="number of headers (litecoind source; default 144)")
    parser.add_argument("--file", default="",
                        help="path to pre-fetched hex file (file source)")
    parser.add_argument("--fee-per-byte", type=int, default=1)
    parser.add_argument("--no-broadcast", action="store_true",
                        help="build but do NOT submit to mempool")
    args = parser.parse_args()

    if args.source == "litecoind":
        if args.count <= 0 or args.count > 144:
            sys.stderr.write("count must be 1..144 (iriumd cap)\n")
            return 1
        sys.stderr.write(
            f"[fetch] litecoind heights {args.from_height}..{args.from_height + args.count - 1}\n"
        )
        headers_hex = fetch_headers_from_litecoind(args.from_height, args.count)
    else:
        if not args.file:
            sys.stderr.write("--file required when --source file\n")
            return 1
        sys.stderr.write(f"[fetch] file {args.file}\n")
        headers_hex = fetch_headers_from_file(args.file)

    header_count = len(headers_hex) // 160
    sys.stderr.write(f"[submit] {header_count} header(s), {len(headers_hex)} hex chars\n")

    resp = submit_to_iriumd(
        headers_hex,
        fee_per_byte=args.fee_per_byte,
        broadcast=(not args.no_broadcast),
    )
    print(json.dumps(resp, indent=2))
    if not resp.get("accepted", False):
        sys.stderr.write("[fail] iriumd did not accept the batch\n")
        return 1
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except RuntimeError as e:
        sys.stderr.write(f"[error] {e}\n")
        sys.exit(1)
