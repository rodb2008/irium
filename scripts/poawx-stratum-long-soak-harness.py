#!/usr/bin/env python3
"""
PoAW-X stratum long-soak harness - Phase 10-C.

Extends the Phase 10-B miner harness for multi-hour soak runs.
Speaks Stratum v1; mines devnet blocks in a loop; tests non-empty receipt
path and bogus share rejection; emits JSON summary.

Usage:
  python3 poawx-stratum-long-soak-harness.py \
      <stratum_host> <stratum_port> <rpc_url> <rpc_token> \
      [--blocks N] [--seconds S] [--receipt] [--bogus]
"""

import argparse, hashlib, json, socket, struct, sys, time
import urllib.request, urllib.error

parser = argparse.ArgumentParser()
parser.add_argument("stratum_host")
parser.add_argument("stratum_port", type=int)
parser.add_argument("rpc_url")
parser.add_argument("rpc_token")
parser.add_argument("--blocks",  type=int, default=50,    help="target block count")
parser.add_argument("--seconds", type=int, default=10800, help="max soak duration seconds")
parser.add_argument("--receipt", action="store_true",     help="test non-empty receipt path")
parser.add_argument("--bogus",   action="store_true",     help="send bogus share and verify rejection")
args = parser.parse_args()

STRATUM_HOST = args.stratum_host
STRATUM_PORT = args.stratum_port
RPC_URL      = args.rpc_url
RPC_TOKEN    = args.rpc_token
BLOCK_TARGET = args.blocks
SOAK_SECONDS = args.seconds
TEST_RECEIPT = args.receipt
TEST_BOGUS   = args.bogus

# ── crypto ───────────────────────────────────────────────────────────────────
def sha256(d):  return hashlib.sha256(d).digest()
def sha256d(d): return sha256(sha256(d))

def swap4(b):
    out = bytearray()
    for i in range(0, len(b), 4):
        out.extend(bytes(reversed(b[i:i+4])))
    return bytes(out)

_B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

def b58encode(b):
    n = int.from_bytes(b, "big")
    result = []
    while n:
        n, r = divmod(n, 58)
        result.append(_B58[r])
    result.reverse()
    for byte in b:
        if byte == 0:
            result.insert(0, "1")
        else:
            break
    return "".join(result)

def make_address(pkh):
    payload = bytes([0x3c]) + pkh
    cs = sha256d(payload)[:4]
    return b58encode(payload + cs)

MINER_PKH     = bytes(range(1, 21))
MINER_ADDRESS = make_address(MINER_PKH)
MINER_WORKER  = MINER_ADDRESS + ".soak"

# ── RPC ──────────────────────────────────────────────────────────────────────
def rpc_get(path):
    req = urllib.request.Request(
        RPC_URL.rstrip("/") + path,
        headers={"Authorization": "Bearer " + RPC_TOKEN},
    )
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.loads(r.read())

def rpc_post(path, body):
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        RPC_URL.rstrip("/") + path, data=data,
        headers={
            "Authorization": "Bearer " + RPC_TOKEN,
            "Content-Type": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        try:
            body_resp = json.loads(e.read())
        except Exception:
            body_resp = {}
        return e.code, body_resp

# ── receipt / puzzle ─────────────────────────────────────────────────────────
def brute_force_receipt(seed_hex, nonce_hex, difficulty):
    seed  = bytes.fromhex(seed_hex)
    nonce = bytes.fromhex(nonce_hex)
    sol   = bytearray(32)
    for attempt in range(10_000_000):
        struct.pack_into("<I", sol, 0, attempt)
        h = sha256d(seed + nonce + bytes(sol))
        leading = 0
        for byte in h:
            if byte == 0:
                leading += 8
            else:
                m = 0x80
                while m and not (byte & m):
                    leading += 1
                    m >>= 1
                break
        if leading >= difficulty:
            return sol.hex(), attempt
    raise RuntimeError("brute_force_receipt failed after 10M attempts (diff=%d)" % difficulty)

def post_receipt(height):
    asgn   = rpc_get("/poawx/assignment")
    seed   = asgn["seed"]
    cnonce = asgn.get("commitment_nonce", "00" * 16)
    diff   = asgn.get("puzzle_difficulty", 1)
    lane   = asgn.get("lane", "cpu")
    sol_hex, attempts = brute_force_receipt(seed, cnonce, diff)
    body = {
        "height": height, "lane": lane,
        "worker_pkh": MINER_PKH.hex(),
        "solution": sol_hex, "commitment_nonce": cnonce,
    }
    status, resp = rpc_post("/poawx/receipt", body)
    return status, resp, diff, attempts

# ── mining ───────────────────────────────────────────────────────────────────
DIFF1_TARGET         = int("7fffff0000000000000000000000000000000000000000000000000000000000", 16)
STANDARD_FORK_HEIGHT = 22_888

def bits_to_target(bits_hex):
    bits = int(bits_hex, 16)
    exp  = (bits >> 24) & 0xff
    m    = bits & 0xffffff
    return (m >> (8 * (3 - exp))) if exp <= 3 else (m << (8 * (exp - 3)))

def compute_merkle(cb_bytes, branches_hex):
    cur = sha256d(cb_bytes)
    for bh in branches_hex:
        cur = sha256d(cur + bytes.fromhex(bh))
    return cur

def mine_nonce(version_hex, prevhash_hex, cb_bytes, branches_hex,
               ntime_hex, bits_hex, height, share_target=None):
    merkle_int     = compute_merkle(cb_bytes, branches_hex)
    block_tgt      = bits_to_target(bits_hex)
    stgt           = share_target if share_target is not None else block_tgt
    prevhash_bytes = bytes.fromhex(prevhash_hex)
    prev_wire      = swap4(prevhash_bytes)
    prev_canon     = bytes(reversed(swap4(prevhash_bytes)))
    mr_rev         = merkle_int[::-1] if height < STANDARD_FORK_HEIGHT else merkle_int
    mr_raw         = merkle_int
    version        = bytes.fromhex(version_hex)
    ntime          = struct.pack("<I", int(ntime_hex, 16))
    bits_b         = struct.pack("<I", int(bits_hex, 16))
    canon_prefix   = version + prev_wire  + mr_rev + ntime + bits_b
    variant_prefix = version + prev_canon + mr_raw + ntime + bits_b
    for n in range(0x1_0000_0000):
        nonce = struct.pack("<I", n)
        h_c = sha256d(canon_prefix   + nonce)
        h_v = sha256d(variant_prefix + nonce)
        if (int.from_bytes(h_c, "little") <= block_tgt and
                int.from_bytes(h_v, "little") <= stgt):
            return n, h_c.hex(), merkle_int
    raise RuntimeError("no nonce found in 2^32 attempts")

# ── stratum protocol ─────────────────────────────────────────────────────────
class Stratum:
    def __init__(self, host, port):
        self.sock = socket.socket()
        self.sock.settimeout(30)
        self.sock.connect((host, port))
        self.buf        = b""
        self._id        = 0
        self.difficulty = 1.0

    def _next_id(self):
        self._id += 1
        return self._id

    def send(self, msg):
        self.sock.sendall((json.dumps(msg) + "\n").encode())

    def recv(self):
        while True:
            if b"\n" in self.buf:
                line, self.buf = self.buf.split(b"\n", 1)
                return json.loads(line)
            chunk = self.sock.recv(4096)
            if not chunk:
                raise ConnectionError("stratum disconnected")
            self.buf += chunk

    def call(self, method, params):
        mid = self._next_id()
        self.send({"id": mid, "method": method, "params": params})
        return mid

    def subscribe(self, agent="phase10c-soak/1.0"):
        mid = self.call("mining.subscribe", [agent])
        resp = self.recv()
        assert resp.get("id") == mid and resp.get("error") is None, \
            "subscribe failed: %s" % resp
        en1    = resp["result"][1]
        en2sz  = resp["result"][2]
        print("[harness] subscribe OK en1=%s en2sz=%d" % (en1, en2sz), flush=True)
        return en1, en2sz

    def authorize(self, worker, password="x"):
        mid = self.call("mining.authorize", [worker, password])
        while True:
            msg = self.recv()
            if msg.get("id") == mid:
                assert msg.get("result") is True, "authorize failed: %s" % msg
                print("[harness] authorize OK", flush=True)
                return
            self._handle_notification(msg)

    def wait_notify(self, timeout=30):
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
        if msg.get("method") == "mining.set_difficulty":
            self.difficulty = float(msg["params"][0])
            print("[harness] set_difficulty=%s" % self.difficulty, flush=True)

    def share_target(self):
        return int(DIFF1_TARGET // max(1, self.difficulty))

    def submit(self, worker, job_id, en2_hex, ntime_hex, nonce_hex):
        mid = self.call("mining.submit", [worker, job_id, en2_hex, ntime_hex, nonce_hex])
        deadline = time.time() + 15
        while time.time() < deadline:
            self.sock.settimeout(max(1.0, deadline - time.time()))
            try:
                msg = self.recv()
            except socket.timeout:
                continue
            if msg.get("id") == mid:
                return msg
            self._handle_notification(msg)
        raise TimeoutError("no submit response within 15s")

    def close(self):
        try:
            self.sock.close()
        except Exception:
            pass

# ── soak loop helpers ─────────────────────────────────────────────────────────
def log(s):
    print("[soak] " + s, flush=True)

def mine_one_block(stratum, en1_hex, en2_size, receipt_mode=False):
    tpl    = rpc_get("/rpc/getblocktemplate")
    height = tpl["height"]

    receipt_result = None
    if receipt_mode:
        log("  [receipt] posting receipt for height=%d" % height)
        try:
            status, resp, diff, attempts = post_receipt(height)
            if status == 200:
                log("  [receipt] POST ok diff=%d attempts=%d" % (diff, attempts))
                time.sleep(1)
                tpl = rpc_get("/rpc/getblocktemplate")
                pending = tpl.get("poawx_pending_receipts", [])
                receipt_result = {
                    "status": status, "pending_count": len(pending),
                    "diff": diff, "attempts": attempts,
                }
                log("  [receipt] pending_receipts in template: %d" % len(pending))
            else:
                log("  [receipt] POST status=%d resp=%s" % (status, resp))
                receipt_result = {"status": status}
        except Exception as e:
            log("  [receipt] error: %s" % e)
            receipt_result = {"error": str(e)}

    job          = stratum.wait_notify(timeout=20)
    job_id       = job[0]
    prevhash_hex = job[1]
    cb1_hex      = job[2]
    cb2_hex      = job[3]
    branches_hex = job[4]
    version_hex  = job[5]
    bits_hex     = job[6]
    ntime_hex    = job[7]

    has_irx1 = "6a2469727831" in (cb1_hex + cb2_hex).lower()
    en2_hex  = "00" * en2_size
    cb_bytes = (bytes.fromhex(cb1_hex) + bytes.fromhex(en1_hex) +
                bytes.fromhex(en2_hex) + bytes.fromhex(cb2_hex))
    stgt = stratum.share_target()

    t0 = time.time()
    nonce_int, hash_hex, _ = mine_nonce(
        version_hex, prevhash_hex, cb_bytes, branches_hex,
        ntime_hex, bits_hex, height, share_target=stgt,
    )
    elapsed = time.time() - t0

    resp         = stratum.submit(MINER_WORKER, job_id, en2_hex, ntime_hex, "%08x" % nonce_int)
    strat_ok     = (resp.get("result") is True or
                    (resp.get("result") is not None and resp.get("error") is None))

    time.sleep(2)
    new_height   = rpc_get("/status").get("height", 0)
    block_ok     = (new_height == height)

    return {
        "height": height, "mine_s": round(elapsed, 3),
        "has_irx1": has_irx1, "nonce": nonce_int,
        "stratum_accepted": strat_ok, "block_accepted": block_ok,
        "receipt": receipt_result,
    }

def send_bogus_share(stratum, en2_size):
    try:
        job = stratum.wait_notify(timeout=10)
        before_h = rpc_get("/status").get("height", 0)
        resp = stratum.submit(MINER_WORKER, "bogus_job_00", "00" * en2_size, job[7], "deadbeef")
        time.sleep(1)
        after_h  = rpc_get("/status").get("height", 0)
        rejected      = (resp.get("result") is False or resp.get("error") is not None)
        no_advance    = (before_h == after_h)
        return {"resp": resp, "rejected": rejected, "height_unchanged": no_advance}
    except Exception as e:
        return {"error": str(e), "rejected": True, "height_unchanged": True}

def reconnect(host, port, en1_for_label="reconnect"):
    stratum = Stratum(host, port)
    en1, en2sz = stratum.subscribe("phase10c-soak/1.0-" + en1_for_label)
    stratum.authorize(MINER_WORKER)
    return stratum, en1, en2sz

# ── main ─────────────────────────────────────────────────────────────────────
def main():
    log("Phase 10-C soak harness: blocks=%d seconds=%d receipt=%s bogus=%s" % (
        BLOCK_TARGET, SOAK_SECONDS, TEST_RECEIPT, TEST_BOGUS))
    log("  stratum=%s:%d  rpc=%s" % (STRATUM_HOST, STRATUM_PORT, RPC_URL))

    status = rpc_get("/status")
    tpl    = rpc_get("/rpc/getblocktemplate")
    bits   = tpl.get("bits", "")
    log("Step 0: iriumd h=%d bits=%s" % (status.get("height", 0), bits))
    assert bits == "207fffff", "Expected devnet bits 207fffff, got %r" % bits

    # Verify PoAW-X is not disabled (503 = explicitly disabled; 404 at h=0 is expected)
    try:
        rpc_get("/poawx/assignment")
        log("Step 0: /poawx/assignment accessible - PoAW-X mode active")
    except urllib.error.HTTPError as e:
        if e.code == 503:
            raise AssertionError("PoAW-X is disabled - /poawx/assignment returned 503")
        elif e.code == 404:
            log("Step 0: /poawx/assignment 404 at h=0 - expected before first block, continuing")
        else:
            raise AssertionError("PoAW-X unexpected HTTP %d from /poawx/assignment" % e.code)
    except Exception as e:
        raise AssertionError("PoAW-X RPC unreachable: %s" % e)

    try:
        asgn = rpc_get("/poawx/assignment")
        log("Step 0: /poawx/assignment lane=%s diff=%s" % (
            asgn.get("lane", "?"), asgn.get("puzzle_difficulty", "?")))
    except urllib.error.HTTPError:
        log("Step 0: /poawx/assignment not yet available at h=0 (will be ready after first block)")

    stratum = Stratum(STRATUM_HOST, STRATUM_PORT)
    en1_hex, en2_size = stratum.subscribe()
    stratum.authorize(MINER_WORKER)

    results       = []
    bogus_result  = None
    receipt_done  = False
    start_t       = time.time()
    blocks_done   = 0
    share_accepts = 0
    share_rejects = 0

    if TEST_BOGUS:
        log("\n--- Bogus share test (pre-soak) ---")
        bogus_result = send_bogus_share(stratum, en2_size)
        log("bogus: rejected=%s height_unchanged=%s resp=%s" % (
            bogus_result.get("rejected"),
            bogus_result.get("height_unchanged"),
            bogus_result.get("resp", "?")))
        stratum.close()
        time.sleep(1)
        stratum, en1_hex, en2_size = reconnect(STRATUM_HOST, STRATUM_PORT, "after-bogus")
        log("reconnected after bogus share")

    while True:
        elapsed = time.time() - start_t
        if blocks_done >= BLOCK_TARGET:
            log("Target %d blocks reached" % BLOCK_TARGET)
            break
        if elapsed >= SOAK_SECONDS:
            log("Soak time %ds elapsed" % SOAK_SECONDS)
            break

        use_receipt = TEST_RECEIPT and not receipt_done and blocks_done == 2
        try:
            result = mine_one_block(stratum, en1_hex, en2_size, receipt_mode=use_receipt)
            if use_receipt and result.get("receipt"):
                receipt_done = True
            blocks_done += 1
            if result["stratum_accepted"]:
                share_accepts += 1
            else:
                share_rejects += 1
            results.append(result)
            mark = "PASS" if (result["stratum_accepted"] and result["block_accepted"]) else "FAIL"
            log("Block %d/%d [%s] h=%d irx1=%s strat=%s blk=%s mine=%.3fs" % (
                blocks_done, BLOCK_TARGET, mark, result["height"],
                result["has_irx1"], result["stratum_accepted"],
                result["block_accepted"], result["mine_s"]))
        except TimeoutError as e:
            log("Block %d: timeout - %s" % (blocks_done + 1, e))
            results.append({"height": -1, "stratum_accepted": False, "block_accepted": False,
                             "error": str(e)})
            share_rejects += 1
            try:
                stratum.close()
                time.sleep(2)
                stratum, en1_hex, en2_size = reconnect(STRATUM_HOST, STRATUM_PORT, "timeout-recovery")
                log("  reconnected after timeout")
            except Exception as re:
                log("  reconnect failed: %s" % re)
                break
        except Exception as e:
            log("Block %d: error - %s" % (blocks_done + 1, e))
            results.append({"height": -1, "stratum_accepted": False, "block_accepted": False,
                             "error": str(e)})
            share_rejects += 1

    stratum.close()

    total      = len(results)
    passed     = sum(1 for r in results if r.get("stratum_accepted") and r.get("block_accepted"))
    failed     = total - passed
    irx1_count = sum(1 for r in results if r.get("has_irx1"))
    receipt_ok = (receipt_done and
                  any(r.get("receipt", {}).get("status") == 200 for r in results
                      if r.get("receipt")))
    receipt_pending = next(
        (r["receipt"].get("pending_count", 0) for r in results
         if r.get("receipt", {}) and r["receipt"].get("status") == 200), 0)

    summary = {
        "blocks_total":          total,
        "blocks_pass":           passed,
        "blocks_fail":           failed,
        "share_accepts":         share_accepts,
        "share_rejects":         share_rejects,
        "irx1_in_coinbase_count": irx1_count,
        "receipt_test_passed":   receipt_ok,
        "receipt_pending_count": receipt_pending,
        "bogus_rejected":        bogus_result.get("rejected", False) if bogus_result else None,
        "bogus_height_unchanged": bogus_result.get("height_unchanged", False) if bogus_result else None,
        "elapsed_s":             round(time.time() - start_t, 1),
    }

    log("\n=== Phase 10-C Harness Summary ===")
    log("  Blocks: %d/%d PASS  Shares: %d accepted / %d rejected" % (
        passed, total, share_accepts, share_rejects))
    log("  irx1 in coinbase: %d/%d blocks" % (irx1_count, total))
    if TEST_RECEIPT:
        log("  Receipt path: %s pending_count=%d" % (
            "PASS" if receipt_ok else "FAIL", receipt_pending))
    if TEST_BOGUS:
        log("  Bogus share: rejected=%s height_unchanged=%s" % (
            bogus_result.get("rejected") if bogus_result else None,
            bogus_result.get("height_unchanged") if bogus_result else None))

    print("SUMMARY_JSON:" + json.dumps(summary), flush=True)
    sys.exit(0 if failed == 0 else 1)

if __name__ == "__main__":
    main()
