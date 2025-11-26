// Consensus and economic constants for Irium mainnet (Rust mirror of constants.py)

pub const MAX_MONEY: u64 = 100_000_000 * 100_000_000; // 1e8 * 1e8 sat-equivalent
pub const BLOCK_TARGET_INTERVAL: u64 = 600; // seconds
pub const DIFFICULTY_RETARGET_INTERVAL: u64 = 2016; // blocks
pub const MAX_FUTURE_BLOCK_TIME: i64 = 7200; // 2 hours
pub const COINBASE_MATURITY: u64 = 100; // blocks

#[allow(dead_code)]
const INITIAL_SUBSIDY: u64 = 50 * 100_000_000; // 50 IRM in sat-equivalent
#[allow(dead_code)]
const HALVING_INTERVAL: u64 = 210_000; // blocks

#[allow(dead_code)]
pub fn block_reward(height: u64) -> u64 {
    if height == 0 {
        return 0;
    }
    if HALVING_INTERVAL == 0 {
        return INITIAL_SUBSIDY;
    }
    let halvings = (height - 1) / HALVING_INTERVAL;
    if halvings >= 64 {
        return 0;
    }
    INITIAL_SUBSIDY >> halvings
}
