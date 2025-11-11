#!/usr/bin/env python3
"""Helper to sign bootstrap/anchors.json with ssh-keygen."""
from __future__ import annotations

import argparse
import json
import subprocess
import tempfile
import textwrap
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


def canonical_payload_bytes(path: Path) -> bytes:
    """Return canonical JSON bytes excluding signature entries."""
    data = json.loads(path.read_text())
    payload = dict(data)
    payload.pop("signatures", None)
    return json.dumps(payload, separators=(",", ":"), sort_keys=True).encode()


def write_signature_file(signature_b64: str, dest: Path) -> None:
    """Write an SSH signature file with BEGIN/END wrappers."""
    wrapped = textwrap.fill(signature_b64, 70)
    dest.write_text(
        "-----BEGIN SSH SIGNATURE-----\n"
        f"{wrapped}\n"
        "-----END SSH SIGNATURE-----\n"
    )


def create_signature(
    tmp_payload: Path,
    key_path: Path,
    namespace: str,
) -> Path:
    """Invoke ssh-keygen -Y sign and return the resulting signature path."""
    sig_path = tmp_payload.with_suffix(".sig")
    if sig_path.exists():
        sig_path.unlink()

    cmd = [
        "ssh-keygen",
        "-Y",
        "sign",
        "-n",
        namespace,
        "-f",
        str(key_path),
        str(tmp_payload),
    ]
    subprocess.run(cmd, check=True)

    if not sig_path.exists():
        raise SystemExit("ssh-keygen did not produce a signature file")
    return sig_path


def extract_signature(sig_path: Path) -> str:
    lines = []
    for line in sig_path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("-----"):
            continue
        lines.append(line)
    return "".join(lines)


def upsert_signature(data: dict[str, Any], new_sig: dict[str, Any]) -> None:
    """Replace existing signer entry (if any) and append the new signature."""
    signatures = data.setdefault("signatures", [])
    filtered = [entry for entry in signatures if entry.get("signer") != new_sig["signer"]]
    filtered.append(new_sig)
    data["signatures"] = filtered


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

    key_path = Path(args.key).expanduser()
    if not key_path.exists():
        raise SystemExit(f"Signing key not found: {key_path}")

    canonical_bytes = canonical_payload_bytes(anchors_path)
    data = json.loads(anchors_path.read_text())

    with tempfile.NamedTemporaryFile(delete=False) as tmp:
        tmp.write(canonical_bytes)
        tmp_path = Path(tmp.name)

    sig_path = None
    try:
        sig_path = create_signature(tmp_path, key_path, args.namespace)
        signature_b64 = extract_signature(sig_path)
    finally:
        tmp_path.unlink(missing_ok=True)
        if sig_path is not None:
            sig_path.unlink(missing_ok=True)

    pub_key_path = key_path.with_suffix(".pub")
    if not pub_key_path.exists():
        raise SystemExit(f"Missing public key: {pub_key_path}")

    sig_entry = {
        "signer": args.signer,
        "public_key": pub_key_path.read_text().strip(),
        "namespace": args.namespace,
        "algorithm": "ssh-ed25519",
        "signature": signature_b64,
        "signed_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
    }

    upsert_signature(data, sig_entry)
    anchors_path.write_text(json.dumps(data, indent=2) + "\n")
    print(f"Added signature from {args.signer} to {anchors_path}")


if __name__ == "__main__":
    main()
