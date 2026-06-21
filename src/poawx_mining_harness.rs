//! Phase 24K: Irium-native PoAW-X all-gates mining submit harness (helpers).
//!
//! Phase 24J proved the remaining blocker for a real all-gates block was mining
//! TOOLING, not PoAW-X consensus: stock cpuminer/minerd hashes a standard
//! Bitcoin 80-byte header with `sha256d`, but Irium hashes the block header via
//! [`crate::block::BlockHeader::hash_for_height`] (a height-bound serialization),
//! so minerd's shares never match Irium's PoW target. The fix is to mine with
//! Irium's ACTUAL PoW hash.
//!
//! This module provides the small, reusable, **mainnet-hard-off** primitives a
//! devnet/testnet harness (in-process test today, an explicit dev binary for a
//! future live Phase 24L) needs to produce a real all-gates block:
//!   * [`guard_network`] — refuse mainnet (`network_id == 0`); require an
//!     explicit devnet/testnet id.
//!   * [`guard_isolated_storage`] — refuse missing/default/`/tmp` runtime
//!     storage; require an explicit isolated dir that is NOT the production
//!     `$HOME/.irium`.
//!   * [`mine_pow`] — grind the header nonce using Irium's REAL PoW path
//!     ([`hash_for_height`](crate::block::BlockHeader::hash_for_height) +
//!     [`meets_target`](crate::pow::meets_target)) until it satisfies the
//!     supplied target. This NEVER touches LWMA / difficulty / target logic —
//!     it only searches a nonce against an externally-decided target.
//!
//! Nothing here holds or prints private keys, seeds, or VRF secrets: the PoW
//! grind takes only a header + a target, and the guards take only a network id
//! and a path. The heavy all-gates block assembly + authoritative-validator
//! checks live in the Phase 24K chain tests, which reuse these primitives.

use crate::block::BlockHeader;
use crate::pow::{meets_target, Target};
use std::path::Path;

/// Refuse to run the harness on mainnet. Mainnet is `network_id == 0`
/// ([`crate::activation::NetworkKind::id_byte`]). Only testnet (1) and devnet
/// (2) are permitted. The caller MUST pass the resolved network id; this is the
/// single mainnet-hard-off choke point for every harness entry point.
pub fn guard_network(network_id: u8) -> Result<(), String> {
    match network_id {
        0 => Err(
            "poawx mining harness: refusing to run on mainnet (network_id==0); \
             require explicit devnet/testnet"
                .to_string(),
        ),
        1 | 2 => Ok(()),
        other => Err(format!(
            "poawx mining harness: unknown network_id {other}; require testnet(1)/devnet(2)"
        )),
    }
}

/// Refuse to use runtime storage unless it is an EXPLICIT isolated directory.
///
/// `None` (no dir chosen) is rejected: a runtime harness must be pointed at an
/// explicit isolated dir, never a default. A `/tmp` path is rejected (the
/// project rule forbids `/tmp` for Irium storage). The production default
/// `$HOME/.irium` (and any path whose final component is `.irium` directly under
/// `$HOME`) is rejected so the harness can never clobber real node/wallet data.
///
/// Pure in-process tests do not use runtime storage and need not call this; it
/// exists so a future live (Phase 24L) harness fails closed before touching
/// disk.
pub fn guard_isolated_storage(dir: Option<&Path>) -> Result<(), String> {
    let dir = dir.ok_or_else(|| {
        "poawx mining harness: runtime storage requires an explicit isolated dir \
         (none provided)"
            .to_string()
    })?;
    if dir.starts_with("/tmp") {
        return Err(format!(
            "poawx mining harness: refusing /tmp storage dir {}",
            dir.display()
        ));
    }
    if let Ok(home) = std::env::var("HOME") {
        let home = Path::new(&home);
        let default_dir = home.join(".irium");
        if dir == default_dir {
            return Err(format!(
                "poawx mining harness: refusing the production default dir {} \
                 (use an explicit isolated dir)",
                dir.display()
            ));
        }
        // Also refuse a bare `$HOME/.irium`-style default expressed differently.
        if dir.parent() == Some(home) && dir.file_name().and_then(|s| s.to_str()) == Some(".irium")
        {
            return Err(format!(
                "poawx mining harness: refusing a $HOME/.irium default dir {}",
                dir.display()
            ));
        }
    }
    Ok(())
}

/// Mine Irium's REAL proof-of-work for `header` at `height`: search the nonce
/// space `[0, max_iters)` until `hash_for_height(height)` satisfies `target`
/// (the EXACT check `validate_block_header` performs). On success the header's
/// `nonce` is set and the winning nonce is returned. Returns `Err` if the range
/// is exhausted.
///
/// This is the Irium-native counterpart to stock cpuminer's wrong-algorithm
/// grind: it calls the same hashing path the node validator uses, so a mined
/// header is accepted by the authoritative PoW check. It does NOT read or modify
/// any LWMA/difficulty/target state — `target` is supplied by the caller (who
/// derived it from the chain via `target_for_height`).
pub fn mine_pow(
    header: &mut BlockHeader,
    height: u64,
    target: Target,
    max_iters: u64,
) -> Result<u32, String> {
    let cap = max_iters.min(u32::MAX as u64 + 1);
    for n in 0..cap {
        header.nonce = n as u32;
        if meets_target(&header.hash_for_height(height), target) {
            return Ok(header.nonce);
        }
    }
    Err(format!(
        "poawx mining harness: no nonce satisfied target in {cap} iterations"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_network_rejects_mainnet() {
        // E1: harness refuses mainnet; accepts only testnet/devnet.
        assert!(guard_network(0).is_err(), "mainnet (0) must be refused");
        assert!(guard_network(1).is_ok(), "testnet (1) allowed");
        assert!(guard_network(2).is_ok(), "devnet (2) allowed");
        assert!(guard_network(7).is_err(), "unknown id refused");
        // E18: the rejection message names no secret/key material.
        let msg = guard_network(0).unwrap_err().to_lowercase();
        assert!(!msg.contains("secret") && !msg.contains("private") && !msg.contains("mnemonic"));
    }

    #[test]
    fn guard_isolated_storage_refuses_default_and_missing() {
        // E2: missing dir refused.
        assert!(guard_isolated_storage(None).is_err(), "no dir refused");
        // /tmp refused.
        assert!(
            guard_isolated_storage(Some(Path::new("/tmp/irium-x"))).is_err(),
            "/tmp refused"
        );
        // $HOME/.irium (production default) refused.
        std::env::set_var("HOME", "/home/tester");
        assert!(
            guard_isolated_storage(Some(Path::new("/home/tester/.irium"))).is_err(),
            "production default $HOME/.irium refused"
        );
        // an explicit isolated dir is accepted.
        assert!(
            guard_isolated_storage(Some(Path::new("/home/tester/irium-p24k-node"))).is_ok(),
            "explicit isolated dir accepted"
        );
    }

    #[test]
    fn mine_pow_finds_nonce_with_real_irium_hash() {
        // E11 (unit level): grinding the nonce with Irium's actual hash path
        // produces a header that satisfies the target via the SAME check the
        // node validator (`validate_block_header`) uses.
        let mut header = BlockHeader {
            version: 1,
            prev_hash: [0u8; 32],
            merkle_root: [0x11u8; 32],
            time: 1,
            // Easy target: high exponent, so a winning nonce is found quickly.
            bits: 0x1f00ffff,
            nonce: 0,
        };
        let height = 1u64;
        let target = header.target();
        let nonce = mine_pow(&mut header, height, target, 5_000_000).expect("mine a nonce");
        assert_eq!(header.nonce, nonce);
        assert!(
            meets_target(&header.hash_for_height(height), target),
            "mined header satisfies the real Irium PoW check"
        );
        // A trivially-impossible target is exhausted (fails closed, no panic).
        let mut h2 = header.clone();
        let impossible = Target { bits: 0x0300_0001 };
        assert!(mine_pow(&mut h2, height, impossible, 2_000).is_err());
    }
}
