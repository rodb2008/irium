#!/usr/bin/env python3
"""Helper to sign bootstrap/anchors.json with ssh-keygen."""
from __future__ import annotations

import argparse
import json
import subprocess
import tempfile
from pathlib import Path


def canonical_json(path: Path) -> bytes:
    data = json.loads(path.read_text())
    return json.dumps(data, separators=(",", ":"), sort_keys=True).encode()


def extract_signature(sig_path: Path) -> str:
    lines = []
    for line in sig_path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("-----"):
            continue
        lines.append(line)
    return "".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description="Sign anchors.json using ssh-keygen -Y sign")
    parser.add_argument("--anchors", default="bootstrap/anchors.json", help="Path to anchors.json")
    parser.add_argument("--key", default="~/.ssh/git-signing", help="Private key used for signing")
    parser.add_argument("--signer", default="iriumlabs", help="Signer label recorded in JSON")
    parser.add_argument("--namespace", default="irium-anchor", help="ssh-keygen namespace")
    args = parser.parse_args()

    anchors_path = Path(args.anchors)
    if not anchors_path.exists():
        raise SystemExit(f"Anchors file not found: {anchors_path}")

    canonical = canonical_json(anchors_path)
    with tempfile.NamedTemporaryFile(delete=False) as tmp:
        tmp.write(canonical)
        tmp_path = Path(tmp.name)

    sig_file = tmp_path.with_suffix(".sig")
    if sig_file.exists():
        sig_file.unlink()

    key_path = Path(args.key).expanduser()
    cmd = [
        "ssh-keygen", "-Y", "sign",
        "-n", args.namespace,
        "-f", str(key_path),
        str(tmp_path)
    ]
    subprocess.run(cmd, check=True)

    if not sig_file.exists():
        raise SystemExit("ssh-keygen did not produce a signature file")

    signature = extract_signature(sig_file)
    sig_entry = {
        "signer": args.signer,
        "public_key": key_path.with_suffix(".pub").read_text().strip(),
        "namespace": args.namespace,
        "algorithm": "ssh-ed25519",
        "signature": signature,
    }

    anchors = json.loads(anchors_path.read_text())
    anchors.setdefault("signatures", [])
    anchors["signatures"].append(sig_entry)
    anchors_path.write_text(json.dumps(anchors, indent=2) + "\n")

    tmp_path.unlink(missing_ok=True)
    sig_file.unlink(missing_ok=True)
    print(f"Added signature from {args.signer} to {anchors_path}")


if __name__ == "__main__":
    main()
