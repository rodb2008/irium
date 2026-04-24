mod activation;
mod anchors;
mod block;
mod chain;
mod constants;
mod genesis;
mod pow;
mod tx;

use crate::activation::{
    network_kind_from_env, resolved_htlcv1_activation_height, resolved_lwma_activation_height,
    resolved_lwma_v2_activation_height,
};
use crate::chain::{block_from_locked, ChainParams, ChainState, LwmaParams};
use crate::genesis::load_locked_genesis;
use crate::pow::Target;

fn main() {
    let locked = match load_locked_genesis() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Failed to load locked genesis: {e}");
            std::process::exit(1);
        }
    };

    let block = match block_from_locked(&locked) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to build genesis block from locked config: {e}");
            std::process::exit(1);
        }
    };
    let pow_limit = Target { bits: 0x1d00_ffff };
    let network = network_kind_from_env();
    let params = ChainParams {
        genesis_block: block,
        pow_limit,
        htlcv1_activation_height: resolved_htlcv1_activation_height(network),
        mpsov1_activation_height: None,
        lwma: LwmaParams::new(resolved_lwma_activation_height(network), pow_limit),
        lwma_v2: resolved_lwma_v2_activation_height(network)
            .map(|h| LwmaParams::new_v2(Some(h), pow_limit)),
    };

    let state = ChainState::new(params);

    println!("Irium Rust node initialized:");
    println!("  chain height: {}", state.height);
    println!("  genesis hash: {}", locked.header.hash);
}
