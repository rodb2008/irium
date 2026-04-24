#![allow(dead_code)]
// Consensus and economic constants for Irium mainnet (Rust mirror of constants.py)

pub const MAX_MONEY: u64 = 100_000_000 * 100_000_000; // 1e8 * 1e8 sat-equivalent
pub const BLOCK_TARGET_INTERVAL: u64 = 600; // seconds
pub const DIFFICULTY_RETARGET_INTERVAL: u64 = 2016; // blocks
pub const MAX_FUTURE_BLOCK_TIME: i64 = 7200; // 2 hours
pub const COINBASE_MATURITY: u64 = 100; // blocks
pub const LWMA_WINDOW: u64 = 60; // blocks
pub const LWMA_SOLVETIME_CLAMP_FACTOR: u64 = 6; // clamp to [1, 6T]
pub const LWMA_MAX_TARGET_UP_FACTOR: u64 = 2; // target may ease by at most 2x per block
pub const LWMA_MAX_TARGET_DOWN_FACTOR: u64 = 2; // target may tighten by at most 2x per block
pub const LWMA_MIN_DIFFICULTY_FLOOR: u64 = 1; // 1 disables any stricter post-activation max-target cap

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

// LWMA v2 parameters (inactive until MAINNET_LWMA_V2_ACTIVATION_HEIGHT is set).
//
// Motivation: real-world observation (blocks 19639-19704) showed that after a
// dominant miner left the network, the 60-block window dilutes slow-block signal
// so heavily that difficulty takes ~7.5 days to reach usable levels for infra
// miners alone. Smaller window and larger solvetime clamp both increase the
// signal each slow block contributes without weakening the per-block step clamp.
//
// Simulation (infra at 16.7 MH/s, difficulty 1.02e12, T=600s):
//   v1 (N=60, clamp=6T): usable after 7.1d, near-target after 7.5d
//   v2 (N=30, clamp=10T): usable after 2.6d, near-target after 2.7d
//
// Max single-block ease is unchanged (2x step clamp), preserving manipulation
// resistance. Upward step clamp (hardening) is also unchanged.
pub const LWMA_V2_WINDOW: u64 = 30; // reduced from 60 for faster response
pub const LWMA_V2_SOLVETIME_CLAMP_FACTOR: u64 = 10; // increased from 6 for stronger slow-block signal
pub const LWMA_V2_MAX_TARGET_UP_FACTOR: u64 = 2; // unchanged: max 2x ease per block
pub const LWMA_V2_MAX_TARGET_DOWN_FACTOR: u64 = 2; // unchanged: max 2x harden per block
