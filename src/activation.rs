use std::env;

/// Mainnet HTLCv1 activation height source-of-truth.
///
/// Set this to `Some(<height>)` only after activation governance is complete.
/// `None` keeps HTLCv1 disabled on mainnet.
pub const MAINNET_HTLCV1_ACTIVATION_HEIGHT: Option<u64> = Some(18677);

/// Mainnet LWMA difficulty activation height source-of-truth.
///
/// Mainnet LWMA has been active since block height 16,462.
/// Historical consensus from that height onward must remain unchanged.
pub const MAINNET_LWMA_ACTIVATION_HEIGHT: Option<u64> = Some(16_462);

/// Mainnet LWMA v2 activation height source-of-truth.
///
/// INACTIVE by default. Set to Some(<height>) only after governance review
/// and explicit approval. When active, switches difficulty to LWMA v2
/// parameters (N=30, clamp=10T) for faster post-collapse recovery.
/// Historical consensus before this height is unaffected.
pub const MAINNET_LWMA_V2_ACTIVATION_HEIGHT: Option<u64> = Some(19_740);

/// Mainnet AuxPoW merged-mining activation height.
///
/// At this height the chain begins accepting blocks that carry a Namecoin
/// AuxPoW extension (version bit 1<<8). Standard single-hash PoW blocks
/// remain valid after activation.
///
/// Height 26500 is approximately 6 weeks after height 20299 (when this
/// constant was set), giving all known node operators time to upgrade
/// before the first AuxPoW block can appear.
pub const MAINNET_AUXPOW_ACTIVATION_HEIGHT: Option<u64> = Some(26_500);

/// Mainnet Bitcoin SPV header relay activation height (Phase 1).
///
/// `None` keeps the BTC SPV header relay disabled on mainnet. When this is
/// set to `Some(<height>)`, iriumd blocks at or after that height may carry
/// a `BtcHeaderBatch` output (script tag `0xc4`) and the validator will
/// apply such batches into `ChainState.btc_headers`.
///
/// Phase 1 ships disabled. Activation requires a dedicated commit and
/// release per the workflow in docs/htlcv1_activation_commit_workflow.md.
pub const MAINNET_BTC_SPV_RELAY_ACTIVATION_HEIGHT: Option<u64> = Some(23_850);

/// Mainnet anchor for the BTC SPV header relay.
///
/// All four values are zero until the relay is activated on mainnet. They
/// must be set together (a known finalized BTC mainnet block) at the same
/// time as `MAINNET_BTC_SPV_RELAY_ACTIVATION_HEIGHT`.
#[allow(dead_code)] // anchor placeholder; populated by the Phase 1 activation commit
pub const MAINNET_BTC_ANCHOR_HEIGHT: u64 = 880_000;
#[allow(dead_code)] // anchor placeholder; populated by the Phase 1 activation commit
pub const MAINNET_BTC_ANCHOR_HASH: [u8; 32] = [
    // Bitcoin mainnet block 880000 hash in NATURAL byte order
    // (display hex 000000000000000000010b17283c3c400507969a9c2afd1dcf2082ec5cca2880
    // reversed - chain-linkage checks compare to header.prev_hash which is also
    // stored in natural order).
    0x80, 0x28, 0xca, 0x5c, 0xec, 0x82, 0x20, 0xcf,
    0x1d, 0xfd, 0x2a, 0x9c, 0x9a, 0x96, 0x07, 0x05,
    0x40, 0x3c, 0x3c, 0x28, 0x17, 0x0b, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
#[allow(dead_code)] // anchor placeholder; populated by the Phase 1 activation commit
pub const MAINNET_BTC_ANCHOR_BITS: u32 = 0x17028c61;
#[allow(dead_code)] // anchor placeholder; populated by the Phase 1 activation commit
pub const MAINNET_BTC_ANCHOR_TIME: u32 = 1_737_337_343;

/// Mainnet Litecoin SPV header relay activation height (Phase B).
///
/// `None` keeps the LTC SPV header relay disabled on mainnet. When set to
/// `Some(<height>)`, iriumd blocks at or after that height may carry an
/// `LtcHeaderBatch` output (script tag `0xc6`) and the validator will
/// apply such batches into `ChainState.ltc_headers`.
///
/// Phase B ships disabled. Activation requires a dedicated commit per the
/// same workflow as Phase 1.
pub const MAINNET_LTC_SPV_RELAY_ACTIVATION_HEIGHT: Option<u64> = None;

/// Mainnet anchor for the LTC SPV header relay.
///
/// Litecoin mainnet block 3_106_656 (a 2016-block retarget boundary
/// chosen well-confirmed at pick time). Hash stored here in DISPLAY order
/// for readability; reversed to natural byte order in
/// `LtcAnchor::mainnet()` so it lines up with `prev_hash` chain-linkage
/// fields. These constants take effect only after governance flips
/// `MAINNET_LTC_SPV_RELAY_ACTIVATION_HEIGHT` to `Some(<height>)`.
#[allow(dead_code)] // wired through ChainParams once Phase B callers come online
pub const MAINNET_LTC_ANCHOR_HEIGHT: u64 = 3_106_656;
#[allow(dead_code)]
pub const MAINNET_LTC_ANCHOR_HASH_DISPLAY: [u8; 32] = [
    0x8a, 0x89, 0xd2, 0xe5, 0x23, 0x29, 0xaa, 0xbe,
    0x63, 0xfa, 0xbe, 0xb9, 0xd4, 0xcf, 0x73, 0x4d,
    0x8a, 0x44, 0xde, 0x15, 0x85, 0x98, 0xaf, 0xb6,
    0x56, 0x0f, 0x20, 0xf8, 0xc9, 0x47, 0xbe, 0x64,
];
#[allow(dead_code)]
pub const MAINNET_LTC_ANCHOR_BITS: u32 = 0x1929_b619;
#[allow(dead_code)]
pub const MAINNET_LTC_ANCHOR_TIME: u32 = 1_778_676_649;

/// Mainnet HtlcLtcSwapV1 activation height (Phase C).
///
/// `None` keeps the LTC-proof claim path disabled on mainnet. When set
/// to `Some(<height>)`, blocks at or after that height may carry
/// HtlcLtcSwapV1 outputs (script tag `0xc7`) and the validator will
/// accept LTC-proof claim witnesses against them.
///
/// Phase C ships disabled. Activation should not precede
/// `MAINNET_LTC_SPV_RELAY_ACTIVATION_HEIGHT`, otherwise no proof would
/// resolve.
pub const MAINNET_HTLC_LTC_SWAP_V1_ACTIVATION_HEIGHT: Option<u64> = None;

/// Mainnet HtlcBtcSwapV1 activation height (Phase 2).
///
/// `None` keeps the BTC-proof claim path disabled on mainnet. When set to
/// `Some(<height>)`, blocks at or after that height may carry HtlcBtcSwapV1
/// outputs (script tag `0xc3`) and the validator will accept BTC-proof
/// claim witnesses against them.
///
/// Phase 2 ships disabled. Activation requires:
/// 1. The BTC SPV relay being active (so headers and merkle proofs can be
///    verified). Setting this height before
///    `MAINNET_BTC_SPV_RELAY_ACTIVATION_HEIGHT` is meaningless because no
///    proofs would resolve.
/// 2. A dedicated activation commit per the workflow in
///    docs/htlcv1_activation_commit_workflow.md.
pub const MAINNET_HTLC_BTC_SWAP_V1_ACTIVATION_HEIGHT: Option<u64> = Some(23_850);

/// Mainnet SwapOrder activation height (Phase 3).
///
/// `None` keeps the on-chain order book disabled on mainnet. When set to
/// `Some(<height>)`, blocks at or after that height may carry SwapOrder
/// outputs (script tag `0xc5`) and the validator will accept Fill /
/// Cancel / ExpireSweep witnesses against them.
///
/// Phase 3 ships disabled. Sell-direction fills emit `HtlcBtcSwapV1`
/// outputs, so this should not be activated before HtlcBtcSwapV1 — the
/// fill covenant would otherwise reject every spend.
pub const MAINNET_SWAP_ORDER_V1_ACTIVATION_HEIGHT: Option<u64> = Some(23_850);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkKind {
    Mainnet,
    Testnet,
    Devnet,
}

impl NetworkKind {
    pub fn from_env_value(v: &str) -> Self {
        match v.trim().to_ascii_lowercase().as_str() {
            "testnet" => Self::Testnet,
            "devnet" | "regtest" | "trial" => Self::Devnet,
            _ => Self::Mainnet,
        }
    }
}

pub fn network_kind_from_env() -> NetworkKind {
    env::var("IRIUM_NETWORK")
        .map(|v| NetworkKind::from_env_value(&v))
        .unwrap_or(NetworkKind::Mainnet)
}

pub fn runtime_htlcv1_env_override() -> Option<u64> {
    env::var("IRIUM_HTLCV1_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

pub fn runtime_lwma_env_override() -> Option<u64> {
    env::var("IRIUM_LWMA_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

pub fn runtime_lwma_v2_env_override() -> Option<u64> {
    env::var("IRIUM_LWMA_V2_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

pub fn runtime_auxpow_env_override() -> Option<u64> {
    env::var("IRIUM_AUXPOW_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

pub fn resolved_htlcv1_activation_height(network: NetworkKind) -> Option<u64> {
    match network {
        NetworkKind::Mainnet => MAINNET_HTLCV1_ACTIVATION_HEIGHT,
        NetworkKind::Testnet | NetworkKind::Devnet => runtime_htlcv1_env_override(),
    }
}

pub fn resolved_lwma_activation_height(network: NetworkKind) -> Option<u64> {
    match network {
        NetworkKind::Mainnet => MAINNET_LWMA_ACTIVATION_HEIGHT,
        NetworkKind::Testnet | NetworkKind::Devnet => runtime_lwma_env_override(),
    }
}

pub fn resolved_lwma_v2_activation_height(network: NetworkKind) -> Option<u64> {
    match network {
        NetworkKind::Mainnet => MAINNET_LWMA_V2_ACTIVATION_HEIGHT,
        NetworkKind::Testnet | NetworkKind::Devnet => runtime_lwma_v2_env_override(),
    }
}

pub fn resolved_auxpow_activation_height(network: NetworkKind) -> Option<u64> {
    match network {
        NetworkKind::Mainnet => MAINNET_AUXPOW_ACTIVATION_HEIGHT,
        NetworkKind::Testnet | NetworkKind::Devnet => runtime_auxpow_env_override(),
    }
}

/// Devnet/testnet override for the BTC SPV header relay activation height.
/// Read from `IRIUM_BTC_SPV_RELAY_ACTIVATION_HEIGHT`. Ignored on mainnet.
#[allow(dead_code)] // wired through ChainParams once Phase 1 callers come online
pub fn runtime_btc_spv_relay_env_override() -> Option<u64> {
    env::var("IRIUM_BTC_SPV_RELAY_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

#[allow(dead_code)] // wired through ChainParams once Phase 1 callers come online
pub fn resolved_btc_spv_relay_activation_height(network: NetworkKind) -> Option<u64> {
    match network {
        NetworkKind::Mainnet => MAINNET_BTC_SPV_RELAY_ACTIVATION_HEIGHT,
        NetworkKind::Testnet | NetworkKind::Devnet => runtime_btc_spv_relay_env_override(),
    }
}

/// Devnet/testnet override for the LTC SPV header relay activation height.
/// Read from `IRIUM_LTC_SPV_RELAY_ACTIVATION_HEIGHT`. Ignored on mainnet.
#[allow(dead_code)] // wired through ChainParams once Phase B callers come online
pub fn runtime_ltc_spv_relay_env_override() -> Option<u64> {
    env::var("IRIUM_LTC_SPV_RELAY_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

#[allow(dead_code)] // wired through ChainParams once Phase B callers come online
pub fn resolved_ltc_spv_relay_activation_height(network: NetworkKind) -> Option<u64> {
    match network {
        NetworkKind::Mainnet => MAINNET_LTC_SPV_RELAY_ACTIVATION_HEIGHT,
        NetworkKind::Testnet | NetworkKind::Devnet => runtime_ltc_spv_relay_env_override(),
    }
}

/// Devnet/testnet override for the HtlcLtcSwapV1 activation height.
/// Read from `IRIUM_HTLC_LTC_SWAP_V1_ACTIVATION_HEIGHT`. Ignored on mainnet.
#[allow(dead_code)] // wired through ChainParams once Phase C callers come online
pub fn runtime_htlc_ltc_swap_v1_env_override() -> Option<u64> {
    env::var("IRIUM_HTLC_LTC_SWAP_V1_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

#[allow(dead_code)] // wired through ChainParams once Phase C callers come online
pub fn resolved_htlc_ltc_swap_v1_activation_height(network: NetworkKind) -> Option<u64> {
    match network {
        NetworkKind::Mainnet => MAINNET_HTLC_LTC_SWAP_V1_ACTIVATION_HEIGHT,
        NetworkKind::Testnet | NetworkKind::Devnet => runtime_htlc_ltc_swap_v1_env_override(),
    }
}

/// Devnet/testnet override for the HtlcBtcSwapV1 activation height.
/// Read from `IRIUM_HTLC_BTC_SWAP_V1_ACTIVATION_HEIGHT`. Ignored on mainnet.
#[allow(dead_code)] // wired through ChainParams once Phase 2 callers come online
pub fn runtime_htlc_btc_swap_v1_env_override() -> Option<u64> {
    env::var("IRIUM_HTLC_BTC_SWAP_V1_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

#[allow(dead_code)] // wired through ChainParams once Phase 2 callers come online
pub fn resolved_htlc_btc_swap_v1_activation_height(network: NetworkKind) -> Option<u64> {
    match network {
        NetworkKind::Mainnet => MAINNET_HTLC_BTC_SWAP_V1_ACTIVATION_HEIGHT,
        NetworkKind::Testnet | NetworkKind::Devnet => runtime_htlc_btc_swap_v1_env_override(),
    }
}

/// Devnet/testnet override for the SwapOrder activation height.
/// Read from `IRIUM_SWAP_ORDER_V1_ACTIVATION_HEIGHT`. Ignored on mainnet.
#[allow(dead_code)] // wired through ChainParams once Phase 3 callers come online
pub fn runtime_swap_order_v1_env_override() -> Option<u64> {
    env::var("IRIUM_SWAP_ORDER_V1_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

#[allow(dead_code)] // wired through ChainParams once Phase 3 callers come online
pub fn resolved_swap_order_v1_activation_height(network: NetworkKind) -> Option<u64> {
    match network {
        NetworkKind::Mainnet => MAINNET_SWAP_ORDER_V1_ACTIVATION_HEIGHT,
        NetworkKind::Testnet | NetworkKind::Devnet => runtime_swap_order_v1_env_override(),
    }
}

/// Mainnet MPSOv1 (M-of-N multisig output) activation height.
///
/// Activated at block 20,000. No MPSO outputs exist before this height.
#[allow(dead_code)] // protocol constant: MPSOv1 activation height on mainnet
pub const MAINNET_MPSOV1_ACTIVATION_HEIGHT: Option<u64> = Some(20_000);

#[allow(dead_code)] // env override for testing MPSOv1 activation on non-mainnet networks
pub fn runtime_mpsov1_env_override() -> Option<u64> {
    env::var("IRIUM_MPSOV1_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

#[allow(dead_code)] // public resolver for MPSOv1 activation height; used by wallet and block validators once MPSOv1 ships
pub fn resolved_mpsov1_activation_height(network: NetworkKind) -> Option<u64> {
    match network {
        NetworkKind::Mainnet => MAINNET_MPSOV1_ACTIVATION_HEIGHT,
        NetworkKind::Testnet | NetworkKind::Devnet => runtime_mpsov1_env_override(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn mainnet_ignores_htlc_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_HTLCV1_ACTIVATION_HEIGHT", "42");
        let resolved = resolved_htlcv1_activation_height(NetworkKind::Mainnet);
        std::env::remove_var("IRIUM_HTLCV1_ACTIVATION_HEIGHT");
        assert_eq!(resolved, MAINNET_HTLCV1_ACTIVATION_HEIGHT);
    }

    #[test]
    fn non_mainnet_uses_htlc_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_HTLCV1_ACTIVATION_HEIGHT", "42");
        assert_eq!(
            resolved_htlcv1_activation_height(NetworkKind::Devnet),
            Some(42)
        );
        assert_eq!(
            resolved_htlcv1_activation_height(NetworkKind::Testnet),
            Some(42)
        );
        std::env::remove_var("IRIUM_HTLCV1_ACTIVATION_HEIGHT");
    }

    #[test]
    fn mainnet_ignores_lwma_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_LWMA_ACTIVATION_HEIGHT", "42");
        let resolved = resolved_lwma_activation_height(NetworkKind::Mainnet);
        std::env::remove_var("IRIUM_LWMA_ACTIVATION_HEIGHT");
        assert_eq!(resolved, MAINNET_LWMA_ACTIVATION_HEIGHT);
    }

    #[test]
    fn non_mainnet_uses_lwma_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_LWMA_ACTIVATION_HEIGHT", "42");
        assert_eq!(
            resolved_lwma_activation_height(NetworkKind::Devnet),
            Some(42)
        );
        assert_eq!(
            resolved_lwma_activation_height(NetworkKind::Testnet),
            Some(42)
        );
        std::env::remove_var("IRIUM_LWMA_ACTIVATION_HEIGHT");
    }

    #[test]
    fn mainnet_lwma_v2_activation_height_is_set() {
        assert_eq!(
            MAINNET_LWMA_V2_ACTIVATION_HEIGHT,
            Some(19_740),
            "LWMA v2 mainnet activation height must be 19740"
        );
        assert_eq!(
            resolved_lwma_v2_activation_height(NetworkKind::Mainnet),
            Some(19_740),
            "resolved v2 height must be Some(19740) for mainnet"
        );
    }

    #[test]
    fn mainnet_ignores_lwma_v2_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_LWMA_V2_ACTIVATION_HEIGHT", "99999");
        let resolved = resolved_lwma_v2_activation_height(NetworkKind::Mainnet);
        std::env::remove_var("IRIUM_LWMA_V2_ACTIVATION_HEIGHT");
        assert_eq!(resolved, MAINNET_LWMA_V2_ACTIVATION_HEIGHT);
        assert_eq!(
            resolved,
            Some(19_740),
            "mainnet v2 height must be code-defined 19740, not env override 99999"
        );
    }

    #[test]
    fn non_mainnet_uses_lwma_v2_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_LWMA_V2_ACTIVATION_HEIGHT", "500");
        assert_eq!(
            resolved_lwma_v2_activation_height(NetworkKind::Devnet),
            Some(500)
        );
        assert_eq!(
            resolved_lwma_v2_activation_height(NetworkKind::Testnet),
            Some(500)
        );
        std::env::remove_var("IRIUM_LWMA_V2_ACTIVATION_HEIGHT");
    }

    #[test]
    fn mainnet_auxpow_activation_height_is_26500() {
        assert_eq!(
            MAINNET_AUXPOW_ACTIVATION_HEIGHT,
            Some(26_500),
            "AuxPoW mainnet activation height must be 26500"
        );
        assert_eq!(
            resolved_auxpow_activation_height(NetworkKind::Mainnet),
            Some(26_500)
        );
    }

    #[test]
    fn mainnet_ignores_auxpow_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_AUXPOW_ACTIVATION_HEIGHT", "99999");
        let resolved = resolved_auxpow_activation_height(NetworkKind::Mainnet);
        std::env::remove_var("IRIUM_AUXPOW_ACTIVATION_HEIGHT");
        assert_eq!(resolved, MAINNET_AUXPOW_ACTIVATION_HEIGHT);
    }

    #[test]
    fn non_mainnet_uses_auxpow_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_AUXPOW_ACTIVATION_HEIGHT", "1000");
        assert_eq!(
            resolved_auxpow_activation_height(NetworkKind::Devnet),
            Some(1000)
        );
        assert_eq!(
            resolved_auxpow_activation_height(NetworkKind::Testnet),
            Some(1000)
        );
        std::env::remove_var("IRIUM_AUXPOW_ACTIVATION_HEIGHT");
    }

    #[test]
    fn mainnet_btc_spv_relay_height_is_23850() {
        assert_eq!(
            MAINNET_BTC_SPV_RELAY_ACTIVATION_HEIGHT, Some(23_850),
            "Phase 1 activated on mainnet at height 23850"
        );
        assert_eq!(
            resolved_btc_spv_relay_activation_height(NetworkKind::Mainnet),
            Some(23_850)
        );
    }

    #[test]
    fn mainnet_ignores_btc_spv_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_BTC_SPV_RELAY_ACTIVATION_HEIGHT", "12345");
        let resolved = resolved_btc_spv_relay_activation_height(NetworkKind::Mainnet);
        std::env::remove_var("IRIUM_BTC_SPV_RELAY_ACTIVATION_HEIGHT");
        assert_eq!(resolved, MAINNET_BTC_SPV_RELAY_ACTIVATION_HEIGHT);
        assert_eq!(resolved, Some(23_850));
    }

    #[test]
    fn non_mainnet_uses_btc_spv_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_BTC_SPV_RELAY_ACTIVATION_HEIGHT", "50");
        assert_eq!(
            resolved_btc_spv_relay_activation_height(NetworkKind::Devnet),
            Some(50)
        );
        assert_eq!(
            resolved_btc_spv_relay_activation_height(NetworkKind::Testnet),
            Some(50)
        );
        std::env::remove_var("IRIUM_BTC_SPV_RELAY_ACTIVATION_HEIGHT");
    }

    #[test]
    fn mainnet_htlc_btc_swap_v1_height_is_23850() {
        assert_eq!(
            MAINNET_HTLC_BTC_SWAP_V1_ACTIVATION_HEIGHT, Some(23_850),
            "Phase 2 activated on mainnet at height 23850"
        );
        assert_eq!(
            resolved_htlc_btc_swap_v1_activation_height(NetworkKind::Mainnet),
            Some(23_850)
        );
    }

    #[test]
    fn mainnet_ignores_htlc_btc_swap_v1_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_HTLC_BTC_SWAP_V1_ACTIVATION_HEIGHT", "777");
        let resolved = resolved_htlc_btc_swap_v1_activation_height(NetworkKind::Mainnet);
        std::env::remove_var("IRIUM_HTLC_BTC_SWAP_V1_ACTIVATION_HEIGHT");
        assert_eq!(resolved, MAINNET_HTLC_BTC_SWAP_V1_ACTIVATION_HEIGHT);
        assert_eq!(resolved, Some(23_850));
    }

    #[test]
    fn non_mainnet_uses_htlc_btc_swap_v1_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_HTLC_BTC_SWAP_V1_ACTIVATION_HEIGHT", "777");
        assert_eq!(
            resolved_htlc_btc_swap_v1_activation_height(NetworkKind::Devnet),
            Some(777)
        );
        assert_eq!(
            resolved_htlc_btc_swap_v1_activation_height(NetworkKind::Testnet),
            Some(777)
        );
        std::env::remove_var("IRIUM_HTLC_BTC_SWAP_V1_ACTIVATION_HEIGHT");
    }

    #[test]
    fn mainnet_swap_order_v1_height_is_23850() {
        assert_eq!(
            MAINNET_SWAP_ORDER_V1_ACTIVATION_HEIGHT, Some(23_850),
            "Phase 3 activated on mainnet at height 23850"
        );
        assert_eq!(
            resolved_swap_order_v1_activation_height(NetworkKind::Mainnet),
            Some(23_850)
        );
    }

    #[test]
    fn mainnet_ignores_swap_order_v1_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_SWAP_ORDER_V1_ACTIVATION_HEIGHT", "4242");
        let resolved = resolved_swap_order_v1_activation_height(NetworkKind::Mainnet);
        std::env::remove_var("IRIUM_SWAP_ORDER_V1_ACTIVATION_HEIGHT");
        assert_eq!(resolved, MAINNET_SWAP_ORDER_V1_ACTIVATION_HEIGHT);
        assert_eq!(resolved, Some(23_850));
    }

    #[test]
    fn non_mainnet_uses_swap_order_v1_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_SWAP_ORDER_V1_ACTIVATION_HEIGHT", "111");
        assert_eq!(
            resolved_swap_order_v1_activation_height(NetworkKind::Devnet),
            Some(111)
        );
        assert_eq!(
            resolved_swap_order_v1_activation_height(NetworkKind::Testnet),
            Some(111)
        );
        std::env::remove_var("IRIUM_SWAP_ORDER_V1_ACTIVATION_HEIGHT");
    }

    #[test]
    fn mainnet_ltc_spv_height_is_none_pending_governance() {
        assert!(
            MAINNET_LTC_SPV_RELAY_ACTIVATION_HEIGHT.is_none(),
            "LTC SPV must stay disabled on mainnet until governance flips this constant"
        );
        assert!(resolved_ltc_spv_relay_activation_height(NetworkKind::Mainnet).is_none());
    }

    #[test]
    fn mainnet_ignores_ltc_spv_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_LTC_SPV_RELAY_ACTIVATION_HEIGHT", "5555");
        let resolved = resolved_ltc_spv_relay_activation_height(NetworkKind::Mainnet);
        std::env::remove_var("IRIUM_LTC_SPV_RELAY_ACTIVATION_HEIGHT");
        assert_eq!(resolved, MAINNET_LTC_SPV_RELAY_ACTIVATION_HEIGHT);
        assert!(resolved.is_none());
    }

    #[test]
    fn non_mainnet_uses_ltc_spv_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_LTC_SPV_RELAY_ACTIVATION_HEIGHT", "77");
        assert_eq!(
            resolved_ltc_spv_relay_activation_height(NetworkKind::Devnet),
            Some(77)
        );
        assert_eq!(
            resolved_ltc_spv_relay_activation_height(NetworkKind::Testnet),
            Some(77)
        );
        std::env::remove_var("IRIUM_LTC_SPV_RELAY_ACTIVATION_HEIGHT");
    }

    #[test]
    fn mainnet_ltc_anchor_constants_have_expected_values() {
        // Display-order hash (from litecoinspace.org / Litecoin Core RPC).
        // Reversed to natural order in `LtcAnchor::mainnet()`.
        assert_eq!(MAINNET_LTC_ANCHOR_HEIGHT, 3_106_656);
        assert_eq!(MAINNET_LTC_ANCHOR_BITS, 0x1929_b619);
        assert_eq!(MAINNET_LTC_ANCHOR_TIME, 1_778_676_649);
        assert_eq!(MAINNET_LTC_ANCHOR_HASH_DISPLAY[0], 0x8a);
        assert_eq!(MAINNET_LTC_ANCHOR_HASH_DISPLAY[31], 0x64);
    }

    #[test]
    fn mainnet_htlc_ltc_swap_v1_height_is_none_pending_governance() {
        assert!(
            MAINNET_HTLC_LTC_SWAP_V1_ACTIVATION_HEIGHT.is_none(),
            "HtlcLtcSwapV1 must stay disabled on mainnet until governance flips this constant"
        );
        assert!(resolved_htlc_ltc_swap_v1_activation_height(NetworkKind::Mainnet).is_none());
    }

    #[test]
    fn mainnet_ignores_htlc_ltc_swap_v1_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_HTLC_LTC_SWAP_V1_ACTIVATION_HEIGHT", "8888");
        let resolved = resolved_htlc_ltc_swap_v1_activation_height(NetworkKind::Mainnet);
        std::env::remove_var("IRIUM_HTLC_LTC_SWAP_V1_ACTIVATION_HEIGHT");
        assert_eq!(resolved, MAINNET_HTLC_LTC_SWAP_V1_ACTIVATION_HEIGHT);
        assert!(resolved.is_none());
    }

    #[test]
    fn non_mainnet_uses_htlc_ltc_swap_v1_env_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("IRIUM_HTLC_LTC_SWAP_V1_ACTIVATION_HEIGHT", "99");
        assert_eq!(
            resolved_htlc_ltc_swap_v1_activation_height(NetworkKind::Devnet),
            Some(99)
        );
        assert_eq!(
            resolved_htlc_ltc_swap_v1_activation_height(NetworkKind::Testnet),
            Some(99)
        );
        std::env::remove_var("IRIUM_HTLC_LTC_SWAP_V1_ACTIVATION_HEIGHT");
    }
}
