//! In-process header sync helpers for the iriumd background tasks.
//!
//! Each submodule exposes async fetch helpers that talk to the public
//! external-chain block-explorer APIs (or a local regtest daemon's
//! JSON-RPC port) and return raw-hex header blobs ready for direct
//! submission via the corresponding `submit_*_headers_core` in iriumd.
//!
//! The standalone `src/bin/{btc,ltc,doge}-header-sync.rs` binaries
//! retain their own duplicate implementations using `reqwest::blocking`
//! and are kept for one-shot manual top-ups and as a fallback for
//! operators who can't upgrade iriumd immediately. New deployments
//! should rely on the iriumd-integrated tokio threads.

pub mod btc;
pub mod common;
pub mod doge;
pub mod ltc;
