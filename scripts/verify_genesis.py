import hashlib
import json
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GENESIS_LOCKED = ROOT / "configs" / "genesis-locked.json"

def _little_endian_bytes(hex_str: str) -> bytes:
    return bytes.fromhex(hex_str)[::-1]

with open(GENESIS_LOCKED, "r", encoding="utf-8") as fp:
    payload = json.load(fp)

header = payload["header"]

version = struct.pack("<I", header["version"])
prev_hash = _little_endian_bytes(header["prev_hash"])
merkle_root = _little_endian_bytes(header["merkle_root"])
time_bytes = struct.pack("<I", header["time"])
bits_value = header["bits"]
if isinstance(bits_value, str):
    bits_compact = int(bits_value, 16)
else:
    bits_compact = int(bits_value)
bits = struct.pack("<I", bits_compact)
nonce = struct.pack("<I", header["nonce"])
block_header = version + prev_hash + merkle_root + time_bytes + bits + nonce

derived = hashlib.sha256(hashlib.sha256(block_header).digest()).digest()[::-1].hex()
print("derived header hash:", derived)
print("file header hash   :", header["hash"])
print("match:", derived.lower() == header["hash"].lower())
