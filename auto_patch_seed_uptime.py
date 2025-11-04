#!/usr/bin/env python3
import os, json, re, datetime

BASE = "/home/irium/irium/irium"
P2P_FILE = os.path.join(BASE, "p2p.py")
UPTIME_FILE = os.path.join(BASE, "uptime.py")
SEED_FILE = "/home/irium/.irium/seeds.txt"

print("🔧 Patching Irium node seed uptime logic...")

# 1️⃣ Patch uptime.py to add peer uptime store
with open(UPTIME_FILE, "r") as f:
    uptime_code = f.read()

if "def record_peer_uptime" not in uptime_code:
    patch_uptime = """

# === AutoPatch: Peer uptime tracking ===
import json, os, datetime
SEED_FILE = os.path.expanduser("~/.irium/seeds.txt")

def load_peer_uptime():
    if not os.path.exists(SEED_FILE):
        return {}
    try:
        with open(SEED_FILE) as f:
            return json.load(f)
    except Exception:
        return {}

def save_peer_uptime(data):
    os.makedirs(os.path.dirname(SEED_FILE), exist_ok=True)
    with open(SEED_FILE, "w") as f:
        json.dump(data, f, indent=2)

def record_peer_uptime(ip):
    data = load_peer_uptime()
    now = datetime.datetime.utcnow().timestamp()
    peer = data.get(ip, {"first_seen": now, "last_seen": now})
    peer["last_seen"] = now
    data[ip] = peer
    save_peer_uptime(data)

def prune_peers():
    data = load_peer_uptime()
    now = datetime.datetime.utcnow().timestamp()
    keep = {}
    for ip, meta in data.items():
        age_days = (now - meta["first_seen"]) / 86400
        inactive = (now - meta["last_seen"]) / 86400
        if age_days >= 7:
            meta["trusted"] = True
        if inactive < 1:
            keep[ip] = meta
    save_peer_uptime(keep)
# === End AutoPatch ===
"""
    with open(UPTIME_FILE, "a") as f:
        f.write(patch_uptime)
    print("✅ Patched uptime.py")

# 2️⃣ Patch p2p.py to record uptime on connection and prune hourly
with open(P2P_FILE, "r") as f:
    p2p_code = f.read()

if "record_peer_uptime" not in p2p_code:
    p2p_code = re.sub(
        r"(import asyncio.*)",
        r"\1\nfrom irium.uptime import record_peer_uptime, prune_peers",
        p2p_code,
        count=1,
    )

    p2p_code = re.sub(
        r"(async def handle_client\(.*?\):)",
        r"\1\n    import asyncio; prune_peers()",
        p2p_code,
        count=1,
    )

    p2p_code = re.sub(
        r"(async def connect_to_peer\(self, host, port.*?\):)",
        r"\1\n        record_peer_uptime(host)",
        p2p_code,
        count=1,
    )

    with open(P2P_FILE, "w") as f:
        f.write(p2p_code)
    print("✅ Patched p2p.py")

print("💾 Done. Restart your node to apply changes.")
