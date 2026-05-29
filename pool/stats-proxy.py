#!/usr/bin/env python3
import json
import os
import threading
import time
import urllib.request
from collections import deque
from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler

ASIC_METRICS = "http://127.0.0.1:3334/metrics"
# CPU/GPU legacy metrics moved from 3336 to 3346 when port 3336 was
# reassigned to the public solo-pool listener (2026-05-29). Keep this
# string in sync with STRATUM_METRICS_BIND in stratum-legacy.env.
CPU_METRICS = "http://127.0.0.1:3346/metrics"
SOLO_METRICS = "http://127.0.0.1:3338/metrics"
PUBLIC_PORT = 3337

# Configured default share difficulties per profile (kept in sync with
# /etc/irium-pool/stratum*.env). Vardiff may drift up to MAX_DIFF per
# session; this constant is the conservative baseline used for estimation.
DEFAULTS = {
    "asic": {"diff": 2048, "metrics": ASIC_METRICS},
    "cpu_gpu": {"diff": 1024, "metrics": CPU_METRICS},
    "solo": {"diff": 10000, "metrics": SOLO_METRICS},
}

# Solo pool fee in basis points; mirrors IRIUM_STRATUM_SOLO_FEE_BPS in
# stratum-solo.env. Exposed in /solo-stats so the GUI can show the
# operator's headline "1% pool fee" copy without hardcoding it.
SOLO_FEE_BPS = 100
SOLO_PORT = 3336

# Rolling sample window: keep up to 15 minutes of (ts, accepted_shares)
# samples. We pick the oldest sample for delta computation, yielding a
# window-sized estimate that smooths out vardiff jitter.
WINDOW_SECS = 15 * 60
# MIN_SHARES lowered from 2 to 1 (was 2, originally 4). At 2, sporadic
# CPU miners producing ~1 share per 15-min window silently dropped to 0
# H/s even with a share just seconds old, because the deque's oldest
# accepted_count was N-1 and delta=1 fell below the threshold. 1 share
# is enough to surface a rough rate; delta_shares=0 still yields 0 via
# the formula (correctly indicates a truly idle worker).
MIN_SHARES = 1
MIN_SECONDS = 60

_lock = threading.Lock()
_samples = {name: deque() for name in DEFAULTS}

# Per-miner rolling samples for the /miners endpoint. Keyed by worker
# username, each value is a deque of (timestamp, accepted_count) tuples
# pruned to WINDOW_SECS. Same shape and semantics as _samples above but
# per-worker rather than per-profile. Populated by background_sampler
# from the "miners" subobject in stratum's /metrics JSON.
_miner_samples = {}

# blocks_found_today persistence: file-backed snapshot of the lifetime
# total_blocks_found counter as it stood at the start of the current UTC
# day. The /stats response surfaces today's diff so the Explorer UI can
# render a "found today" counter that resets at midnight UTC. The file
# survives proxy restarts so a mid-day systemctl restart does not lose
# the day's running count.
SNAPSHOT_FILE = "/opt/irium-pool/data/blocks_today_snapshot.json"
_today_lock = threading.Lock()
_today_snapshot = {"utc_date": None, "lifetime_at_snapshot": 0}


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


def _is_stale_session(m):
    """A miner row is 'stale' when its last share was over 10 minutes
    ago AND its rolling 15-min hashrate is zero or null. A
    last_share_ago_seconds value of None means the worker has never
    submitted a share - we treat that as 'just connected' rather than
    stale so a freshly-authorized session is not hidden."""
    ls = m.get("last_share_ago_seconds")
    hr = m.get("hashrate_15m")
    return (
        ls is not None
        and ls > 600
        and (hr is None or hr == 0)
    )


def _filter_stale_duplicates(miners):
    """Drop stale rows whose (base_address, profile) tuple has a peer
    with a more-recent last_share_ago_seconds. Grouping by the tuple
    rather than the base address alone means a wallet connected to BOTH
    the ASIC pool (port 3333) AND the legacy CPU/GPU pool (3335) under
    the same address keeps separate rows per profile - the ASIC session
    and the legacy session are genuinely different mining contexts and
    should both surface in the UI. Within a single profile, a stale row
    with a more-recent peer is still dropped (the original intent: kill
    yesterday's leftover session after the miner reconnected). Solo rows
    are kept even when stale so a wallet with no active session in
    either profile is still visible."""
    by_key = {}
    for m in miners:
        key = (m["worker"].split(".", 1)[0], m.get("profile"))
        by_key.setdefault(key, []).append(m)

    def recency(m):
        ls = m.get("last_share_ago_seconds")
        return ls if ls is not None else 10**9

    out = []
    for group in by_key.values():
        if len(group) <= 1:
            out.extend(group)
            continue
        most_recent_age = min(recency(m) for m in group)
        for m in group:
            if _is_stale_session(m) and recency(m) > most_recent_age:
                continue
            out.append(m)
    return out


def _load_today_snapshot():
    """Load the midnight snapshot from disk on startup. Silent on every
    error path - the snapshot is best-effort and a missing/corrupt file
    just means today's counter restarts from this point."""
    global _today_snapshot
    try:
        with open(SNAPSHOT_FILE) as f:
            data = json.load(f)
        if (
            isinstance(data, dict)
            and isinstance(data.get("utc_date"), str)
            and isinstance(data.get("lifetime_at_snapshot"), (int, float))
        ):
            with _today_lock:
                _today_snapshot = {
                    "utc_date": data["utc_date"],
                    "lifetime_at_snapshot": int(data["lifetime_at_snapshot"]),
                }
    except (FileNotFoundError, json.JSONDecodeError, ValueError, OSError):
        pass


def _save_today_snapshot():
    """Atomic write via tmp+rename so a crashed write never corrupts the
    snapshot file. Silent on errors - never crash the proxy for a
    failed counter save."""
    try:
        os.makedirs(os.path.dirname(SNAPSHOT_FILE), exist_ok=True)
        tmp = SNAPSHOT_FILE + ".tmp"
        with open(tmp, "w") as f:
            json.dump(_today_snapshot, f)
        os.replace(tmp, SNAPSHOT_FILE)
    except OSError:
        pass


def _blocks_found_today(current_total):
    """Return today's blocks-found count by diffing the current lifetime
    counter against the start-of-UTC-day snapshot. Rolls the snapshot
    forward when a new UTC day begins. On a cold start with no persisted
    snapshot the first call returns 0 - we don't know how many blocks
    were found before the proxy started observing today, so the honest
    answer is "none observed since we started watching". Subsequent
    calls within the same UTC day return the cumulative increase since
    that first call (or since the persisted snapshot if it carries
    today's date)."""
    today_utc = time.strftime("%Y-%m-%d", time.gmtime())
    with _today_lock:
        if _today_snapshot["utc_date"] != today_utc:
            _today_snapshot["utc_date"] = today_utc
            _today_snapshot["lifetime_at_snapshot"] = int(current_total)
            _save_today_snapshot()
            return 0
        diff = int(current_total) - int(_today_snapshot["lifetime_at_snapshot"])
        # Defensive: lifetime counter shouldn't go backwards but if the
        # upstream stratum lost state and the counter reset, re-baseline
        # so we never surface a negative number to the UI.
        if diff < 0:
            _today_snapshot["lifetime_at_snapshot"] = int(current_total)
            _save_today_snapshot()
            return 0
        return diff


class Handler(BaseHTTPRequestHandler):
    # Bound per-request socket I/O so a slow/silent client cannot hold a
    # worker thread indefinitely. With plain HTTPServer the accept loop
    # itself blocked on recv() from a stuck client (proven via
    # /proc/PID/stack: main thread wedged in tcp_recvmsg) while new SYNs
    # piled up in the backlog. ThreadingHTTPServer (below) gives each
    # request its own thread; timeout caps damage per thread.
    timeout = 10

    def do_GET(self):
        if self.path == "/miners":
            self._handle_miners()
            return
        if self.path == "/payouts":
            self._handle_payouts()
            return
        if self.path == "/solo-stats":
            self._handle_solo_stats()
            return
        if self.path == "/solo-miners":
            self._handle_solo_miners()
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
            "blocks_found_today": _blocks_found_today(
                asic.get("submit_accepted", 0) + cpu.get("submit_accepted", 0)
            ),
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
        # PPLNS standing — only the ASIC pool runs the payout subsystem
        # right now (legacy + 443 still on the pre-PPLNS binary). Failing
        # to fetch is non-fatal: the enrichment fields just go to None.
        payout_view = fetch("http://127.0.0.1:3334/miners_payout")
        payout_by_addr = (payout_view.get("by_address") or {}) if isinstance(payout_view, dict) else {}
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
                # are directly comparable. Key the sample lookup by
                # "{profile}:{worker}" so a wallet connected to BOTH
                # the ASIC pool (port 3333) and the legacy pool (3335)
                # does not contaminate a single shared deque with two
                # different `accepted` counters — that was the root
                # cause of hashrate_15m=0 on multi-pool miners.
                hashrate_15m, _hr_ws = estimate_miner_hashrate(f"{profile}:{worker}", accepted, diff)
                # PPLNS enrichment (ASIC profile only). Strip the worker
                # suffix to get the payout address — the payout subsystem
                # aggregates shares per-address since one wallet can
                # connect with multiple rig names.
                addr = worker.split(".", 1)[0]
                miner_payout = payout_by_addr.get(addr) if profile == "asic" else None
                pending_shares = miner_payout.get("pending_shares") if miner_payout else None
                estimated_payout_irm = miner_payout.get("estimated_payout_irm") if miner_payout else None
                miners_list.append({
                    "worker": worker,
                    "profile": profile,
                    "accepted": accepted,
                    "rejected": rejected,
                    "reject_rate_pct": reject_rate,
                    "reject_reasons": mstats.get("reject_reasons", {}) or {},
                    "hashrate_15m": hashrate_15m,
                    "last_share_ago_seconds": last_share_ago,
                    "pending_shares": pending_shares,
                    "estimated_payout_irm": estimated_payout_irm,
                })
        # Annotate session_status on every row so the UI can render an
        # "active" vs "stale" pill regardless of whether the row
        # survives the dedup filter below. Stale = no shares for over
        # 10 minutes AND zero/null rolling hashrate.
        for m in miners_list:
            m["session_status"] = "stale" if _is_stale_session(m) else "active"
        # Drop redundant stale entries: when the same base address has
        # both a stale row and a more-recent peer, the stale row is a
        # yesterday's-session leftover that just clutters the UI.
        miners_list = _filter_stale_duplicates(miners_list)
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

    def _handle_solo_stats(self):
        """GET /solo-stats: aggregate solo pool snapshot. Same shape as
        each profile inside /stats but scoped to the solo listener on
        port 3336. Surfaces fee_bps and solo_port up-front so the GUI
        can render the connection info card without a second fetch.
        """
        solo = fetch(SOLO_METRICS)
        solo_est = record_and_estimate("solo", solo)
        tcp = solo.get("active_tcp_sessions", 0)
        # Effective active miners: only count once at least one share has
        # been accepted, same convention the /stats endpoint uses for the
        # ASIC and CPU/GPU profiles. Suppresses noise from port scanners.
        accepted = solo.get("accepted_shares", 0)
        effective_miners = tcp if accepted > 0 else 0
        data = {
            "pool": "Irium Solo Pool",
            "url": "pool.iriumlabs.org",
            "solo_port": SOLO_PORT,
            "fee_bps": SOLO_FEE_BPS,
            "active_miners": effective_miners,
            "tcp_sessions": tcp,
            "accepted_shares": accepted,
            "rejected_shares": solo.get("rejected_shares", 0),
            "blocks_found": solo.get("submit_accepted", 0),
            "integrity": solo.get("pool_integrity", "unknown"),
            **solo_est,
        }
        body = json.dumps(data, indent=2).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _handle_solo_miners(self):
        """GET /solo-miners: per-worker accepted/rejected counts and the
        rolling 15-min hashrate for the solo listener (port 3336). Shape
        mirrors /miners so the GUI can render the same table component.
        """
        solo = fetch(SOLO_METRICS)
        now = time.time()
        miners_list = []
        miners_data = solo.get("miners", {}) or {}
        diff = DEFAULTS["solo"]["diff"]
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
            hashrate_15m, _hr_ws = estimate_miner_hashrate(f"solo:{worker}", accepted, diff)
            miners_list.append({
                "worker": worker,
                "profile": "solo",
                "accepted": accepted,
                "rejected": rejected,
                "reject_rate_pct": reject_rate,
                "reject_reasons": mstats.get("reject_reasons", {}) or {},
                "hashrate_15m": hashrate_15m,
                "last_share_ago_seconds": last_share_ago,
            })
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

    def _handle_payouts(self):
        """GET /payouts: proxy of stratum's PPLNS payout log. Returns the
        last 50 sent/failed payouts with block height, miner address,
        amount, share count, pct of window, and tx_id where available.

        Only the ASIC pool (port 3333) runs the PPLNS subsystem; legacy
        and 443 still operate on the pre-PPLNS binary. If the upstream
        endpoint is unreachable the proxy returns an empty list rather
        than 5xx — the consumer (Block Explorer pool page) renders this
        as "no payouts yet" which is correct during cold-start.
        """
        payouts = fetch("http://127.0.0.1:3334/payouts")
        if not isinstance(payouts, dict) or "payouts" not in payouts:
            payouts = {"count": 0, "payouts": []}
        body = json.dumps(payouts, indent=2).encode()
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
                        # Namespace by profile so a wallet connected to
                        # BOTH stratum services keeps separate deques per
                        # pool. See _handle_miners for the full rationale.
                        record_miner_sample(f"{name}:{worker}", accepted)
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
    _load_today_snapshot()
    threading.Thread(target=background_sampler, daemon=True).start()
    # Outer retry loop: if HTTPServer construction or serve_forever raises
    # (port temporarily unavailable, transient OS error, OOM recovery,
    # internal Python exception), sleep briefly and try again. systemd
    # will also restart the process if we exit, but the in-process retry
    # keeps the rolling sampling window populated across transient errors
    # instead of resetting it on every process restart.
    while True:
        try:
            # ThreadingHTTPServer: one OS thread per request. Required so
            # a stuck client socket cannot wedge the accept loop and
            # starve every other request. daemon_threads=True so worker
            # threads exit cleanly on systemd restart.
            server = ThreadingHTTPServer(("0.0.0.0", PUBLIC_PORT), Handler)
            server.daemon_threads = True
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
