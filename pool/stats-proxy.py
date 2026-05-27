#!/usr/bin/env python3
import json
import threading
import time
import urllib.request
from collections import deque
from http.server import HTTPServer, BaseHTTPRequestHandler

ASIC_METRICS = "http://127.0.0.1:3334/metrics"
CPU_METRICS = "http://127.0.0.1:3336/metrics"
PUBLIC_PORT = 3337

# Configured default share difficulties per profile (kept in sync with
# /etc/irium-pool/stratum*.env). Vardiff may drift up to MAX_DIFF per
# session; this constant is the conservative baseline used for estimation.
DEFAULTS = {
    "asic": {"diff": 2048, "metrics": ASIC_METRICS},
    "cpu_gpu": {"diff": 1024, "metrics": CPU_METRICS},
}

# Rolling sample window: keep up to 15 minutes of (ts, accepted_shares)
# samples. We pick the oldest sample for delta computation, yielding a
# window-sized estimate that smooths out vardiff jitter.
WINDOW_SECS = 15 * 60
MIN_SHARES = 4
MIN_SECONDS = 120

_lock = threading.Lock()
_samples = {name: deque() for name in DEFAULTS}

# Per-miner rolling samples for the /miners endpoint. Keyed by worker
# username, each value is a deque of (timestamp, accepted_count) tuples
# pruned to WINDOW_SECS. Same shape and semantics as _samples above but
# per-worker rather than per-profile. Populated by background_sampler
# from the "miners" subobject in stratum's /metrics JSON.
_miner_samples = {}


def fetch(url):
    try:
        with urllib.request.urlopen(url, timeout=3) as r:
            return json.loads(r.read())
    except Exception:
        return {}


def confidence(delta_shares):
    if delta_shares < 30:
        return "low"
    if delta_shares < 200:
        return "medium"
    return "high"


def record_and_estimate(profile, metrics):
    accepted = int(metrics.get("accepted_shares", 0) or 0)
    rejected = int(metrics.get("rejected_shares", 0) or 0)
    now = time.time()
    with _lock:
        buf = _samples[profile]
        # 3-tuple: (ts, accepted, rejected). Track rejected alongside
        # accepted so we can derive a rolling reject rate from the same
        # 15-min window the hashrate estimate already uses. Old entries
        # from a pre-upgrade running process are flushed when this file
        # is reloaded (systemctl restart of the proxy), so no migration.
        buf.append((now, accepted, rejected))
        # Prune anything older than WINDOW_SECS.
        cutoff = now - WINDOW_SECS
        while buf and buf[0][0] < cutoff:
            buf.popleft()
        oldest_ts, oldest_accepted, oldest_rejected = buf[0]
    delta_shares = accepted - oldest_accepted
    delta_rejected = rejected - oldest_rejected
    delta_seconds = now - oldest_ts
    # Rolling reject rate over the same 15-min window as the hashrate
    # estimate. Guarded by MIN_SHARES so a fresh process with only a few
    # observations doesn't render a noisy 0% / 100% percentage. None
    # surfaces as JSON null — the GUI should render "Collecting…".
    delta_total = delta_shares + delta_rejected
    recent_reject_rate_pct = (
        round((delta_rejected / delta_total) * 100, 1)
        if delta_total >= MIN_SHARES else None
    )
    # Per-profile baseline share difficulty. Exposed unconditionally so
    # the GUI has a value even before the rolling window matures. When
    # stratum's /metrics later exposes vardiff-current we can scrape it
    # here and override this baseline. The field name is `current_diff`
    # (not `diff`) so the consumer can tell baseline-vs-vardiff apart
    # in a future revision without renaming.
    current_diff = DEFAULTS[profile]["diff"]
    # Still warming up — too little time to compute anything meaningful.
    # The GUI shows "Collecting…" until the sampling window is at least
    # MIN_SECONDS long.
    if delta_seconds < MIN_SECONDS:
        return {
            "hashrate_estimate_hps": None,
            "hashrate_window_seconds": int(delta_seconds),
            "hashrate_confidence": "low",
            "current_diff": current_diff,
            "recent_reject_rate_pct": recent_reject_rate_pct,
        }
    # Window is mature but we haven't seen MIN_SHARES accepted shares yet.
    # The honest answer is 0 H/s — there is no hashrate being produced
    # against valid work — so we return 0 instead of None and let the GUI
    # render "0 H/s" rather than a perpetually-"collecting" placeholder.
    if delta_shares < MIN_SHARES:
        return {
            "hashrate_estimate_hps": 0,
            "hashrate_window_seconds": int(delta_seconds),
            "hashrate_confidence": "low",
            "current_diff": current_diff,
            "recent_reject_rate_pct": recent_reject_rate_pct,
        }
    diff = current_diff
    # Stratum convention: 1 share at difficulty 1 = 2^32 hashes on average.
    hashrate_hps = (delta_shares * diff * (1 << 32)) / delta_seconds
    return {
        "hashrate_estimate_hps": hashrate_hps,
        "hashrate_window_seconds": int(delta_seconds),
        "hashrate_confidence": confidence(delta_shares),
        "current_diff": current_diff,
        "recent_reject_rate_pct": recent_reject_rate_pct,
    }


def record_miner_sample(worker, accepted):
    """Append (now, accepted) to the per-worker deque and prune old entries.

    Same rolling-window semantics as record_and_estimate but keyed by
    worker. Caller holds no lock - this acquires _lock briefly. Safe to
    call concurrently from background_sampler and the /miners request
    handler.
    """
    now = time.time()
    with _lock:
        buf = _miner_samples.setdefault(worker, deque())
        buf.append((now, accepted))
        cutoff = now - WINDOW_SECS
        while buf and buf[0][0] < cutoff:
            buf.popleft()


def estimate_miner_hashrate(worker, accepted_now, diff):
    """Compute the per-worker rolling hashrate over the last WINDOW_SECS.

    Returns a tuple (hashrate_hps, window_seconds). Returns (None, 0) if
    no samples are available, (None, ws) during warmup (window too short),
    and (0, ws) if the window is mature but no new shares arrived.

    Uses the same formula as record_and_estimate (delta * diff * 2^32 /
    seconds) so the per-miner number is directly comparable to the
    aggregate hashrate_estimate_hps in /stats.
    """
    with _lock:
        buf = _miner_samples.get(worker)
        if not buf:
            return (None, 0)
        oldest_ts, oldest_accepted = buf[0]
    now = time.time()
    delta_shares = accepted_now - oldest_accepted
    delta_seconds = now - oldest_ts
    if delta_seconds < MIN_SECONDS:
        return (None, int(delta_seconds))
    if delta_shares < MIN_SHARES:
        return (0, int(delta_seconds))
    hashrate_hps = (delta_shares * diff * (1 << 32)) / delta_seconds
    return (hashrate_hps, int(delta_seconds))


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/miners":
            self._handle_miners()
            return
        if self.path not in ("/", "/stats", "/api/stats"):
            self.send_response(404)
            self.end_headers()
            return
        asic = fetch(ASIC_METRICS)
        cpu = fetch(CPU_METRICS)
        asic_est = record_and_estimate("asic", asic)
        cpu_est = record_and_estimate("cpu_gpu", cpu)
        asic_tcp = asic.get("active_tcp_sessions", 0)
        cpu_tcp = cpu.get("active_tcp_sessions", 0)
        data = {
            "pool": "Irium Official Pool",
            "url": "pool.iriumlabs.org",
            "asic_port": 3333,
            "cpu_gpu_port": 3335,
            "asic": {
                "active_miners": asic_tcp,
                "tcp_sessions": asic_tcp,
                "accepted_shares": asic.get("accepted_shares", 0),
                "rejected_shares": asic.get("rejected_shares", 0),
                "blocks_found": asic.get("submit_accepted", 0),
                "integrity": asic.get("pool_integrity", "unknown"),
                **asic_est,
            },
            "cpu_gpu": {
                "active_miners": cpu_tcp,
                "tcp_sessions": cpu_tcp,
                "accepted_shares": cpu.get("accepted_shares", 0),
                "rejected_shares": cpu.get("rejected_shares", 0),
                "blocks_found": cpu.get("submit_accepted", 0),
                "integrity": cpu.get("pool_integrity", "unknown"),
                **cpu_est,
            },
            "total_miners": asic_tcp + cpu_tcp,
            "total_blocks_found": asic.get("submit_accepted", 0) + cpu.get("submit_accepted", 0),
        }
        body = json.dumps(data, indent=2).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _handle_miners(self):
        """GET /miners: per-worker accepted/rejected counts, reject reasons,
        rolling 15-min hashrate, and seconds since the worker's last share.

        Reads the "miners" subobject from stratum's /metrics JSON (no new
        IPC) and merges per-worker rolling samples maintained by
        background_sampler. Workers appearing on both ASIC and CPU
        endpoints (rare in practice - the same worker name would have to
        be authorized on both ports) are emitted twice in the output,
        once per profile, with the profile baseline diff used for
        hashrate. This is the cleanest mapping that mirrors how the
        aggregate /stats endpoint already segregates asic vs cpu_gpu.
        """
        asic = fetch(ASIC_METRICS)
        cpu = fetch(CPU_METRICS)
        now = time.time()
        miners_list = []
        for profile, source_metrics in (("asic", asic), ("cpu_gpu", cpu)):
            miners_data = source_metrics.get("miners", {}) or {}
            diff = DEFAULTS[profile]["diff"]
            for worker, mstats in miners_data.items():
                accepted = int(mstats.get("accepted", 0) or 0)
                rejected = int(mstats.get("rejected", 0) or 0)
                total = accepted + rejected
                reject_rate = (
                    round((rejected / total) * 100, 1)
                    if total >= MIN_SHARES else None
                )
                last_share_at = int(mstats.get("last_share_at", 0) or 0)
                last_share_ago = (
                    int(now - last_share_at) if last_share_at > 0 else None
                )
                # Per-miner hashrate over the rolling 15-min window. Uses
                # the same formula as the aggregate hashrate so numbers
                # are directly comparable.
                hashrate_15m, _hr_ws = estimate_miner_hashrate(worker, accepted, diff)
                miners_list.append({
                    "worker": worker,
                    "profile": profile,
                    "accepted": accepted,
                    "rejected": rejected,
                    "reject_rate_pct": reject_rate,
                    "reject_reasons": mstats.get("reject_reasons", {}) or {},
                    "hashrate_15m": hashrate_15m,
                    "last_share_ago_seconds": last_share_ago,
                })
        # Sort newest-active first so the most informative rows appear at
        # the top of any consumer that doesn't sort itself.
        miners_list.sort(
            key=lambda m: (m["last_share_ago_seconds"] if m["last_share_ago_seconds"] is not None else 10**9)
        )
        response = {
            "total_miners": len(miners_list),
            "miners": miners_list,
        }
        body = json.dumps(response, indent=2).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def background_sampler():
    # Periodically self-scrape so the rolling window stays populated even
    # when no client is requesting /stats. Without this, the first GUI
    # fetch always returns null until two requests are far enough apart.
    #
    # Wrapped in try/except so a transient exception in record_and_estimate
    # (e.g., malformed upstream metrics, deque race) does not silently kill
    # this daemon thread. Without the wrapper, a single bad iteration
    # leaves the process alive but sampling frozen — hashrate estimates
    # would never advance again until process restart.
    while True:
        try:
            for name, cfg in DEFAULTS.items():
                metrics = fetch(cfg["metrics"])
                record_and_estimate(name, metrics)
                # Also feed per-miner samples so the /miners endpoint has
                # rolling hashrate data even when no client has hit it
                # yet. Inner try/except so one malformed miner entry
                # doesn't poison the whole sampling tick.
                miners_data = metrics.get("miners", {}) or {}
                for worker, mstats in miners_data.items():
                    try:
                        accepted = int(mstats.get("accepted", 0) or 0)
                        record_miner_sample(worker, accepted)
                    except Exception as inner_e:
                        print(
                            f"[stats-proxy] miner-sample error for "
                            f"{worker}: {inner_e}",
                            flush=True,
                        )
        except Exception as e:
            print(f"[stats-proxy] background sampler error: {e}", flush=True)
        time.sleep(30)


if __name__ == "__main__":
    threading.Thread(target=background_sampler, daemon=True).start()
    # Outer retry loop: if HTTPServer construction or serve_forever raises
    # (port temporarily unavailable, transient OS error, OOM recovery,
    # internal Python exception), sleep briefly and try again. systemd
    # will also restart the process if we exit, but the in-process retry
    # keeps the rolling sampling window populated across transient errors
    # instead of resetting it on every process restart.
    while True:
        try:
            server = HTTPServer(("0.0.0.0", PUBLIC_PORT), Handler)
            print(f"Pool stats proxy running on :{PUBLIC_PORT}", flush=True)
            server.serve_forever()
        except OSError as e:
            # Most likely: port already in use (zombie process from a
            # previous run, or unit-file collision). Sleep longer for
            # bind contention so the holder has time to exit.
            print(f"[stats-proxy] HTTPServer bind/serve OSError: {e}", flush=True)
            time.sleep(5)
        except Exception as e:
            print(f"[stats-proxy] unexpected error in main loop: {e}", flush=True)
            time.sleep(3)
