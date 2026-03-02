import logging
import os
import subprocess
import time
from urllib.parse import urlparse, urlunparse

import httpx
from fastapi import FastAPI, Request

app = FastAPI()

logging.basicConfig(level=os.getenv("IRIUM_SHIM_LOG_LEVEL", "INFO"))
log = logging.getLogger("irium-pool-shim")

IRIUM_RPC_URL = os.getenv("IRIUM_RPC_URL", "http://127.0.0.1:38300")
IRIUM_RPC_TOKEN = os.getenv("IRIUM_RPC_TOKEN")
HEIGHT_DRIFT_MAX = int(os.getenv("IRIUM_TEMPLATE_HEIGHT_DRIFT_MAX", "2"))
STRATUM_HEALTH_URL = os.getenv("IRIUM_STRATUM_HEALTH_URL", "http://127.0.0.1:3334/health")
PUBLIC_API_BASES = [
    b.strip().rstrip("/")
    for b in os.getenv("IRIUM_PUBLIC_API_BASES", "https://api.iriumlabs.org/api").split(",")
    if b.strip()
]
POOL_PORT = int(os.getenv("IRIUM_POOL_PORT", "3333"))

log.info("upstream_rpc_url=%s", IRIUM_RPC_URL)


def _auth_headers():
    return {"Authorization": f"Bearer {IRIUM_RPC_TOKEN}"} if IRIUM_RPC_TOKEN else {}


def _http_variant(url: str) -> str:
    p = urlparse(url)
    if p.scheme == "https":
        return urlunparse(p._replace(scheme="http"))
    if p.scheme == "http":
        return urlunparse(p._replace(scheme="https"))
    return url


async def _request_json(method: str, path: str, *, params=None, json_body=None, timeout=20):
    base = IRIUM_RPC_URL.rstrip("/")
    urls = [base]
    alt = _http_variant(base)
    if alt != base:
        urls.append(alt)

    last_err = None
    for i, u in enumerate(urls):
        verify = False if u.startswith("https://") else True
        try:
            async with httpx.AsyncClient(verify=verify, timeout=timeout) as client:
                resp = await client.request(
                    method,
                    f"{u}{path}",
                    headers=_auth_headers(),
                    params=params,
                    json=json_body,
                )
                resp.raise_for_status()
                return resp.json(), u
        except Exception as e:
            last_err = e
            msg = str(e)
            if i == 0 and base.startswith("https://") and "WRONG_VERSION_NUMBER" in msg.upper():
                log.warning("upstream https wrong version; retrying with http url=%s", _http_variant(base))
                continue
            if i == 0 and base.startswith("http://") and "SSL" in msg.upper():
                log.warning("upstream http tls mismatch; retrying with https url=%s", _http_variant(base))
                continue
            break

    raise RuntimeError(f"upstream_connect_failed path={path} base={base}: {last_err}")


async def _fetch_public_json(path: str, timeout=12):
    last_err = None
    for base in PUBLIC_API_BASES:
        url = f"{base}{path}"
        try:
            async with httpx.AsyncClient(timeout=timeout, verify=False) as client:
                r = await client.get(url)
                r.raise_for_status()
                return r.json()
        except Exception as e:
            last_err = e
            continue
    if last_err:
        raise RuntimeError(f"public_api_fetch_failed path={path}: {last_err}")
    raise RuntimeError("public_api_fetch_failed: no bases configured")


def _count_tcp_sessions(port: int) -> int:
    try:
        out = subprocess.check_output(["ss", "-tn", "state", "established"], text=True, timeout=3)
        return sum(1 for line in out.splitlines() if f":{port}" in line)
    except Exception:
        return 0


async def _stratum_health():
    try:
        async with httpx.AsyncClient(timeout=8, verify=False) as client:
            r = await client.get(STRATUM_HEALTH_URL)
            r.raise_for_status()
            return r.json()
    except Exception:
        return None


async def fetch_status():
    body, _ = await _request_json("GET", "/status", timeout=20)
    return body


async def fetch_block_by_height(height: int):
    body, _ = await _request_json("GET", "/rpc/block", params={"height": int(height)}, timeout=20)
    return body


async def fetch_template():
    body, _ = await _request_json("GET", "/rpc/getblocktemplate", timeout=60)
    return body.get("result", body)


async def call_irium(method, payload=None):
    if method == "getblocktemplate":
        tpl = await fetch_template()
        s = await fetch_status()

        node_height = int(s.get("height", 0))
        tpl_height = int(tpl.get("height", 0))
        prev = tpl.get("previousblockhash") or tpl.get("prev_hash") or ""

        if "previousblockhash" not in tpl and "prev_hash" in tpl:
            tpl["previousblockhash"] = tpl["prev_hash"]

        drift = abs(node_height - tpl_height)
        log.info(
            "template_check rpc_height=%s template_height=%s prevhash=%s drift=%s",
            node_height,
            tpl_height,
            prev,
            drift,
        )
        if drift > HEIGHT_DRIFT_MAX:
            msg = (
                f"template height drift too high: rpc_height={node_height} "
                f"template_height={tpl_height} prevhash={prev} drift={drift}"
            )
            log.error(msg)
            raise RuntimeError(msg)

        return tpl

    if method == "submitblock":
        body, _ = await _request_json("POST", "/rpc/submit_block", json_body=payload, timeout=60)
        return body.get("result", body)

    if method == "getblockcount":
        s = await fetch_status()
        return int(s.get("height", 0))

    if method == "getbestblockhash":
        s = await fetch_status()
        best = s.get("best_header_tip") or {}
        return best.get("hash") or s.get("tip_hash") or ""

    if method == "getblockhash":
        if payload is None:
            raise ValueError("getblockhash requires [height]")
        if isinstance(payload, list):
            if not payload:
                raise ValueError("getblockhash requires [height]")
            h = int(payload[0])
        else:
            h = int(payload)
        b = await fetch_block_by_height(h)
        hdr = b.get("header") or {}
        hsh = hdr.get("hash") or ""
        if not hsh:
            raise RuntimeError(f"no block hash found for height={h}")
        return hsh

    raise ValueError("unsupported method")


@app.get("/health")
async def health():
    s = await fetch_status()
    return {
        "ok": True,
        "height": s.get("height", 0),
        "best_header_tip": s.get("best_header_tip", {}),
        "peer_count": s.get("peer_count", 0),
        "source": IRIUM_RPC_URL,
    }


@app.get("/status")
async def status_alias():
    return await health()


@app.get("/api/pool/health")
async def pool_health():
    out = {
        "healthy": False,
        "backend_connected": False,
        "updated_at": int(time.time()),
        "issues": [],
    }
    try:
        s = await fetch_status()
        out["height"] = int(s.get("height", 0))
        out["peer_count"] = int(s.get("peer_count", 0))
        out["backend_connected"] = True
    except Exception as e:
        out["issues"].append(f"status fetch failed: {e}")

    sh = await _stratum_health()
    if sh:
        now = int(time.time())
        age = int(sh.get("age_seconds", 0) or 0)
        out["stratum_status"] = sh.get("status", "unknown")
        out["stratum_age_seconds"] = age
        out["stratum_height"] = int(sh.get("height", 0) or 0)
        out["stratum_prevhash"] = sh.get("prevhash", "")
        out["last_template_update_ts"] = max(0, now - age)
    else:
        out["issues"].append("stratum health unavailable")

    out["healthy"] = out["backend_connected"] and (out.get("stratum_status") in ("ok", "fresh", None))
    return out


@app.get("/api/pool/status")
async def pool_status():
    return await pool_health()


@app.get("/api/pool/stats")
async def pool_stats():
    now = int(time.time())
    s = await fetch_status()
    h = await pool_health()

    active_miners = 0
    active_window = 144
    try:
        st = await _fetch_public_json("/stats")
        active_miners = int(st.get("active_miners", 0) or 0)
        active_window = int(st.get("active_miners_window_blocks", 144) or 144)
    except Exception:
        pass

    tcp = _count_tcp_sessions(POOL_PORT)

    return {
        "backend_connected": bool(h.get("backend_connected", False)),
        "network_height": int(s.get("height", 0) or 0),
        "active_tcp_sessions": int(tcp),
        "workers_online": int(tcp),
        "active_miners_as_of_height": int(active_miners),
        "active_miners_window_blocks": int(active_window),
        "accepted_shares": 0,
        "rejected_shares": 0,
        "stale_shares": 0,
        "pending_count": 0,
        "last_share_accept_ts": 0,
        "last_share_reject_ts": 0,
        "last_template_update_ts": int(h.get("last_template_update_ts", now)),
        "template_updated_at": int(h.get("last_template_update_ts", now)),
        "stratum_status": h.get("stratum_status", "unknown"),
        "stratum_age_seconds": int(h.get("stratum_age_seconds", 0) or 0),
        "updated_at": now,
    }


@app.get("/api/pool/payouts")
async def pool_payouts(limit: int = 20):
    rows = []
    try:
        blocks = await _fetch_public_json(f"/blocks?limit={max(1, min(limit, 100))}")
        bl = blocks.get("blocks") if isinstance(blocks, dict) else None
        if isinstance(bl, list):
            for b in bl:
                reward = b.get("reward")
                if reward is None:
                    reward = b.get("block_reward")
                if reward is None:
                    reward = 50
                rows.append({
                    "height": int(b.get("height", 0) or 0),
                    "time": int((b.get("header") or {}).get("time", 0) or 0),
                    "address": b.get("miner_address") or b.get("miner") or "",
                    "reward_irm": float(reward),
                    "status": "on_chain",
                    "hash": (b.get("header") or {}).get("hash", ""),
                    "maturity_remaining": 0,
                })
    except Exception:
        rows = []

    return {"pending_count": 0, "payouts": rows[: max(1, min(limit, 100))]}


@app.get("/api/pool/account/{address}")
async def pool_account(address: str, window: int = 5000, limit: int = 12):
    payouts = await pool_payouts(limit=max(limit, 50))
    rows = [r for r in payouts.get("payouts", []) if r.get("address") == address]
    rows = rows[: max(1, min(limit, 100))]
    total = sum(float(r.get("reward_irm", 0) or 0) for r in rows)
    last = rows[0] if rows else None
    return {
        "address": address,
        "window": int(window),
        "blocks_found": len(rows),
        "total_rewards_irm": total,
        "paid_total_irm": total,
        "pending_balance_irm": 0,
        "last_found": last,
        "records": rows,
    }


@app.get("/api/pool/workers")
async def pool_workers(window: int = 144, limit: int = 20):
    st = await _fetch_public_json("/stats")
    count = int(st.get("active_miners", 0) or 0)
    return {
        "window": int(window),
        "limit": int(limit),
        "worker_count": count,
        "workers": [],
    }


@app.post("/")
async def rpc_proxy(req: Request):
    data = await req.json()
    method = data.get("method")
    params = data.get("params", [])

    try:
        payload = params[0] if params else None
        result = await call_irium(method, payload)
        return {"jsonrpc": "2.0", "id": data.get("id"), "result": result, "error": None}
    except Exception as e:
        return {
            "jsonrpc": "2.0",
            "id": data.get("id"),
            "result": None,
            "error": {"code": -32603, "message": str(e)},
        }
