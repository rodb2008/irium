//! v1.9.66 — Native DOGE P2P SPV header client.
//!
//! Speaks the Dogecoin P2P protocol (Bitcoin-compatible wire format
//! with `magic = 0xC0C0C0C0`, port 22556) directly. Replaces the
//! external HTTP block-explorer APIs (`dogecoinspace.org`,
//! `blockcypher.com`, etc.) that v1.9.65 was juggling, eliminating
//! the last third-party HTTP dependency on the DOGE relay path.
//!
//! Issue #60 phase 1. BTC and LTC keep their HTTP paths until DOGE
//! has demonstrated 48hr stability on mainnet.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::{lookup_host, TcpStream};
use tokio::time::timeout;

use crate::p2p_wire::{
    build_getdata_payload, build_getheaders_payload, build_version_payload,
    parse_block_prefix_for_auxpow, parse_headers_payload, read_message,
    read_varint_slice, sha256d, write_message, MSG_BLOCK,
};

use std::collections::HashMap;

/// DOGE mainnet network magic. Wire bytes are `c0 c0 c0 c0`, which
/// `u32.to_le_bytes()` reproduces from this host integer.
const DOGE_MAGIC: u32 = 0xC0C0C0C0;

/// DOGE mainnet default P2P port.
const DOGE_PORT: u16 = 22556;

/// DOGE DNS seeds. Each resolves to a rotating list of node IPs run by
/// Dogecoin Core developers + community. We iterate all seeds, collect
/// unique candidates, then try up to MAX_PEER_ATTEMPTS of them. DOGE
/// has a smaller alive-node pool than BTC, so the attempt cap is
/// intentionally generous (R6 from the Issue #60 plan).
const DOGE_SEEDS: &[&str] = &[
    "seed.dogecoin.com",
    "seed.multidoge.org",
    "seed.dogechain.info",
];

/// Per-peer TCP connect timeout. 10s is well above typical RTT and
/// below the ~30s most network stacks default to.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Max distinct peers we will try per fetch_headers call before giving
/// up. The cycle re-runs every 600s so silent failure of one cycle is
/// recoverable; this cap prevents a single cycle from spending its
/// whole budget on dead peers.
const MAX_PEER_ATTEMPTS: usize = 20;

/// Max messages we will read while waiting for a specific reply (e.g.
/// VERACK during handshake, HEADERS after getheaders). Modern Core
/// sends ~5-10 feature-negotiation messages around verack; 30 is well
/// above that and bounds the loop so a chatty peer cannot stall us.
const MAX_HANDSHAKE_MESSAGES: usize = 30;
const MAX_POST_GETHEADERS_MESSAGES: usize = 30;

/// PR-6 of issue #68: fetch DOGE headers AND, for any header with the
/// AuxPoW version bit set, the iriumd-format AuxPoW bytes for that
/// header. Returns each header alongside `Option<Vec<u8>>` where Some
/// is iriumd-format AuxPoW (ready for `auxpow::deserialize`) and None
/// means the header has no AuxPoW (pre-371,337 or no-bit pool blocks).
///
/// Wire protocol: getheaders → headers, then a getdata batch with
/// `MSG_BLOCK` invs for the AuxPoW headers, drained one block message
/// at a time. AuxPoW data is extracted by
/// `p2p_wire::parse_block_prefix_for_auxpow`, which also converts from
/// DOGE on-wire format (CTransaction coinbase, no length prefix) to
/// iriumd internal format (varint-prefixed coinbase).
pub async fn fetch_doge_headers_with_auxpow(
    relay_tip_hash: [u8; 32],
) -> Result<Vec<([u8; 80], Option<Vec<u8>>)>, String> {
    let peers = discover_peers().await?;
    let mut last_err = String::new();
    let mut attempts = 0usize;
    for peer in peers {
        if attempts >= MAX_PEER_ATTEMPTS {
            break;
        }
        attempts += 1;
        match try_peer_with_auxpow(peer, relay_tip_hash).await {
            Ok(items) => return Ok(items),
            Err(e) => {
                last_err = format!("{peer}: {e}");
                continue;
            }
        }
    }
    Err(format!(
        "all {attempts} DOGE peers failed; last error: {last_err}"
    ))
}

/// PR-6 deprecated thin wrapper for any leftover callers. Drops AuxPoW
/// data — callers needing AuxPoW validation must migrate to
/// `fetch_doge_headers_with_auxpow`.
#[deprecated(note = "use fetch_doge_headers_with_auxpow; this drops AuxPoW data")]
#[allow(dead_code)]
pub async fn fetch_headers(relay_tip_hash: [u8; 32]) -> Result<Vec<[u8; 80]>, String> {
    let items = fetch_doge_headers_with_auxpow(relay_tip_hash).await?;
    Ok(items.into_iter().map(|(h, _)| h).collect())
}

/// Resolve every DOGE DNS seed, dedupe IPs (so we do not waste
/// MAX_PEER_ATTEMPTS on the same node from two seeds), shuffle by
/// nondeterministic insertion order. DNS resolution failure on
/// individual seeds is silent — if all three fail we error out.
async fn discover_peers() -> Result<Vec<SocketAddr>, String> {
    use std::collections::HashSet;
    let mut seen: HashSet<SocketAddr> = HashSet::new();
    let mut out: Vec<SocketAddr> = Vec::new();
    for seed in DOGE_SEEDS {
        let host = format!("{seed}:{DOGE_PORT}");
        match timeout(CONNECT_TIMEOUT, lookup_host(host.as_str())).await {
            Ok(Ok(iter)) => {
                for sa in iter {
                    if seen.insert(sa) {
                        out.push(sa);
                    }
                }
            }
            Ok(Err(_)) | Err(_) => continue,
        }
        if out.len() >= MAX_PEER_ATTEMPTS * 2 {
            break;
        }
    }
    if out.is_empty() {
        return Err("no DOGE peers resolved from DNS seeds".to_string());
    }
    Ok(out)
}

/// v1.9.81: temporarily reduced to 1 for isolation diagnostics —
/// if a single-inv getdata still times out, the issue is not batch
/// size. Will be restored to a higher value (10–50) once the failure
/// mode is identified. PR-6 of issue #68.
const INV_BATCH: usize = 1;

/// PR-6 of issue #68: max messages drained while waiting for block
/// responses to a single getdata batch. Covers the requested blocks
/// plus typical chatter (ping/inv/addr) interleaved by the peer.
const MAX_BLOCK_RESPONSE_MESSAGES: usize = 200;

/// PR-6 of issue #68: full session against one peer. Same handshake
/// + getheaders flow as v1.9.66's `try_peer`, then issues `getdata`
/// for headers with the AuxPoW bit, parses block prefixes, returns
/// the header-with-optional-auxpow items in original order.
async fn try_peer_with_auxpow(
    peer: SocketAddr,
    tip: [u8; 32],
) -> Result<Vec<([u8; 80], Option<Vec<u8>>)>, String> {
    let mut stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(peer))
        .await
        .map_err(|_| "connect timeout".to_string())?
        .map_err(|e| format!("connect error: {e}"))?;
    // v1.9.82 of issue #68: disable Nagle so the 37-byte `getdata`
    // payload ships immediately. With Nagle on, a small write sits in
    // the kernel send buffer waiting for more data or the next ACK
    // (up to 200ms delayed-ACK on Linux). DOGE peers were silently
    // timing us out before our getdata even left the socket. Bitcoin
    // Core sets TCP_NODELAY on all P2P sockets; we match that.
    stream
        .set_nodelay(true)
        .map_err(|e| format!("set_nodelay: {e}"))?;

    // Handshake — send our VERSION first.
    let vp = build_version_payload(0, DOGE_PORT);
    write_message(&mut stream, DOGE_MAGIC, "version", &vp)
        .await
        .map_err(|e| format!("send version: {e}"))?;

    // Now consume messages until we have BOTH received their version
    // AND received their verack. Modern Bitcoin Core sends pre-verack
    // feature messages (SENDADDRV2, WTXIDRELAY); silently drain them
    // so they do not knock us off the conversation.
    let mut got_their_version = false;
    let mut got_their_verack = false;
    for _ in 0..MAX_HANDSHAKE_MESSAGES {
        if got_their_version && got_their_verack {
            break;
        }
        let msg = read_message(&mut stream, DOGE_MAGIC)
            .await
            .map_err(|e| format!("handshake read: {e}"))?;
        match msg.command.as_str() {
            "version" => {
                got_their_version = true;
                // v1.9.81 diagnostic: log peer's services + user_agent.
                if msg.payload.len() >= 12 {
                    let services = u64::from_le_bytes(
                        msg.payload[4..12].try_into().unwrap(),
                    );
                    // version body: i32 ver(4) + u64 services(8) + i64 ts(8) +
                    // recv_addr(26) + from_addr(26) + nonce(8) = offset 80
                    let mut ua = String::new();
                    if msg.payload.len() > 80 {
                        if let Ok((ualen, used)) =
                            read_varint_slice(&msg.payload[80..])
                        {
                            let start = 80 + used;
                            let end = start + (ualen as usize);
                            if end <= msg.payload.len() {
                                ua = String::from_utf8_lossy(
                                    &msg.payload[start..end],
                                )
                                .into_owned();
                            }
                        }
                    }
                    eprintln!(
                        "[doge-p2p] peer {peer} version services=0x{services:016x} user_agent={ua:?}"
                    );
                }
                // Echo verack back.
                write_message(&mut stream, DOGE_MAGIC, "verack", &[])
                    .await
                    .map_err(|e| format!("send verack: {e}"))?;
            }
            "verack" => got_their_verack = true,
            "ping" => {
                // RFC: pong echoes the ping nonce payload back verbatim.
                write_message(&mut stream, DOGE_MAGIC, "pong", &msg.payload)
                    .await
                    .map_err(|e| format!("send pong during handshake: {e}"))?;
            }
            _ => {
                // SENDADDRV2 / WTXIDRELAY / SENDCMPCT / FEEFILTER / ...
                // We do not act on these — just drain.
            }
        }
    }
    if !(got_their_version && got_their_verack) {
        return Err(format!(
            "incomplete handshake (version={got_their_version}, verack={got_their_verack})"
        ));
    }

    // Send getheaders with a single-hash locator. Peer walks our
    // locator and starts streaming headers from the FIRST entry it
    // also knows. If `tip` is on the peer's chain, it sends the next
    // up-to-2000 headers; if not, it sends nothing (an empty headers
    // message). The next cycle will retry with whatever chain.doge_tip
    // is after the block apply.
    let _ = sha256d; // silences unused-import on minimal builds
    let payload = build_getheaders_payload(&[tip], [0u8; 32]);
    write_message(&mut stream, DOGE_MAGIC, "getheaders", &payload)
        .await
        .map_err(|e| format!("send getheaders: {e}"))?;

    // Drain until we see a HEADERS reply (or hit the message cap).
    // Continue answering PING during this window so the peer does not
    // disconnect us mid-conversation.
    let mut headers: Option<Vec<[u8; 80]>> = None;
    for _ in 0..MAX_POST_GETHEADERS_MESSAGES {
        let msg = read_message(&mut stream, DOGE_MAGIC)
            .await
            .map_err(|e| format!("post-getheaders read: {e}"))?;
        match msg.command.as_str() {
            "headers" => {
                headers = Some(parse_headers_payload(&msg.payload)?);
                break;
            }
            "ping" => {
                write_message(&mut stream, DOGE_MAGIC, "pong", &msg.payload)
                    .await
                    .map_err(|e| format!("send pong post-getheaders: {e}"))?;
            }
            _ => {
                // INV / ADDR / GETHEADERS-from-peer / etc — ignore.
            }
        }
    }
    let headers = headers
        .ok_or_else(|| "no headers received after MAX_POST_GETHEADERS_MESSAGES".to_string())?;

    // v1.9.81 diagnostic: log how many headers and how many need AuxPoW.
    let auxpow_bit_count = headers.iter().filter(|h| {
        let v = i32::from_le_bytes(h[..4].try_into().unwrap());
        (v as u32) & 0x100 != 0
    }).count();
    eprintln!(
        "[doge-p2p] peer {peer} returned {} headers, {} with AuxPoW bit",
        headers.len(), auxpow_bit_count
    );

    // Build the result vec. Slots for non-AuxPoW headers are filled
    // immediately; slots for AuxPoW-bit headers wait for a matching
    // block message.
    let mut result: Vec<([u8; 80], Option<Vec<u8>>)> = Vec::with_capacity(headers.len());
    let mut pending: HashMap<[u8; 32], usize> = HashMap::new();
    for (i, header) in headers.iter().enumerate() {
        let version = i32::from_le_bytes(header[..4].try_into().unwrap());
        let has_auxpow_bit = (version as u32) & 0x100 != 0;
        result.push((*header, None));
        if has_auxpow_bit {
            let block_hash = sha256d(header);
            pending.insert(block_hash, i);
        }
    }

    if pending.is_empty() {
        return Ok(result);
    }

    // Issue getdata batches and drain block responses.
    let mut all_hashes: Vec<[u8; 32]> = pending.keys().copied().collect();
    while !all_hashes.is_empty() {
        let chunk: Vec<[u8; 32]> = all_hashes.drain(..all_hashes.len().min(INV_BATCH)).collect();
        let invs: Vec<(u32, [u8; 32])> = chunk.iter().map(|h| (MSG_BLOCK, *h)).collect();
        let payload = build_getdata_payload(&invs);
        eprintln!(
            "[doge-p2p] peer {peer} sending getdata for {} blocks (batch)",
            chunk.len()
        );
        write_message(&mut stream, DOGE_MAGIC, "getdata", &payload)
            .await
            .map_err(|e| format!("send getdata: {e}"))?;

        let mut received_in_batch = 0usize;
        for response_idx in 0..MAX_BLOCK_RESPONSE_MESSAGES {
            if received_in_batch >= chunk.len() {
                break;
            }
            // v1.9.81 diagnostic: log before each read so we can see
            // exactly which response number times out.
            eprintln!(
                "[doge-p2p] peer {peer} waiting for block {}/{} (msg #{response_idx})",
                received_in_batch + 1,
                chunk.len()
            );
            let msg = read_message(&mut stream, DOGE_MAGIC)
                .await
                .map_err(|e| {
                    let err = format!("post-getdata read: {e}");
                    eprintln!(
                        "[doge-p2p] peer {peer} read error after {received_in_batch}/{} blocks: {err}",
                        chunk.len()
                    );
                    err
                })?;
            match msg.command.as_str() {
                "block" => {
                    let (block_header, auxpow_bytes) =
                        parse_block_prefix_for_auxpow(&msg.payload)
                            .map_err(|e| format!("block prefix parse: {e}"))?;
                    let block_hash = sha256d(&block_header);
                    if let Some(&slot) = pending.get(&block_hash) {
                        result[slot] = (block_header, auxpow_bytes);
                        pending.remove(&block_hash);
                        received_in_batch += 1;
                    }
                    // Unmatched block: peer sent an unrelated block;
                    // ignore (rare, but cheap to tolerate).
                }
                "ping" => {
                    write_message(&mut stream, DOGE_MAGIC, "pong", &msg.payload)
                        .await
                        .map_err(|e| format!("send pong post-getdata: {e}"))?;
                }
                "notfound" => {
                    // Peer doesn't have one of the requested blocks.
                    // Stop draining this batch — caller will rotate.
                    return Err(format!(
                        "peer returned notfound after {} blocks in batch",
                        received_in_batch
                    ));
                }
                _ => {
                    // INV / ADDR / etc — ignore.
                }
            }
        }
        if received_in_batch < chunk.len() {
            return Err(format!(
                "block-response batch short: received {}/{} after {} messages",
                received_in_batch,
                chunk.len(),
                MAX_BLOCK_RESPONSE_MESSAGES
            ));
        }
    }

    if !pending.is_empty() {
        return Err(format!(
            "missing {} AuxPoW block responses",
            pending.len()
        ));
    }
    Ok(result)
}
