"""Verify signed bootstrap material for zero-DNS startup."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Iterable, List

from irium.wallet import verify_der_signature

BOOTSTRAP_PUBLIC_KEYS = {
    "founder": "03ffb48b174908aa757487b8dbf39bb0a021f20572f865530891a243f59242c702",
    "guardian": "02c5f1f7a54f60d2ba2c9fb3b7910e7198ff9d1de3c8c8fba34cb57ec9adadd9d6",
}


def hash_file(path: Path) -> bytes:
    digest = hashlib.sha256()
    with path.open("rb") as infile:
        while chunk := infile.read(8192):
            digest.update(chunk)
    return digest.digest()


def _load_seed_entries(path: Path) -> List[str]:
    if not path.exists():
        return []
    entries = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        entries.append(line)
    return entries


def verify_seedlist(seed_path: Path, runtime_path: Path | None = None, signature_path: Path | None = None) -> Iterable[str]:
    if not seed_path.exists():
        raise FileNotFoundError(seed_path)
    seeds = _load_seed_entries(seed_path)
    if runtime_path is not None:
        seeds.extend(_load_seed_entries(runtime_path))
    unique = list(dict.fromkeys(seeds))
    if not unique:
        raise ValueError("Seedlist contains no usable entries")
    if signature_path is not None:
        verify_seedlist_signature(seed_path, signature_path)
    return unique


def verify_anchor(anchor_path: Path) -> None:
    data = json.loads(anchor_path.read_text())
    if "anchors" not in data:
        raise ValueError("anchors.json missing anchors array")
    for anchor in data["anchors"]:
        digest = _anchor_digest(anchor)
        signatures = anchor.get("signatures", [])
        if not signatures:
            raise ValueError("Anchor is missing signatures")
        _verify_signatures(signatures, digest, f"anchor@{anchor.get('height')}")


def verify_seedlist_signature(seed_path: Path, signature_path: Path) -> None:
    data = json.loads(signature_path.read_text())
    expected_hash = data.get("hash")
    if not isinstance(expected_hash, str):
        raise ValueError("Seedlist signature missing hash")
    digest = hash_file(seed_path)
    if expected_hash.lower() != digest.hex():
        raise ValueError("Seedlist hash mismatch")
    signatures = data.get("signatures", [])
    if not signatures:
        raise ValueError("Seedlist signature file missing signatures")
    _verify_signatures(signatures, digest, "seedlist.txt")


def _anchor_digest(anchor: dict) -> bytes:
    try:
        height = int(anchor["height"])
        block_hash = bytes.fromhex(anchor["block_hash"])
        timestamp = int(anchor["timestamp"])
    except (KeyError, ValueError) as exc:
        raise ValueError("Anchor entry missing required fields") from exc
    if len(block_hash) != 32:
        raise ValueError("Anchor block hash must be 32 bytes")
    payload = (
        height.to_bytes(4, "big", signed=False)
        + block_hash
        + timestamp.to_bytes(8, "big", signed=False)
    )
    return hashlib.sha256(payload).digest()


def _verify_signatures(signatures: List[dict], digest: bytes, subject: str) -> None:
    valid_count = 0
    for entry in signatures:
        signer = entry.get("signer")
        signature_hex = entry.get("signature")
        if signer not in BOOTSTRAP_PUBLIC_KEYS:
            raise ValueError(f"Unknown signer '{signer}' in {subject}")
        if not isinstance(signature_hex, str):
            raise ValueError(f"Signature for {signer} in {subject} is not a string")
        try:
            signature = bytes.fromhex(signature_hex)
        except ValueError as exc:
            raise ValueError(f"Signature for {signer} in {subject} is not valid hex") from exc
        pubkey = bytes.fromhex(BOOTSTRAP_PUBLIC_KEYS[signer])
        if not verify_der_signature(pubkey, digest, signature):
            raise ValueError(f"Signature verification failed for {signer} in {subject}")
        valid_count += 1
    if valid_count == 0:
        raise ValueError(f"No valid signatures found for {subject}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seed", type=Path, required=True)
    parser.add_argument("--runtime", type=Path, default=Path("bootstrap/seedlist.runtime"))
    parser.add_argument("--seed-sig", type=Path, default=Path("bootstrap/seedlist.txt.sig"))
    parser.add_argument("--anchors", type=Path, required=True)
    args = parser.parse_args()

    seeds = verify_seedlist(args.seed, args.runtime, args.seed_sig)
    verify_anchor(args.anchors)
    seed_hash = hash_file(args.seed)
    print("Seedlist hash:", seed_hash.hex())
    if args.runtime.exists():
        print("Runtime seedlist hash:", hash_file(args.runtime).hex())
    print("Combined peers:", len(seeds))
    print("Anchors hash:", hash_file(args.anchors).hex())
    print("Signatures verified")


if __name__ == "__main__":
    main()
