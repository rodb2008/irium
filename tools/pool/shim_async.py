import logging
import os
from urllib.parse import urlparse, urlunparse

import httpx
from fastapi import FastAPI, Request

app = FastAPI()

logging.basicConfig(level=os.getenv("IRIUM_SHIM_LOG_LEVEL", "INFO"))
log = logging.getLogger("irium-pool-shim")

IRIUM_RPC_URL = os.getenv("IRIUM_RPC_URL", "http://127.0.0.1:38300")
IRIUM_RPC_TOKEN = os.getenv("IRIUM_RPC_TOKEN")
HEIGHT_DRIFT_MAX = int(os.getenv("IRIUM_TEMPLATE_HEIGHT_DRIFT_MAX", "2"))

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
        verify = u.startswith("https://")
        if verify:
            verify = False
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
    return await health()


@app.get("/api/pool/status")
async def pool_status():
    return await health()


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
