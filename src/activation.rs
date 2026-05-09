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
/// Height 26347 is approximately 6 weeks after height 20299 (when this
/// constant was set), giving all known node operators time to upgrade
/// before the first AuxPoW block can appear.
pub const MAINNET_AUXPOW_ACTIVATION_HEIGHT: Option<u64> = Some(26_347);

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
    fn mainnet_auxpow_activation_height_is_26347() {
        assert_eq!(
            MAINNET_AUXPOW_ACTIVATION_HEIGHT,
            Some(26_347),
            "AuxPoW mainnet activation height must be 26347"
        );
        assert_eq!(
            resolved_auxpow_activation_height(NetworkKind::Mainnet),
            Some(26_347)
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
}
