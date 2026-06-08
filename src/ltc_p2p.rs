//! v1.9.67 — Native LTC P2P SPV header client.
//!
//! Speaks the Litecoin P2P protocol (magic `0xFBC0B6DB` little-endian
//! on the wire, port 9333) directly to mainnet peers discovered via
//! Litecoin Core's official DNS seeds. Replaces the public LTC API
//! HTTP path that v1.9.55-v1.9.66 used as the only LTC header source.
//!
//! Issue #60 phase 3. Mirrors `doge_p2p` exactly; only the network
//! constants differ. All wire-format work lives in `p2p_wire`.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::{lookup_host, TcpStream};
use tokio::time::timeout;

use crate::p2p_wire::{
    build_getheaders_payload, build_version_payload, parse_headers_payload, read_message,
    write_message,
};

/// LTC mainnet network magic. Wire bytes are `fb c0 b6 db`, which
/// `u32.to_le_bytes()` reproduces from this host integer.
const LTC_MAGIC: u32 = 0xDBB6C0FB;

/// LTC mainnet default P2P port.
const LTC_PORT: u16 = 9333;

/// LTC DNS seeds. Loshan + Thrasher + LitecoinTools are the canonical
/// seeds run by Litecoin Core developers; petertodd's seed is widely
/// mirrored. petertodd added because LTC seed coverage is thinner than
/// BTC and the extra source improves first-cycle reachability.
const LTC_SEEDS: &[&str] = &[
    "seed-a.litecoin.loshan.co.uk",
    "dnsseed.thrasher.io",
    "dnsseed.litecointools.com",
    "seed.ltc.petertodd.org",
];

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PEER_ATTEMPTS: usize = 20;
const MAX_HANDSHAKE_MESSAGES: usize = 30;
const MAX_POST_GETHEADERS_MESSAGES: usize = 30;

/// Fetch LTC block headers from a connected mainnet peer. See
/// `doge_p2p::fetch_headers` for the full semantics — this function is
/// the same code with per-chain constants substituted.
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
        "all {attempts} LTC peers failed; last error: {last_err}"
    ))
}

async fn discover_peers() -> Result<Vec<SocketAddr>, String> {
    use std::collections::HashSet;
    let mut seen: HashSet<SocketAddr> = HashSet::new();
    let mut out: Vec<SocketAddr> = Vec::new();
    for seed in LTC_SEEDS {
        let host = format!("{seed}:{LTC_PORT}");
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
        return Err("no LTC peers resolved from DNS seeds".to_string());
    }
    Ok(out)
}

async fn try_peer(peer: SocketAddr, tip: [u8; 32]) -> Result<Vec<[u8; 80]>, String> {
    let mut stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(peer))
        .await
        .map_err(|_| "connect timeout".to_string())?
        .map_err(|e| format!("connect error: {e}"))?;

    let vp = build_version_payload(0, LTC_PORT);
    write_message(&mut stream, LTC_MAGIC, "version", &vp)
        .await
        .map_err(|e| format!("send version: {e}"))?;

    let mut got_their_version = false;
    let mut got_their_verack = false;
    for _ in 0..MAX_HANDSHAKE_MESSAGES {
        if got_their_version && got_their_verack {
            break;
        }
        let msg = read_message(&mut stream, LTC_MAGIC)
            .await
            .map_err(|e| format!("handshake read: {e}"))?;
        match msg.command.as_str() {
            "version" => {
                got_their_version = true;
                write_message(&mut stream, LTC_MAGIC, "verack", &[])
                    .await
                    .map_err(|e| format!("send verack: {e}"))?;
            }
            "verack" => got_their_verack = true,
            "ping" => {
                write_message(&mut stream, LTC_MAGIC, "pong", &msg.payload)
                    .await
                    .map_err(|e| format!("send pong during handshake: {e}"))?;
            }
            _ => {}
        }
    }
    if !(got_their_version && got_their_verack) {
        return Err(format!(
            "incomplete handshake (version={got_their_version}, verack={got_their_verack})"
        ));
    }

    let payload = build_getheaders_payload(&[tip], [0u8; 32]);
    write_message(&mut stream, LTC_MAGIC, "getheaders", &payload)
        .await
        .map_err(|e| format!("send getheaders: {e}"))?;

    for _ in 0..MAX_POST_GETHEADERS_MESSAGES {
        let msg = read_message(&mut stream, LTC_MAGIC)
            .await
            .map_err(|e| format!("post-getheaders read: {e}"))?;
        match msg.command.as_str() {
            "headers" => {
                return parse_headers_payload(&msg.payload);
            }
            "ping" => {
                write_message(&mut stream, LTC_MAGIC, "pong", &msg.payload)
                    .await
                    .map_err(|e| format!("send pong post-getheaders: {e}"))?;
            }
            _ => {}
        }
    }
    Err("no headers received after MAX_POST_GETHEADERS_MESSAGES".to_string())
}
