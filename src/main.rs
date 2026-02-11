mod anchors;
mod block;
mod chain;
mod constants;
mod genesis;
mod pow;
mod tx;

use crate::chain::{block_from_locked, ChainParams, ChainState};
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
    let pow_limit = Target { bits: 0x1d00_ffff }; // same as Python default
    let params = ChainParams {
        genesis_block: block,
        pow_limit,
    };

    let state = ChainState::new(params);

    println!("Irium Rust node initialized:");
    println!("  chain height: {}", state.height);
    println!("  genesis hash: {}", locked.header.hash);
}
