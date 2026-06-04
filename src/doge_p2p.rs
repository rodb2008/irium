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
    build_getheaders_payload, build_version_payload, parse_headers_payload, read_message,
    sha256d, write_message,
};

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

/// Fetch DOGE block headers from a connected mainnet peer. `relay_tip_hash`
/// is what the chain currently holds (natural-byte-order, as stored in
/// `chain.doge_tip`); the peer returns up to 2000 headers chained from
/// the first hash in our locator that it also knows.
///
/// Returns the parsed 80-byte headers in network order. Returns an
/// empty Vec when the peer recognises our tip but has nothing newer to
/// send (we are caught up). Returns Err when every attempted peer
/// failed — the caller logs and the cycle retries on the next tick.
pub async fn fetch_headers(relay_tip_hash: [u8; 32]) -> Result<Vec<[u8; 80]>, String> {
    let peers = discover_peers().await?;
    let mut last_err = String::new();
    let mut attempts = 0usize;
    for peer in peers {
        if attempts >= MAX_PEER_ATTEMPTS {
            break;
        }
        attempts += 1;
        match try_peer(peer, relay_tip_hash).await {
            Ok(h) => return Ok(h),
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

/// Full session against one peer: connect, handshake, getheaders,
/// receive headers. Any step failing returns an error string the
/// caller logs and rotates to the next peer.
async fn try_peer(peer: SocketAddr, tip: [u8; 32]) -> Result<Vec<[u8; 80]>, String> {
    let mut stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(peer))
        .await
        .map_err(|_| "connect timeout".to_string())?
        .map_err(|e| format!("connect error: {e}"))?;

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
    for _ in 0..MAX_POST_GETHEADERS_MESSAGES {
        let msg = read_message(&mut stream, DOGE_MAGIC)
            .await
            .map_err(|e| format!("post-getheaders read: {e}"))?;
        match msg.command.as_str() {
            "headers" => {
                return parse_headers_payload(&msg.payload);
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
    Err("no headers received after MAX_POST_GETHEADERS_MESSAGES".to_string())
}
