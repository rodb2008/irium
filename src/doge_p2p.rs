//! v1.9.83 — Native DOGE P2P SPV header client.
//!
//! Speaks the Dogecoin P2P protocol (Bitcoin-compatible wire format
//! with `magic = 0xC0C0C0C0`, port 22556) directly to mainnet peers
//! discovered via Dogecoin Core's official DNS seeds.
//!
//! Issue #68 Option B: this module reverts to the BTC/LTC-shape
//! headers-only flow. PR-6's `getdata MSG_BLOCK` extension is removed
//! because real DOGE peers consistently dropped our getdata responses
//! despite a verified-correct probe. AuxPoW data is now fetched
//! out-of-band over HTTP (`header_sync::doge::fetch_doge_block_auxpow`)
//! using blockchair's `/raw/block/<height>` endpoint. The v1.9.82
//! `set_nodelay(true)` improvement is retained for low-latency wire
//! behavior (harmless if redundant).
//!
//! All wire-format work lives in `p2p_wire` so BTC, LTC, and DOGE are
//! byte-for-byte identical except for per-chain constants.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::{lookup_host, TcpStream};
use tokio::time::timeout;

use crate::p2p_wire::{
    build_getheaders_payload, build_version_payload, parse_headers_payload, read_message,
    write_message,
};

/// DOGE mainnet network magic. Wire bytes are `c0 c0 c0 c0`, which
/// `u32.to_le_bytes()` reproduces from this host integer.
const DOGE_MAGIC: u32 = 0xC0C0C0C0;

/// DOGE mainnet default P2P port.
const DOGE_PORT: u16 = 22556;

/// DOGE DNS seeds. Each resolves to a rotating list of node IPs run
/// by Dogecoin Core developers + community.
const DOGE_SEEDS: &[&str] = &[
    "seed.dogecoin.com",
    "seed.multidoge.org",
    "seed.dogechain.info",
];

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PEER_ATTEMPTS: usize = 20;
const MAX_HANDSHAKE_MESSAGES: usize = 30;
const MAX_POST_GETHEADERS_MESSAGES: usize = 30;

/// Fetch DOGE block headers from a connected mainnet peer. Mirror of
/// `btc_p2p::fetch_headers` and `ltc_p2p::fetch_headers` with DOGE
/// constants substituted.
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

async fn try_peer(peer: SocketAddr, tip: [u8; 32]) -> Result<Vec<[u8; 80]>, String> {
    let mut stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(peer))
        .await
        .map_err(|_| "connect timeout".to_string())?
        .map_err(|e| format!("connect error: {e}"))?;
    // v1.9.82 retained: disable Nagle for low-latency wire behavior.
    stream
        .set_nodelay(true)
        .map_err(|e| format!("set_nodelay: {e}"))?;

    let vp = build_version_payload(0, DOGE_PORT);
    write_message(&mut stream, DOGE_MAGIC, "version", &vp)
        .await
        .map_err(|e| format!("send version: {e}"))?;

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
                write_message(&mut stream, DOGE_MAGIC, "verack", &[])
                    .await
                    .map_err(|e| format!("send verack: {e}"))?;
            }
            "verack" => got_their_verack = true,
            "ping" => {
                write_message(&mut stream, DOGE_MAGIC, "pong", &msg.payload)
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
    write_message(&mut stream, DOGE_MAGIC, "getheaders", &payload)
        .await
        .map_err(|e| format!("send getheaders: {e}"))?;

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
            _ => {}
        }
    }
    Err("no headers received after MAX_POST_GETHEADERS_MESSAGES".to_string())
}
