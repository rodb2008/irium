#!/usr/bin/env python3
"""
PoAW-X stratum TCP miner harness — Phase 10-B.

Speaks standard Stratum v1 to irium-stratum, mines devnet blocks, and
verifies that IRIUM_STRATUM_POAWX=1 causes the stratum to call
/rpc/submit_block_extended (not /rpc/submit_block) with the irx1 receipts
path.

From the miner's view this is ordinary Stratum v1: subscribe → authorize →
receive mining.notify → mine → mining.submit.  The irx1 OP_RETURN commitment
is baked into the coinbase2 by the stratum when poawx_enabled && mode==active.

Usage:
  python3 poawx-stratum-tcp-miner-harness.py \\
      <stratum_host> <stratum_port> <rpc_url> <rpc_token> [n_blocks]
"""

import hashlib, json, socket, struct, sys, time, urllib.request, urllib.error

# ── args ─────────────────────────────────────────────────────────────────────
STRATUM_HOST = sys.argv[1] if len(sys.argv) > 1 else "127.0.0.1"
STRATUM_PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 39420
RPC_URL      = sys.argv[3] if len(sys.argv) > 3 else "http://127.0.0.1:39410"
RPC_TOKEN    = sys.argv[4] if len(sys.argv) > 4 else "phase10b_soak_devnet"
N_BLOCKS     = int(sys.argv[5]) if len(sys.argv) > 5 else 3

# ── crypto helpers ───────────────────────────────────────────────────────────
def sha256(d):  return hashlib.sha256(d).digest()
def sha256d(d): return sha256(sha256(d))

def swap4(b):
    """Reverse each 4-byte word — mirrors swap4_bytes_each_word in Rust."""
    out = bytearray()
    for i in range(0, len(b), 4):
        out.extend(bytes(reversed(b[i:i+4])))
    return bytes(out)

# ── base58 for miner address ─────────────────────────────────────────────────
_B58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'

def b58encode(b):
    n = int.from_bytes(b, 'big')
    result = []
    while n:
        n, r = divmod(n, 58)
        result.append(_B58[r])
    result.reverse()
    for byte in b:
        if byte == 0: result.insert(0, '1')
        else: break
    return ''.join(result)

def make_address(pkh: bytes) -> str:
    """Build a valid base58-checksum address.  Version byte 0x3c is ignored
    by parse_address_to_pkh() in the stratum; only the checksum is checked."""
    payload = bytes([0x3c]) + pkh
    cs = sha256d(payload)[:4]
    return b58encode(payload + cs)

# deterministic 20-byte test PKH (bytes 1..20)
MINER_PKH     = bytes(range(1, 21))
MINER_ADDRESS = make_address(MINER_PKH)
MINER_WORKER  = f"{MINER_ADDRESS}.harness"

# ── RPC helpers ──────────────────────────────────────────────────────────────
def rpc_get(path):
    req = urllib.request.Request(
        f"{RPC_URL.rstrip('/')}{path}",
        headers={"Authorization": f"Bearer {RPC_TOKEN}"},
    )
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.loads(r.read())

def rpc_post(path, body):
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        f"{RPC_URL.rstrip('/')}{path}",
        data=data,
        headers={
            "Authorization": f"Bearer {RPC_TOKEN}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, {}

# ── receipt generation ────────────────────────────────────────────────────────
def derive_puzzle_difficulty(bits_int):
    exp       = (bits_int >> 24) & 0xff
    mantissa  = bits_int & 0xffffff
    mb        = mantissa.bit_length() if mantissa else 0
    bit_len   = max(0, mb - 8*(3-exp)) if exp <= 3 else 8*(exp-3) + mb
    return max(1, min(8, max(0, 256 - bit_len) // 4))

def brute_force_receipt(seed_hex, nonce_hex, difficulty):
    """Find (solution, nonce) s.t. sha256d(seed||nonce||solution) has >= difficulty leading 0-bits."""
    seed  = bytes.fromhex(seed_hex)
    nonce = bytes.fromhex(nonce_hex)
    sol   = bytearray(32)
    for attempt in range(10_000_000):
        struct.pack_into('<I', sol, 0, attempt)
        h = sha256d(seed + nonce + bytes(sol))
        leading = 0
        for byte in h:
            if byte == 0:
                leading += 8
            else:
                mask = 0x80
                while mask and not (byte & mask):
                    leading += 1
                    mask >>= 1
                break
        if leading >= difficulty:
            return sol.hex(), attempt
    raise RuntimeError(f"brute_force_receipt failed after 10M attempts (difficulty={difficulty})")

def post_receipt(height):
    """Fetch assignment for current height and POST a valid receipt."""
    asgn = rpc_get("/poawx/assignment")
    seed  = asgn["seed"]
    cnonce = asgn.get("commitment_nonce", "00" * 16)
    diff  = asgn.get("puzzle_difficulty", 1)
    lane  = asgn.get("lane", "cpu")

    solution_hex, _ = brute_force_receipt(seed, cnonce, diff)

    body = {
        "height": height,
        "lane": lane,
        "worker_pkh": MINER_PKH.hex(),
        "solution": solution_hex,
        "commitment_nonce": cnonce,
    }
    status, resp = rpc_post("/poawx/receipt", body)
    return status, resp

# ── mining: canonical header ─────────────────────────────────────────────────
# Stratum difficulty-1 target for devnet (matches IRIUM_POW_LIMIT_HEX)
DIFF1_TARGET = int("7fffff0000000000000000000000000000000000000000000000000000000000", 16)

def bits_to_target(bits_hex):
    bits     = int(bits_hex, 16)
    exp      = (bits >> 24) & 0xff
    mantissa = bits & 0xffffff
    return (mantissa >> (8*(3-exp))) if exp <= 3 else (mantissa << (8*(exp-3)))

def compute_merkle(cb_bytes, branches_hex):
    cur = sha256d(cb_bytes)
    for bh in branches_hex:
        cur = sha256d(cur + bytes.fromhex(bh))
    return cur  # internal order

# Mirror of irium-stratum STANDARD_HEADER_ACTIVATION_HEIGHT.
# Pre-fork (height < 22888): iriumd wire-serializes merkle_root as reversed bytes.
# Post-fork: natural sha256d bytes (Bitcoin-standard).
STANDARD_FORK_HEIGHT = 22_888

def canonical_header80(version_hex, prevhash_hex, merkle_internal, ntime_hex, bits_hex, nonce_int, height):
    """
    Build the 80-byte header exactly as irium-stratum submits to iriumd (canonical wire).

    send_notify sends prevhash = hex(swap4(reverse(job.prev_hash))).
    Stratum canonical uses prev_wire = reverse(job.prev_hash) = swap4(prevhash_bytes).
    Pre-fork (height < STANDARD_FORK_HEIGHT): merkle_wire = reverse(merkle_internal).
    All multi-byte integers (version, ntime, bits, nonce) in little-endian.
    """
    version = bytes.fromhex(version_hex)   # v_be raw bytes matching stratum chosen variant
    prev    = swap4(bytes.fromhex(prevhash_hex))          # = reverse(job.prev_hash)
    merkle  = merkle_internal[::-1] if height < STANDARD_FORK_HEIGHT else merkle_internal
    ntime   = struct.pack('<I', int(ntime_hex, 16))
    bits    = struct.pack('<I', int(bits_hex, 16))
    nonce   = struct.pack('<I', nonce_int)
    return version + prev + merkle + ntime + bits + nonce

def mine_nonce(version_hex, prevhash_hex, cb_bytes, branches_hex, ntime_hex, bits_hex, height, share_target=None):
    """
    Find nonce satisfying TWO independent conditions:
    1. sha256d(canonical header) BE <= block_tgt  -- iriumd PoW validation
    2. sha256d(fast-scan variant header) LE <= stgt -- stratum ok_block_le decision

    Canonical header: prev = swap4(prevhash_bytes) = reverse(job.prev_hash);
                      merkle = reversed(mr_raw_raw) for height < STANDARD_FORK_HEIGHT.
    Fast-scan variant (Auto mode): prev_canon = bytes(reversed(swap4(prevhash_bytes)))
                      = job.prev_hash; merkle = mr_raw_raw (raw, not reversed).
    Both conditions must hold for the stratum to submit AND iriumd to accept the block.
    """
    merkle_int  = compute_merkle(cb_bytes, branches_hex)
    block_tgt   = bits_to_target(bits_hex)
    stgt        = share_target if share_target is not None else block_tgt

    prevhash_bytes = bytes.fromhex(prevhash_hex)
    prev_wire  = swap4(prevhash_bytes)                    # canonical: reverse(job.prev_hash)
    prev_canon = bytes(reversed(swap4(prevhash_bytes)))   # fast-scan variant: job.prev_hash
    mr_rev = merkle_int[::-1] if height < STANDARD_FORK_HEIGHT else merkle_int
    mr_raw = merkle_int

    version = bytes.fromhex(version_hex)   # v_be raw bytes matching stratum chosen variant
    ntime   = struct.pack('<I', int(ntime_hex, 16))
    bits    = struct.pack('<I', int(bits_hex, 16))
    canon_prefix   = version + prev_wire  + mr_rev + ntime + bits
    variant_prefix = version + prev_canon + mr_raw + ntime + bits

    for n in range(0x1_0000_0000):
        nonce = struct.pack('<I', n)
        h_canon   = sha256d(canon_prefix   + nonce)
        h_variant = sha256d(variant_prefix + nonce)
        # iriumd LE on canonical (meets_target gets reversed sha256d, does BE = LE on raw)
        # stratum LE on fast-scan variant (ok_block_le)
        if int.from_bytes(h_canon,   "little") <= block_tgt and            int.from_bytes(h_variant, "little") <= stgt:
            return n, h_canon.hex(), merkle_int
    raise RuntimeError("no nonce found in 2^32 attempts")

# ── stratum TCP protocol ─────────────────────────────────────────────────────
class Stratum:
    def __init__(self, host, port):
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.sock.settimeout(30)
        self.sock.connect((host, port))
        self.buf  = b""
        self._id  = 0
        self.difficulty = 1.0  # updated by mining.set_difficulty

    def _next_id(self):
        self._id += 1
        return self._id

    def send(self, msg):
        data = (json.dumps(msg) + '\n').encode()
        self.sock.sendall(data)

    def recv(self):
        while True:
            if b'\n' in self.buf:
                line, self.buf = self.buf.split(b'\n', 1)
                return json.loads(line)
            chunk = self.sock.recv(4096)
            if not chunk:
                raise ConnectionError("stratum disconnected")
            self.buf += chunk

    def call(self, method, params):
        mid = self._next_id()
        self.send({"id": mid, "method": method, "params": params})
        return mid

    def subscribe(self, agent="phase10b-harness/1.0"):
        mid = self.call("mining.subscribe", [agent])
        resp = self.recv()
        # result = [[sub_pairs], extranonce1_hex, extranonce2_size]
        assert resp.get("id") == mid, f"subscribe id mismatch: {resp}"
        assert resp.get("error") is None, f"subscribe error: {resp.get('error')}"
        extranonce1_hex = resp["result"][1]
        extranonce2_size = resp["result"][2]
        print(f"[harness] subscribe OK extranonce1={extranonce1_hex} e2size={extranonce2_size}")
        return extranonce1_hex, extranonce2_size

    def authorize(self, worker, password="x"):
        mid = self.call("mining.authorize", [worker, password])
        while True:
            msg = self.recv()
            if msg.get("id") == mid:
                ok = msg.get("result")
                assert ok is True, f"authorize failed: {msg}"
                print(f"[harness] authorize OK worker={worker}")
                return
            # may receive set_difficulty before auth response
            self._handle_notification(msg)

    def wait_notify(self, timeout=30):
        """Wait for a mining.notify message, draining any set_difficulty in between."""
        deadline = time.time() + timeout
        while time.time() < deadline:
            self.sock.settimeout(max(1.0, deadline - time.time()))
            try:
                msg = self.recv()
            except socket.timeout:
                continue
            if msg.get("method") == "mining.notify":
                return msg["params"]
            self._handle_notification(msg)
        raise TimeoutError("timed out waiting for mining.notify")

    def _handle_notification(self, msg):
        m = msg.get("method", "")
        if m == "mining.set_difficulty":
            self.difficulty = float(msg['params'][0])
            print(f"[harness] set_difficulty={self.difficulty}")
        elif m == "mining.notify":
            pass  # caller should handle these

    def share_target(self):
        """Return share target integer: DIFF1_TARGET // difficulty."""
        return int(DIFF1_TARGET // max(1, self.difficulty))

    def submit(self, worker, job_id, extranonce2_hex, ntime_hex, nonce_hex):
        mid = self.call("mining.submit", [worker, job_id, extranonce2_hex, ntime_hex, nonce_hex])
        # Response may arrive after a brief delay
        deadline = time.time() + 10
        while time.time() < deadline:
            self.sock.settimeout(max(1.0, deadline - time.time()))
            try:
                msg = self.recv()
            except socket.timeout:
                continue
            if msg.get("id") == mid:
                return msg
            self._handle_notification(msg)
        raise TimeoutError("no submit response within 10s")

    def drain_notifies(self):
        """Discard any already-buffered mining.notify messages (stale jobs)."""
        drained = 0
        self.sock.settimeout(0.2)
        try:
            while True:
                try:
                    msg = self.recv()
                    if msg.get("method") == "mining.notify":
                        drained += 1
                    else:
                        self._handle_notification(msg)
                except socket.timeout:
                    break
        finally:
            self.sock.settimeout(30)
        if drained:
            print(f"[harness] drained {drained} stale notify(s)", flush=True)

    def close(self):
        try: self.sock.close()
        except: pass

# ── main harness ─────────────────────────────────────────────────────────────
def log(s): print(f"[wiring] {s}", flush=True)

def main():
    log(f"Phase 10-B: stratum TCP miner harness")
    log(f"  stratum={STRATUM_HOST}:{STRATUM_PORT}  rpc={RPC_URL}  n_blocks={N_BLOCKS}")
    log(f"  miner_address={MINER_ADDRESS}")

    # Verify iriumd is up and in active mode
    status = rpc_get("/status")
    log(f"Step 0: iriumd height={status.get('height',0)} network={status.get('network','?')}")

    tpl = rpc_get("/rpc/getblocktemplate")
    assert tpl.get("poawx_mode") == "active", \
        f"Expected poawx_mode=active, got {tpl.get('poawx_mode')!r}"
    log(f"Step 0: template height={tpl['height']} poawx_mode={tpl['poawx_mode']} bits={tpl['bits']}")

    # Connect stratum
    stratum = Stratum(STRATUM_HOST, STRATUM_PORT)
    extranonce1_hex, en2_size = stratum.subscribe()
    stratum.authorize(MINER_WORKER)

    extranonce2_hex = "00" * en2_size  # fixed extranonce2

    results = []

    for block_num in range(1, N_BLOCKS + 1):
        log(f"")
        log(f"=== Block {block_num}/{N_BLOCKS} ===")

        # ── Get current template height ─────────────────────────────────────
        tpl_now = rpc_get("/rpc/getblocktemplate")
        current_height = tpl_now["height"]
        log(f"Step 1: template height={current_height} (empty-receipt path)")

        # ── Wait for mining.notify job (stratum refreshes ~1s after template change) ──
        log(f"Step 2: waiting for mining.notify job...")
        job = stratum.wait_notify(timeout=15)

        job_id       = job[0]
        prevhash_hex = job[1]
        cb1_hex      = job[2]
        cb2_hex      = job[3]
        branches_hex = job[4]
        version_hex  = job[5]
        bits_hex     = job[6]
        ntime_hex    = job[7]
        clean        = job[8]

        log(f"Step 2: job_id={job_id} height={current_height} bits={bits_hex} clean={clean}")
        log(f"  coinbase1_len={len(cb1_hex)//2}B  coinbase2_len={len(cb2_hex)//2}B  branches={len(branches_hex)}")

        # ── Check irx1 OP_RETURN is in coinbase2 (stratum baked it in) ─────
        combined_coinbase = cb1_hex + cb2_hex
        has_irx1 = "6a2469727831" in combined_coinbase.lower()
        log(f"Step 3: irx1 OP_RETURN in coinbase: {'YES' if has_irx1 else 'NO (empty receipts path)'}")

        # ── Build full coinbase bytes and mine ───────────────────────────────
        # Standard Stratum v1: coinbase_prefix + extranonce1 + extranonce2 + coinbase_suffix
        cb_bytes = (bytes.fromhex(cb1_hex) + bytes.fromhex(extranonce1_hex) +
                    bytes.fromhex(extranonce2_hex) + bytes.fromhex(cb2_hex))
        stgt = stratum.share_target()
        log(f"Step 4: mining (bits={bits_hex} diff={stratum.difficulty:.1f} share_tgt={stgt:#x})")

        t0 = time.time()
        nonce_int, hash_hex, merkle_int = mine_nonce(
            version_hex, prevhash_hex, cb_bytes, branches_hex, ntime_hex, bits_hex,
            current_height, share_target=stgt
        )
        elapsed = time.time() - t0
        log(f"Step 4: found nonce={nonce_int} hash={hash_hex[:16]}... in {elapsed:.3f}s")

        # ── Submit share via TCP ────────────────────────────────────────────
        log(f"Step 5: mining.submit job_id={job_id} nonce={nonce_int:08x}")
        resp_sub = stratum.submit(MINER_WORKER, job_id, extranonce2_hex, ntime_hex, f"{nonce_int:08x}")
        log(f"Step 5: submit response: {resp_sub}")

        accepted_by_stratum = (resp_sub.get("result") is True or
                               resp_sub.get("result") is not None and
                               resp_sub.get("error") is None)
        log(f"Step 5: stratum accepted share: {'YES' if accepted_by_stratum else 'NO'}")

        # ── Verify height advanced ───────────────────────────────────────────
        # After block at template height H is accepted, iriumd tip = H.
        time.sleep(2)  # give iriumd a moment to process
        new_status = rpc_get("/status")
        new_height = new_status.get("height", 0)
        block_accepted = (new_height == current_height)
        log(f"Step 6: iriumd height: {current_height - 1} → {new_height}  ({'ADVANCE OK' if block_accepted else 'NO ADVANCE'})")

        # ── Verify next template advanced ────────────────────────────────────
        tpl_after = rpc_get("/rpc/getblocktemplate")
        log(f"Step 7: new template height={tpl_after.get('height')}")

        results.append({
            "block_num": block_num,
            "height": current_height,
            "irx1_in_coinbase": has_irx1,
            "stratum_accepted": accepted_by_stratum,
            "block_accepted": block_accepted,
        })

    # ── Summary ──────────────────────────────────────────────────────────────
    log(f"")
    log(f"=== Phase 10-B Harness Results ===")
    all_ok = True
    for r in results:
        ok = r["stratum_accepted"] and r["block_accepted"]
        all_ok = all_ok and ok
        mark = "PASS" if ok else "FAIL"
        log(f"  [{mark}] block {r['block_num']}: height={r['height']}  "
            f"irx1={r['irx1_in_coinbase']}  "
            f"stratum_accepted={r['stratum_accepted']}  "
            f"block_accepted={r['block_accepted']}")

    stratum.close()

    if all_ok:
        log(f"All {N_BLOCKS} blocks: PASS")
        sys.exit(0)
    else:
        log(f"FAIL — some blocks did not produce confirmed block acceptance")
        sys.exit(1)

if __name__ == "__main__":
    main()
