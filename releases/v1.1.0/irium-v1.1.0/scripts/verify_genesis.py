import json, sys, hashlib, struct

with open("configs/genesis.header.json","r") as f:
    g = json.load(f)
h = g["header"]

# pack header like Bitcoin-style (version|prev|merkle|time|bits|nonce)
def h2b(x): return bytes.fromhex(x)[::-1]  # little-endian
version = struct.pack("<I", h["version"])
prev    = h2b(h["prev_hash"])
merkle  = h2b(h["merkle_root"])
time_   = struct.pack("<I", h["time"])
bits    = bytes.fromhex(h["bits"])[::-1] if len(h["bits"])==8 else struct.pack("<I", int(h["bits"],16))
nonce   = struct.pack("<I", h["nonce"])
header  = version + prev + merkle + time_ + bits + nonce

hash_ = hashlib.sha256(hashlib.sha256(header).digest()).digest()[::-1].hex()
print("derived header hash:", hash_)
print("file header hash   :", h["hash"])
print("match:", hash_.lower()==h["hash"].lower())
