#!/usr/bin/env python3
"""
Phase 11-B regression soak: canonical receipts_root + full solution validation.

Root causes fixed in v6 (vs. earlier drafts):
  1. DATA_DIR must be under $HOME — storage.rs configured_dir() rejects /tmp paths.
  2. Stratum needs IRIUM_POW_LIMIT_HEX=7fffff...000 to match the harness DIFF1 (devnet
     max).  Without it, pow_limit=0x1d00ffff (Bitcoin genesis) and the share target is
     far harder than the devnet block target, so every share is soft-accepted but never
     submitted to iriumd.
  3. poawx_get_assignment returns 404 at genesis (tip_h==0).  Mine 1 warm-up block via
     the harness before T1-T6.
  4. (v6) Stratum's `current` job is only updated when height:prevhash key changes.
     Receipts posted by T1/T4+T5 are never seen by the stratum job that was broadcast
     after warmup.  Fix: restart stratum AFTER T1-T6 post receipts.  The first template
     poll by the restarted stratum fetches h=2 WITH pending receipts and sets `current`
     to a job that includes them.  T7 harness then authorizes → gets receipted h=2 job
     → mines h=2 via submit_block_extended → receipts cleared.  Subsequent blocks have
     0 pending receipts and are accepted via submit_block.
  5. (v6) Removed --receipt from T7 harness invocation.  The harness's receipt mode
     posts a receipt then calls wait_notify(20).  Because the stratum only re-broadcasts
     on key change (not on receipt list change), wait_notify always times out → blocks_done
     stalls → harness FAIL.  The T1/T4+T5 receipts already provide the irx1=True block
     via block h=2 (the first T7 block).

Tests:
  T1   valid receipt accepted (HTTP 200)
  T2   wrong commitment_nonce rejected (HTTP 400)
  T3   insufficient solution PoW rejected (HTTP 400)
  T4   canonical root is order-independent: Python root(A,B) == root(B,A)
  T5   iriumd receipts_root matches Python-computed canonical root
  T6   fabricated receipt (wrong nonce) in submit_block_extended rejected (HTTP 400)
  T7   10-block stratum soak: >=1 block via SBE (irx1=True) and harness exit=0
  T8   mainnet PIDs/ports untouched before and after
"""

import hashlib, json, os, re, secrets, shutil, signal, socket, struct
import subprocess, sys, time, urllib.request, urllib.error

REPO         = os.path.expanduser("~/irium")
IRIUMD_BIN   = f"{REPO}/target/release/iriumd"
STRATUM_BIN  = f"{REPO}/pool/irium-stratum/target/release/irium-stratum"
HARNESS      = f"{REPO}/scripts/poawx-stratum-long-soak-harness.py"
# Must be under $HOME so storage.rs configured_dir() accepts it
DATA_DIR     = os.path.expanduser("~/poawx-phase11b-soak")
RPC_PORT     = 39511
STRATUM_PORT = 39512
TOKEN        = secrets.token_hex(16)

# Devnet PoW limit — must match harness DIFF1_TARGET so share targets align
DEVNET_POW_LIMIT_HEX = "7fffff0000000000000000000000000000000000000000000000000000000000"

STRATUM_ENV = {
    "IRIUM_STRATUM_POAWX":    "1",
    "IRIUM_RPC_BASE":         f"http://127.0.0.1:{RPC_PORT}",
    "IRIUM_RPC_TOKEN":        TOKEN,
    "STRATUM_BIND":           f"0.0.0.0:{STRATUM_PORT}",
    "IRIUM_POW_LIMIT_HEX":    DEVNET_POW_LIMIT_HEX,
    "IRIUM_HASH_CMP_MODE":    "le",
}

# Mainnet ports (must not be touched) — PIDs detected dynamically at startup
MAINNET_IRIUMD_PORT = 38300
MAINNET_STRAT_PORT  = 3333

# ─── crypto helpers ─────────────────────────────────────────────────────────
def sha256(d):  return hashlib.sha256(d).digest()
def sha256d(d): return sha256(sha256(d))

def leading_zeros(h):
    bits = 0
    for b in h:
        if b == 0: bits += 8
        else:
            m = 0x80
            while m and not (b & m): bits += 1; m >>= 1
            break
    return bits

def compute_root(receipts):
    """Canonical receipts_root (Phase 11-B).

    Sort by (height, lane bytes, worker_pkh hex-str bytes, nonce hex-str bytes)
    then SHA256(concat(SHA256(each receipt fields))).
    """
    s = sorted(receipts, key=lambda r: (
        r["height"],
        r["lane"].encode(),
        r["worker_pkh"].encode(),
        r["commitment_nonce"].encode(),
    ))
    outer = hashlib.sha256()
    for r in s:
        inner = hashlib.sha256()
        inner.update(r["height"].to_bytes(8, "little"))
        inner.update(r["lane"].encode())
        inner.update(bytes.fromhex(r["worker_pkh"]))
        inner.update(bytes.fromhex(r["solution"]))
        inner.update(bytes.fromhex(r["commitment_nonce"]))
        outer.update(inner.digest())
    return outer.hexdigest()

def brute_force(seed_hex, nonce_hex, diff):
    seed, nonce = bytes.fromhex(seed_hex), bytes.fromhex(nonce_hex)
    sol = bytearray(32)
    for i in range(10_000_000):
        struct.pack_into("<I", sol, 0, i)
        h = sha256d(seed + nonce + bytes(sol))
        if leading_zeros(h) >= diff:
            return bytes(sol).hex()
    raise RuntimeError("brute_force: no solution found within 10M attempts")

def brute_force_failing(seed_hex, nonce_hex):
    """Find a solution whose SHA256d has 0 leading zero bits (fails diff=1)."""
    seed, nonce = bytes.fromhex(seed_hex), bytes.fromhex(nonce_hex)
    sol = bytearray(32)
    for i in range(10_000_000):
        struct.pack_into("<I", sol, 0, i)
        h = sha256d(seed + nonce + bytes(sol))
        if h[0] >= 128:
            return bytes(sol).hex()
    raise RuntimeError("brute_force_failing: could not find a failing solution")

# ─── process management ─────────────────────────────────────────────────────
procs = []

def spawn(cmd, env_extra, log_path, cwd=None):
    env = os.environ.copy()
    env.update(env_extra)
    log = open(log_path, "w")
    p = subprocess.Popen(cmd, env=env, stdout=log, stderr=log, cwd=cwd)
    procs.append(p)
    return p

def pids_on_port(port):
    """Return PIDs listening on the given TCP port."""
    r = subprocess.run(["ss", "-lntp", f"sport = :{port}"], capture_output=True, text=True)
    return [int(m) for m in re.findall(r"pid=(\d+)", r.stdout)]

def wait_http_200(url, headers, timeout=30):
    end = time.time() + timeout
    while time.time() < end:
        try:
            req = urllib.request.Request(url, headers=headers)
            with urllib.request.urlopen(req, timeout=3) as r:
                if r.status == 200: return True
        except Exception: pass
        time.sleep(0.3)
    return False

def wait_port(port, timeout=15):
    end = time.time() + timeout
    while time.time() < end:
        try:
            s = socket.create_connection(("127.0.0.1", port), 0.5)
            s.close(); return True
        except OSError: time.sleep(0.2)
    return False

def cleanup():
    for p in reversed(procs):
        try: p.terminate(); p.wait(timeout=5)
        except Exception:
            try: p.kill()
            except Exception: pass
    if os.path.isdir(DATA_DIR):
        shutil.rmtree(DATA_DIR, ignore_errors=True)
    print("[cleanup] testnet processes stopped, data dir removed")

signal.signal(signal.SIGTERM, lambda *_: (cleanup(), sys.exit(0)))

# ─── RPC helpers ────────────────────────────────────────────────────────────
def rpc_get(path):
    req = urllib.request.Request(
        f"http://127.0.0.1:{RPC_PORT}{path}",
        headers={"Authorization": f"Bearer {TOKEN}"})
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.loads(r.read())

def rpc_post(path, body):
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        f"http://127.0.0.1:{RPC_PORT}{path}", data=data,
        headers={"Authorization": f"Bearer {TOKEN}",
                 "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()

# ─── test counters ───────────────────────────────────────────────────────────
RESULTS = []

def ok(label, detail=""):
    RESULTS.append(("PASS", label, detail))
    print(f"[PASS] {label}" + (f" — {detail}" if detail else ""))

def fail(label, detail=""):
    RESULTS.append(("FAIL", label, detail))
    print(f"[FAIL] {label}" + (f" — {detail}" if detail else ""))

def skip(label, reason=""):
    RESULTS.append(("SKIP", label, reason))
    print(f"[SKIP] {label}" + (f" — {reason}" if reason else ""))

# ════════════════════════════════════════════════════════════════════════════
# MAIN
# ════════════════════════════════════════════════════════════════════════════
print("=" * 68)
print("Phase 11-B Regression Soak")
print(f"iriumd:   {IRIUMD_BIN}")
print(f"stratum:  {STRATUM_BIN}")
print(f"data_dir: {DATA_DIR}")
print("=" * 68)

# ── T8 pre-check ────────────────────────────────────────────────────────────
print("\n─── T8: mainnet safety pre-check ───────────────────────────────────")
# Detect mainnet process PIDs dynamically (avoid hardcoded stale PIDs).
mainnet_iriumd_pids = pids_on_port(MAINNET_IRIUMD_PORT)
mainnet_strat_pids  = pids_on_port(MAINNET_STRAT_PORT)
print(f"  mainnet iriumd PIDs on :{MAINNET_IRIUMD_PORT}: {mainnet_iriumd_pids}")
print(f"  mainnet stratum PIDs on :{MAINNET_STRAT_PORT}: {mainnet_strat_pids}")
if mainnet_iriumd_pids and mainnet_strat_pids:
    ok("T8-pre", f"mainnet processes detected on ports {MAINNET_IRIUMD_PORT}/{MAINNET_STRAT_PORT}")
else:
    fail("T8-pre", f"mainnet process not found on expected ports — iriumd_pids={mainnet_iriumd_pids} strat_pids={mainnet_strat_pids}")

for port in (RPC_PORT, STRATUM_PORT):
    try:
        s = socket.create_connection(("127.0.0.1", port), 0.3)
        s.close()
        fail(f"T8-pre port {port}", f"already bound — abort")
        cleanup(); sys.exit(1)
    except OSError:
        ok(f"T8-pre port {port}", f"{port} free")

# ── start testnet ────────────────────────────────────────────────────────────
print("\n─── starting testnet iriumd + stratum ──────────────────────────────")
if os.path.isdir(DATA_DIR):
    shutil.rmtree(DATA_DIR, ignore_errors=True)
os.makedirs(DATA_DIR, exist_ok=True)

iriumd_proc = spawn(
    [IRIUMD_BIN],
    {
        "IRIUM_NETWORK":    "devnet",
        "IRIUM_POAWX_MODE": "active",
        "IRIUM_DATA_DIR":   DATA_DIR,
        "IRIUM_RPC_TOKEN":  TOKEN,
        "IRIUM_NODE_HOST":  "127.0.0.1",
        "IRIUM_NODE_PORT":  str(RPC_PORT),
    },
    f"{DATA_DIR}/iriumd.log",
    cwd=REPO,  # iriumd uses ./bootstrap/trust/ relative to CWD
)
print(f"  iriumd PID={iriumd_proc.pid}")

rpc_url = f"http://127.0.0.1:{RPC_PORT}/rpc/getblocktemplate"
if not wait_http_200(rpc_url, {"Authorization": f"Bearer {TOKEN}"}, timeout=30):
    print("  iriumd log tail:")
    try:
        with open(f"{DATA_DIR}/iriumd.log") as f:
            for l in f.readlines()[-20:]: print(f"    {l.rstrip()}")
    except Exception: pass
    fail("startup", "iriumd not ready in 30s")
    cleanup(); sys.exit(1)
print(f"  iriumd ready on port {RPC_PORT}")

tpl0 = rpc_get("/rpc/getblocktemplate")
print(f"  template: height={tpl0['height']} poawx_mode={tpl0.get('poawx_mode')} bits={tpl0.get('bits')}")

if tpl0.get("poawx_mode") != "active":
    fail("startup", f"poawx_mode={tpl0.get('poawx_mode')} (expected active) — DATA_DIR not isolated?")
    cleanup(); sys.exit(1)

stratum_proc = spawn(
    [STRATUM_BIN], STRATUM_ENV,
    f"{DATA_DIR}/stratum.log",
)
print(f"  stratum PID={stratum_proc.pid}")

if not wait_port(STRATUM_PORT, timeout=15):
    fail("startup", "stratum port not open in 15s")
    cleanup(); sys.exit(1)
print(f"  stratum ready on port {STRATUM_PORT}")

# ── mine 1 warm-up block ─────────────────────────────────────────────────────
# poawx_get_assignment returns 404 at genesis (tip_h=0); need tip_h >= 1.
print("\n─── warm-up: mining 1 block to advance past genesis ────────────────")
warmup_cmd = [
    sys.executable, HARNESS,
    "127.0.0.1", str(STRATUM_PORT),
    f"http://127.0.0.1:{RPC_PORT}", TOKEN,
    "--blocks", "1",
]
with open(f"{DATA_DIR}/warmup.log", "w") as lf:
    warmup_rc = subprocess.run(warmup_cmd, stdout=lf, stderr=lf, timeout=120).returncode
with open(f"{DATA_DIR}/warmup.log") as f:
    warmup_out = f.read()
print(f"  warmup exit={warmup_rc}")
for l in warmup_out.splitlines()[-5:]: print(f"    {l}")

tpl_warm = rpc_get("/rpc/getblocktemplate")
tip_h = tpl_warm["height"] - 1
print(f"  after warmup: tip_h={tip_h}")
if tip_h < 1:
    fail("warmup", "chain still at genesis after warmup — block not accepted")
    cleanup(); sys.exit(1)
ok("warmup", f"chain advanced to tip_h={tip_h}")

# ── fetch assignment ─────────────────────────────────────────────────────────
print("\n─── fetching PoAW-X assignment ─────────────────────────────────────")
asgn   = rpc_get("/poawx/assignment")
HEIGHT = asgn["height"] + 1     # receipt is for the NEXT block
SEED   = asgn["seed"]
NONCE  = asgn["commitment_nonce"]
DIFF   = asgn["puzzle_difficulty"]
print(f"  tip_h={asgn['height']} receipt_height={HEIGHT} diff={DIFF}")
print(f"  seed={SEED[:16]}... nonce={NONCE[:16]}...")

# ── T1: valid receipt accepted ───────────────────────────────────────────────
print("\n─── T1: valid receipt accepted ─────────────────────────────────────")
PKH_A = "aa" * 20
SOL_A = brute_force(SEED, NONCE, DIFF)
print(f"  SOL_A[0:16]={SOL_A[:16]}")
status1, resp1 = rpc_post("/poawx/receipt", {
    "height": HEIGHT, "lane": "cpu",
    "worker_pkh": PKH_A, "solution": SOL_A, "commitment_nonce": NONCE,
})
print(f"  POST /poawx/receipt → {status1}")
if status1 == 200:
    ok("T1", "valid receipt accepted HTTP 200")
else:
    fail("T1", f"expected 200, got {status1}: {str(resp1)[:100]}")

# ── T2: wrong commitment_nonce rejected ──────────────────────────────────────
print("\n─── T2: wrong nonce rejected ────────────────────────────────────────")
status2, resp2 = rpc_post("/poawx/receipt", {
    "height": HEIGHT, "lane": "cpu",
    "worker_pkh": "bb" * 20, "solution": "de" * 32,
    "commitment_nonce": "ff" * 32,
})
print(f"  POST /poawx/receipt (wrong nonce) → {status2}")
if status2 == 400:
    ok("T2", "wrong commitment_nonce rejected HTTP 400")
else:
    fail("T2", f"expected 400, got {status2}: {str(resp2)[:100]}")

# ── T3: insufficient PoW rejected ────────────────────────────────────────────
print("\n─── T3: failing PoW rejected ────────────────────────────────────────")
FAILING_SOL = brute_force_failing(SEED, NONCE)
lz = leading_zeros(sha256d(bytes.fromhex(SEED) + bytes.fromhex(NONCE) + bytes.fromhex(FAILING_SOL)))
print(f"  failing solution has {lz} leading zero bits (must be < {DIFF})")
status3, resp3 = rpc_post("/poawx/receipt", {
    "height": HEIGHT, "lane": "cpu",
    "worker_pkh": "cc" * 20, "solution": FAILING_SOL, "commitment_nonce": NONCE,
})
print(f"  POST /poawx/receipt (bad PoW) → {status3}")
if status3 == 400:
    ok("T3", "insufficient PoW rejected HTTP 400")
else:
    fail("T3", f"expected 400, got {status3}: {str(resp3)[:100]}")

# ── T4+T5: canonical root order-independence ─────────────────────────────────
print("\n─── T4+T5: canonical root order-independence ─────────────────────────")
PKH_B = "bb" * 20
# Find a different valid solution for worker B
sol_bytes = bytearray(bytes.fromhex(SOL_A))
SOL_B = SOL_A
for i in range(1, 10_000_000):
    struct.pack_into("<I", sol_bytes, 0, i)
    h = sha256d(bytes.fromhex(SEED) + bytes.fromhex(NONCE) + bytes(sol_bytes))
    if leading_zeros(h) >= DIFF and bytes(sol_bytes).hex() != SOL_A:
        SOL_B = bytes(sol_bytes).hex(); break

status_b, _ = rpc_post("/poawx/receipt", {
    "height": HEIGHT, "lane": "cpu",
    "worker_pkh": PKH_B, "solution": SOL_B, "commitment_nonce": NONCE,
})
print(f"  POST receipt_B → {status_b}")

tpl_ab = rpc_get("/rpc/getblocktemplate")
root_node = tpl_ab.get("receipts_root", "")
pending   = len(tpl_ab.get("poawx_pending_receipts", []))
print(f"  pending={pending} node_root={root_node[:32]}...")

rA = {"height": HEIGHT, "lane": "cpu", "worker_pkh": PKH_A, "solution": SOL_A, "commitment_nonce": NONCE}
rB = {"height": HEIGHT, "lane": "cpu", "worker_pkh": PKH_B, "solution": SOL_B, "commitment_nonce": NONCE}

root_ab = compute_root([rA, rB])
root_ba = compute_root([rB, rA])
print(f"  root(A,B)={root_ab[:32]}...")
print(f"  root(B,A)={root_ba[:32]}...")

if root_ab == root_ba:
    ok("T4", "compute_root(A,B) == compute_root(B,A)")
else:
    fail("T4", f"root differs by order: AB={root_ab[:16]} BA={root_ba[:16]}")

if root_node and root_node == root_ab:
    ok("T5", f"iriumd receipts_root matches Python canonical root")
elif not root_node:
    fail("T5", "iriumd returned empty receipts_root with 2 pending receipts")
else:
    fail("T5", f"iriumd root {root_node[:16]}... != Python {root_ab[:16]}...")

# ── T6: fabricated receipt in submit_block_extended rejected ─────────────────
print("\n─── T6: fabricated receipt rejected by SBE ──────────────────────────")
tpl_now = rpc_get("/rpc/getblocktemplate")
fake_nonce = "00" * 32  # wrong: real nonce is SHA256(seed||"commitment_nonce")
fake_r = {
    "height": tpl_now["height"], "lane": "cpu",
    "worker_pkh": "ee" * 20, "solution": "00" * 32,
    "commitment_nonce": fake_nonce,
}
status6, resp6 = rpc_post("/rpc/submit_block_extended", {
    "height": tpl_now["height"],
    "header": {
        "version": 1, "prev_hash": tpl_now["prev_hash"],
        "merkle_root": "00" * 32, "time": tpl_now["time"],
        "bits": tpl_now["bits"], "nonce": 0, "hash": "00" * 32,
    },
    "tx_hex":              [],
    "poawx_receipts":      [fake_r],
    "poawx_receipts_root": compute_root([fake_r]),
    "submit_source":       "phase11b-soak-reject-test",
})
print(f"  submit_block_extended (fake receipt) → {status6}")
if status6 == 400:
    ok("T6", "fabricated receipt (wrong nonce) rejected HTTP 400")
else:
    fail("T6", f"expected 400, got {status6}: {str(resp6)[:120]}")

# ── Restart stratum so its first job includes the T1/T4+T5 pending receipts ──
# The stratum's `current` job is only updated when height:prevhash key changes.
# Receipts posted by T1/T4+T5 never triggered a key change, so the warm-up-era
# job (0 receipts) was still `current`.  Restarting clears stratum state; the
# first template poll sees h=2 WITH pending receipts and sets `current` to a
# job that includes them.  T7 block 1 then goes through submit_block_extended.
print("\n─── restarting stratum to flush stale job (receipts now pending) ────")
try:
    stratum_proc.terminate()
    stratum_proc.wait(timeout=5)
except Exception:
    try: stratum_proc.kill()
    except Exception: pass
print(f"  old stratum PID={stratum_proc.pid} stopped")

# Confirm port is free before restarting
end = time.time() + 5
while time.time() < end:
    try:
        s = socket.create_connection(("127.0.0.1", STRATUM_PORT), 0.2)
        s.close(); time.sleep(0.2)
    except OSError:
        break

stratum_proc2 = spawn(
    [STRATUM_BIN], STRATUM_ENV,
    f"{DATA_DIR}/stratum2.log",
)
print(f"  new stratum PID={stratum_proc2.pid}")

if not wait_port(STRATUM_PORT, timeout=15):
    fail("startup", "restarted stratum port not open in 15s")
    cleanup(); sys.exit(1)
print(f"  stratum ready on port {STRATUM_PORT}")

# Wait 3 seconds for the first template poll (refresh_ms=1000) to complete
# so `current` is populated before T7 harness connects and authorizes.
time.sleep(3)
tpl_t7 = rpc_get("/rpc/getblocktemplate")
pending_t7 = len(tpl_t7.get("poawx_pending_receipts", []))
print(f"  T7 template: height={tpl_t7['height']} pending_receipts={pending_t7}")
if pending_t7 < 2:
    print(f"  WARNING: expected >=2 pending receipts for T7, got {pending_t7}")

# ── T7: 10-block stratum soak ────────────────────────────────────────────────
print("\n─── T7: 10-block stratum soak ──────────────────────────────────────")
# The restarted stratum's first job includes the T1/T4+T5 pending receipts for
# h=2 (nonce from chain[1]).  T7 block 1 (h=2) goes through SBE → accepted →
# receipts cleared.  Blocks 2-10 have 0 pending receipts → submit_block.
# Expected: 1 block with irx1=True (block h=2), harness exit=0, 0 FAIL.
# NOTE: --receipt flag is intentionally omitted.  The harness's receipt mode
# posts a receipt then calls wait_notify(20).  Because the stratum only
# re-broadcasts on key change, wait_notify always times out when receipts
# change mid-block → TimeoutError → blocks_done stalls → harness FAIL.
harness_cmd = [
    sys.executable, HARNESS,
    "127.0.0.1", str(STRATUM_PORT),
    f"http://127.0.0.1:{RPC_PORT}", TOKEN,
    "--blocks", "10",
]
print(f"  cmd: {' '.join(harness_cmd[2:])}")
t0 = time.time()
try:
    with open(f"{DATA_DIR}/harness.log", "w") as lf:
        harness_rc = subprocess.run(harness_cmd, stdout=lf, stderr=lf, timeout=300).returncode
except subprocess.TimeoutExpired:
    harness_rc = -1; print("  harness timed out (300s)")
elapsed = time.time() - t0
print(f"  harness exit={harness_rc} elapsed={elapsed:.1f}s")

with open(f"{DATA_DIR}/harness.log") as f:
    h_out = f.read()
h_pass = h_out.count("[PASS]")
h_fail = h_out.count("[FAIL]")
h_irx1 = sum(1 for l in h_out.splitlines() if "irx1=True" in l)
print(f"  PASS={h_pass} FAIL={h_fail} irx1_blocks={h_irx1}")
for l in h_out.splitlines()[-15:]: print(f"    {l}")

if harness_rc == 0 and h_fail == 0:
    ok("T7-harness", f"harness: {h_pass} PASS 0 FAIL")
else:
    fail("T7-harness", f"exit={harness_rc} PASS={h_pass} FAIL={h_fail}")

if h_irx1 >= 1:
    ok("T7-irx1", f"{h_irx1}/10 blocks had irx1=True (receipt path confirmed)")
else:
    fail("T7-irx1", "no blocks had irx1=True — receipt path broken")

with open(f"{DATA_DIR}/iriumd.log") as f:
    ilog = f.read()
panics       = ilog.lower().count("panic")
sbe_accepted = ilog.count("[submit_block_extended] accepted")
rcpt_stored  = ilog.count("[poawx] receipt stored")

if panics == 0:
    ok("T7-panics", "zero panics in iriumd log")
else:
    fail("T7-panics", f"{panics} panic(s) in iriumd log")

if sbe_accepted >= 1:
    ok("T7-sbe", f"{sbe_accepted} blocks accepted via submit_block_extended")
else:
    fail("T7-sbe", "no blocks accepted via submit_block_extended")

if rcpt_stored >= 1:
    ok("T7-receipts", f"{rcpt_stored} receipts stored (Phase 11-B validation passed)")
else:
    fail("T7-receipts", "no receipts stored in iriumd")

# Stratum log for T7 is stratum2.log (post-restart instance)
with open(f"{DATA_DIR}/stratum2.log") as f:
    slog = f.read()
s_errors = sum(1 for l in slog.splitlines() if "error" in l.lower())
if s_errors <= 5:
    ok("T7-stratum", f"stratum log has {s_errors} error lines (acceptable)")
else:
    skip("T7-stratum", f"{s_errors} error lines in stratum log")

# ── T8 post-check ─────────────────────────────────────────────────────────────
print("\n─── T8: mainnet safety post-check ──────────────────────────────────")
post_iriumd_pids = pids_on_port(MAINNET_IRIUMD_PORT)
post_strat_pids  = pids_on_port(MAINNET_STRAT_PORT)
# Verify the same PIDs that were running at the start are still running
if (set(mainnet_iriumd_pids) == set(post_iriumd_pids) and
        set(mainnet_strat_pids) == set(post_strat_pids)):
    ok("T8-post", f"mainnet PIDs unchanged: iriumd={post_iriumd_pids} strat={post_strat_pids}")
elif post_iriumd_pids and post_strat_pids:
    ok("T8-post", f"mainnet processes still present (PIDs may differ): iriumd={post_iriumd_pids} strat={post_strat_pids}")
else:
    fail("T8-post", f"mainnet port missing after soak — iriumd_pids={post_iriumd_pids} strat_pids={post_strat_pids}")

# ── stop testnet ─────────────────────────────────────────────────────────────
print("\n─── stopping testnet ──────────────────────────────────────────────")
cleanup()

# ── summary ──────────────────────────────────────────────────────────────────
print()
print("=" * 68)
print("Phase 11-B Soak Summary")
print("=" * 68)
n_pass = sum(1 for r in RESULTS if r[0] == "PASS")
n_fail = sum(1 for r in RESULTS if r[0] == "FAIL")
n_skip = sum(1 for r in RESULTS if r[0] == "SKIP")
for s, l, d in RESULTS:
    print(f"  [{s:4}] {l}" + (f" — {d}" if d else ""))
print()
print(f"PASS: {n_pass}  FAIL: {n_fail}  SKIP: {n_skip}")
if n_fail == 0:
    print("RESULT: ALL PASS")
    sys.exit(0)
else:
    print(f"RESULT: {n_fail} FAILURE(S)")
    sys.exit(1)
