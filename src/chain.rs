#![allow(dead_code)]
use std::collections::{HashMap, HashSet};
use std::env;

use crate::anchors::AnchorManager;
use chrono::Utc;
use hex;
use num_bigint::BigUint;
use num_traits::Zero;

use ripemd::Ripemd160;
use sha2::{Digest, Sha256};

use crate::block::{Block, BlockHeader};
use crate::constants::{
    block_reward, BLOCK_TARGET_INTERVAL, COINBASE_MATURITY, DIFFICULTY_RETARGET_INTERVAL,
    LWMA_MAX_TARGET_DOWN_FACTOR, LWMA_MAX_TARGET_UP_FACTOR, LWMA_MIN_DIFFICULTY_FLOOR,
    LWMA_SOLVETIME_CLAMP_FACTOR, LWMA_WINDOW, LWMA_V2_MAX_TARGET_DOWN_FACTOR,
    LWMA_V2_MAX_TARGET_UP_FACTOR, LWMA_V2_SOLVETIME_CLAMP_FACTOR, LWMA_V2_WINDOW,
    MAX_FUTURE_BLOCK_TIME, MAX_MONEY,
};
use crate::genesis::LockedGenesis;
use crate::pow::{meets_target, min_difficulty_target, sha256d, Target};
use crate::tx::{
    decode_hex, encode_htlcv1_script, parse_htlcv1_script, parse_input_witness,
    parse_output_encumbrance, InputWitness, OutputEncumbrance, Transaction, TxInput, TxOutput,
    HTLC_V1_SCRIPT_TAG,
};

const MAX_ORPHAN_BLOCKS: usize = 100;

fn header_cache_window() -> u64 {
    env::var("IRIUM_HEADER_CACHE_WINDOW")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(10_000)
        .min(200_000)
}

fn block_store_window() -> u64 {
    env::var("IRIUM_BLOCK_STORE_WINDOW")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(5_000)
        .min(200_000)
}

/// Chain parameters for the Irium mainnet.
#[derive(Debug, Clone, Copy)]
pub struct LwmaParams {
    pub activation_height: Option<u64>,
    pub window: u64,
    pub min_solvetime: u64,
    pub max_solvetime: u64,
    pub max_target_up_factor: u64,
    pub max_target_down_factor: u64,
    pub max_target: Target,
}

impl LwmaParams {
    pub fn new(activation_height: Option<u64>, pow_limit: Target) -> Self {
        Self {
            activation_height,
            window: LWMA_WINDOW,
            min_solvetime: 1,
            max_solvetime: BLOCK_TARGET_INTERVAL.saturating_mul(LWMA_SOLVETIME_CLAMP_FACTOR),
            max_target_up_factor: LWMA_MAX_TARGET_UP_FACTOR,
            max_target_down_factor: LWMA_MAX_TARGET_DOWN_FACTOR,
            max_target: min_difficulty_target(pow_limit, LWMA_MIN_DIFFICULTY_FLOOR),
        }
    }

    /// Construct LWMA v2 parameters: smaller window + larger solvetime clamp
    /// for faster post-collapse recovery. Per-block step clamp factors are
    /// unchanged, preserving manipulation resistance.
    pub fn new_v2(activation_height: Option<u64>, pow_limit: Target) -> Self {
        Self {
            activation_height,
            window: LWMA_V2_WINDOW,
            min_solvetime: 1,
            max_solvetime: BLOCK_TARGET_INTERVAL.saturating_mul(LWMA_V2_SOLVETIME_CLAMP_FACTOR),
            max_target_up_factor: LWMA_V2_MAX_TARGET_UP_FACTOR,
            max_target_down_factor: LWMA_V2_MAX_TARGET_DOWN_FACTOR,
            max_target: min_difficulty_target(pow_limit, LWMA_MIN_DIFFICULTY_FLOOR),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChainParams {
    pub genesis_block: Block,
    pub pow_limit: Target,
    pub htlcv1_activation_height: Option<u64>,
    pub lwma: LwmaParams,
    /// Optional LWMA v2 params. When Some and height >= v2.activation_height,
    /// replaces v1. None keeps v1 behavior indefinitely.
    pub lwma_v2: Option<LwmaParams>,
}

/// Reference to a specific transaction output.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct OutPoint {
    pub txid: [u8; 32],
    pub index: u32,
}

/// UTXO with height tracking and coinbase flag.
#[derive(Debug, Clone)]
pub struct UtxoEntry {
    pub output: TxOutput,
    pub height: u64,
    pub is_coinbase: bool,
}

/// In-memory chain state: height, tip, total work, and UTXO set.
#[derive(Debug, Clone, Default)]
struct BlockUndo {
    spent: Vec<(OutPoint, UtxoEntry)>,
    created: Vec<OutPoint>,
    subsidy_created: u64,
}

#[derive(Debug)]
pub struct ChainState {
    pub params: ChainParams,
    pub height: u64,
    pub chain: Vec<Block>,
    pub total_work: BigUint,
    pub utxos: HashMap<OutPoint, UtxoEntry>,
    pub issued: u64,
    /// Pending headers indexed by their hash with cumulative work.
    pub headers: HashMap<[u8; 32], HeaderWork>,
    pub header_chain: Vec<[u8; 32]>,
    /// Orphans waiting on a parent hash.
    pub orphan_pool: HashMap<[u8; 32], Vec<Block>>,
    /// Stored blocks for fork/reorg handling.
    pub block_store: HashMap<[u8; 32], Block>,
    pub heights: HashMap<[u8; 32], u64>,
    pub cumulative_work: HashMap<[u8; 32], BigUint>,
    pub anchors: Option<AnchorManager>,
    pub best_tip: [u8; 32],
    undo_logs: HashMap<[u8; 32], BlockUndo>,
}

#[derive(Debug, Clone)]
pub struct HeaderWork {
    pub header: BlockHeader,
    pub height: u64,
    pub work: BigUint,
}

impl ChainState {
    pub fn new(params: ChainParams) -> Self {
        let mut state = ChainState {
            params,
            height: 0,
            chain: Vec::new(),
            total_work: BigUint::zero(),
            utxos: HashMap::new(),
            issued: 0,
            headers: HashMap::new(),
            header_chain: Vec::new(),
            orphan_pool: HashMap::new(),
            block_store: HashMap::new(),
            heights: HashMap::new(),
            cumulative_work: HashMap::new(),
            anchors: None,
            best_tip: [0u8; 32],
            undo_logs: HashMap::new(),
        };
        let genesis = state.params.genesis_block.clone();
        state
            .connect_genesis(genesis.clone())
            .expect("valid genesis block");
        let genesis_hash = genesis.header.hash();
        let work = ChainState::block_work(&genesis);
        state.block_store.insert(genesis_hash, genesis);
        state.heights.insert(genesis_hash, 0);
        state.cumulative_work.insert(genesis_hash, work.clone());
        state.total_work = work;
        state.best_tip = genesis_hash;
        state
    }

    #[allow(dead_code)]
    pub fn expected_time(&self, height: u64) -> u64 {
        height * BLOCK_TARGET_INTERVAL
    }

    pub fn tip_height(&self) -> u64 {
        self.height.saturating_sub(1)
    }

    fn htlcv1_active_at(&self, height: u64) -> bool {
        self.params
            .htlcv1_activation_height
            .map(|h| height >= h)
            .unwrap_or(false)
    }

    /// Convenience wrapper: compute LWMA target using v1 parameters.
    fn lwma_target_for_height(&self) -> Target {
        self.lwma_target_for_height_with(&self.params.lwma)
    }

    fn lwma_active_at(&self, height: u64) -> bool {
        self.params
            .lwma
            .activation_height
            .map(|h| height >= h)
            .unwrap_or(false)
    }

    fn lwma_trace_enabled() -> bool {
        env::var("IRIUM_TRACE_LWMA")
            .ok()
            .map(|v| {
                matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
    }

    fn orphan_pool_size(&self) -> usize {
        self.orphan_pool.values().map(|v| v.len()).sum()
    }

    fn prune_orphan_pool(&mut self) {
        while self.orphan_pool_size() > MAX_ORPHAN_BLOCKS {
            let key = match self.orphan_pool.keys().next().cloned() {
                Some(k) => k,
                None => break,
            };
            self.orphan_pool.remove(&key);
        }
    }

    pub fn clear_orphan_pool(&mut self) -> usize {
        let count = self.orphan_pool_size();
        self.orphan_pool.clear();
        count
    }

    fn prune_header_cache(&mut self) {
        let window = header_cache_window();
        if window == 0 {
            return;
        }
        let tip = self.tip_height();
        if tip <= window {
            return;
        }
        let min_height = tip.saturating_sub(window);
        self.headers.retain(|_, hw| hw.height >= min_height);
        if !self.header_chain.is_empty() {
            self.header_chain.retain(|h| self.headers.contains_key(h));
        }
    }

    fn prune_block_store(&mut self) {
        let window = block_store_window();
        if window == 0 {
            return;
        }
        let tip = self.tip_height();
        if tip <= window {
            return;
        }
        let min_height = tip.saturating_sub(window);
        let mut remove = Vec::new();
        for (hash, _) in self.block_store.iter() {
            if let Some(h) = self.heights.get(hash) {
                if *h <= min_height {
                    if let Some(block) = self.chain.get(*h as usize) {
                        if block.header.hash() == *hash {
                            remove.push(*hash);
                        }
                    }
                }
            }
        }
        for hash in remove {
            self.block_store.remove(&hash);
        }
    }

    fn prune_caches(&mut self) {
        self.prune_header_cache();
        self.prune_block_store();
    }

    fn block_by_hash(&self, hash: &[u8; 32]) -> Option<Block> {
        if let Some(block) = self.block_store.get(hash) {
            return Some(block.clone());
        }
        let height = *self.heights.get(hash)?;
        self.chain.get(height as usize).cloned()
    }

    fn legacy_target_for_height(&self, height: u64) -> Target {
        if height == 0 {
            return self.params.genesis_block.header.target();
        }
        let last_block = self
            .chain
            .last()
            .expect("chain should have at least genesis when querying target");

        // Pre-activation consensus path. Historical blocks must remain unchanged.
        if height < DIFFICULTY_RETARGET_INTERVAL || height % DIFFICULTY_RETARGET_INTERVAL != 0 {
            return last_block.header.target();
        }

        let interval = DIFFICULTY_RETARGET_INTERVAL as usize;
        if self.chain.len() <= interval {
            return last_block.header.target();
        }

        let prev_index = self.chain.len() - interval;
        let prev_block = &self.chain[prev_index];

        let actual_time = (last_block.header.time as i64) - (prev_block.header.time as i64);
        let mut expected_time = (DIFFICULTY_RETARGET_INTERVAL * BLOCK_TARGET_INTERVAL) as i64;
        if expected_time <= 0 {
            expected_time = 1;
        }

        let mut adj_num = if actual_time <= 0 {
            expected_time / 4
        } else {
            actual_time
        };
        let adj_den = expected_time;

        if adj_num * 4 < adj_den {
            adj_num = adj_den / 4;
        } else if adj_num > adj_den * 4 {
            adj_num = adj_den * 4;
        }

        let last_target = last_block.header.target().to_target();
        let mut new_target = last_target * BigUint::from(adj_num as u64);
        new_target /= BigUint::from(adj_den as u64);

        Target::from_target(&new_target)
    }

    /// Deterministic LWMA next-work calculation used at and after activation.
    ///
    /// Formula in target space:
    /// 1. For the last `N` solved blocks, clamp each solvetime to `[1, 6*T]`.
    /// 2. Compute `weighted_solvetimes = sum_i(i * solvetime_i)` for weights `i = 1..N`.
    /// 3. Compute `avg_target = sum(target_i) / N` over the same window.
    /// 4. Compute `expected = T * sum_i(i)`.
    /// 5. Compute `next_target = avg_target * weighted_solvetimes / expected`.
    /// 6. Clamp `next_target` against the previous target so it cannot tighten
    ///    or ease by more than the configured per-block step factor.
    /// 7. Cap `next_target` by `min(pow_limit, lwma.max_target)`.
    ///
    /// All arithmetic is integer-only and deterministic. Compact bits encoding
    /// is used only at the boundaries.
    fn lwma_target_for_height_with(&self, params: &LwmaParams) -> Target {
        let last_block = self
            .chain
            .last()
            .expect("chain should have at least genesis when querying target");
        let sample_count = std::cmp::min(
            params.window as usize,
            self.chain.len().saturating_sub(1),
        );
        if sample_count == 0 {
            return last_block.header.target();
        }

        let start = self.chain.len() - sample_count;
        let mut weighted_solvetimes = 0u128;
        let mut weight_total = 0u128;
        let mut target_sum = BigUint::zero();

        for (offset, idx) in (start..self.chain.len()).enumerate() {
            let current = &self.chain[idx];
            let previous = &self.chain[idx - 1];
            let raw_solvetime = current
                .header
                .time
                .saturating_sub(previous.header.time)
                .max(params.min_solvetime as u32) as u64;
            let solvetime = raw_solvetime.min(params.max_solvetime);
            let weight = (offset as u128) + 1;
            weighted_solvetimes += weight * u128::from(solvetime);
            weight_total += weight;
            target_sum += current.header.target().to_target();
        }

        let mut avg_target = target_sum / BigUint::from(sample_count as u64);
        if avg_target.is_zero() {
            avg_target = BigUint::from(1u8);
        }

        let observed = BigUint::from(weighted_solvetimes.max(1));
        let expected = BigUint::from((BLOCK_TARGET_INTERVAL as u128) * weight_total);
        let mut next_target = avg_target * observed;
        next_target /= expected;
        if next_target.is_zero() {
            next_target = BigUint::from(1u8);
        }

        let previous_target = last_block.header.target().to_target();
        let mut min_step_target = previous_target.clone();
        min_step_target /= BigUint::from(params.max_target_down_factor.max(1));
        if min_step_target.is_zero() {
            min_step_target = BigUint::from(1u8);
        }
        let max_step_target =
            &previous_target * BigUint::from(params.max_target_up_factor.max(1));

        if next_target < min_step_target {
            next_target = min_step_target;
        }
        if next_target > max_step_target {
            next_target = max_step_target;
        }

        let mut hard_max_target = self.params.pow_limit.to_target();
        let lwma_max_target = params.max_target.to_target();
        if lwma_max_target < hard_max_target {
            hard_max_target = lwma_max_target;
        }
        if next_target > hard_max_target {
            next_target = hard_max_target;
        }

        Target::from_target(&next_target)
    }

    /// Returns true if LWMA v2 is active at the given height.
    fn lwma_v2_active_at(&self, height: u64) -> bool {
        self.params
            .lwma_v2
            .and_then(|v2| v2.activation_height)
            .map(|h| height >= h)
            .unwrap_or(false)
    }

    pub fn target_for_height(&self, height: u64) -> Target {
        let legacy_target = self.legacy_target_for_height(height);
        if !self.lwma_active_at(height) {
            return legacy_target;
        }

        // Use LWMA v2 params if active; otherwise fall back to v1.
        let (lwma_target, version) = if self.lwma_v2_active_at(height) {
            let v2 = self.params.lwma_v2.expect("lwma_v2 must be Some when v2 is active");
            (self.lwma_target_for_height_with(&v2), 2u8)
        } else {
            (self.lwma_target_for_height(), 1u8)
        };

        if Self::lwma_trace_enabled() {
            eprintln!(
                "[trace][lwma] height={} version={} old_bits={:08x} lwma_bits={:08x} selected_bits={:08x}",
                height, version, legacy_target.bits, lwma_target.bits, lwma_target.bits
            );
        }
        lwma_target
    }
    /// Attach an anchor manager for checkpoint enforcement.
    pub fn set_anchors(&mut self, anchors: AnchorManager) {
        self.anchors = Some(anchors);
    }

    /// Work for a block based on its target (Bitcoin-style).
    pub fn block_work(block: &Block) -> BigUint {
        Self::work_for_target(block.header.target())
    }

    fn work_for_target(target: Target) -> BigUint {
        let target = target.to_target();
        if target.is_zero() {
            return BigUint::zero();
        }
        let max = BigUint::from(1u8) << 256;
        max / (target + BigUint::from(1u8))
    }
    #[allow(dead_code)]
    pub fn connect_block(&mut self, block: Block) -> Result<(), String> {
        let expected_height = self.height;
        let previous = self.chain.last();
        self.validate_block_header(&block, expected_height, previous)?;

        let reward = block_reward(expected_height);
        let (_fees, _coinbase_total, subsidy_created, undo) = self
            .validate_and_apply_transactions(
                &block,
                reward,
                expected_height,
                true,
                Some(MAX_MONEY - self.issued),
            )?;

        let new_supply = self
            .issued
            .checked_add(subsidy_created)
            .ok_or_else(|| "Supply overflow".to_string())?;

        let work = ChainState::block_work(&block);
        self.total_work += work;
        let hash = block.header.hash();
        self.chain.push(block.clone());
        self.height += 1;
        self.issued = new_supply;

        self.block_store.insert(hash, block);
        self.heights.insert(hash, expected_height);
        self.cumulative_work.insert(hash, self.total_work.clone());
        self.undo_logs.insert(hash, undo);
        self.best_tip = hash;
        self.prune_caches();

        Ok(())
    }

    fn is_hash_on_main_chain(&self, hash: &[u8; 32]) -> Option<u64> {
        let h = *self.heights.get(hash)?;
        if self
            .chain
            .get(h as usize)
            .map(|b| b.header.hash() == *hash)
            .unwrap_or(false)
        {
            Some(h)
        } else {
            None
        }
    }

    fn disconnect_tip_block(&mut self) -> Result<Block, String> {
        let tip_block = self
            .chain
            .last()
            .cloned()
            .ok_or_else(|| "cannot disconnect empty chain".to_string())?;
        if self.chain.len() <= 1 {
            return Err("cannot disconnect genesis".to_string());
        }
        let tip_hash = tip_block.header.hash();
        let undo = self
            .undo_logs
            .remove(&tip_hash)
            .ok_or_else(|| "missing undo data for tip block".to_string())?;

        for op in undo.created {
            self.utxos.remove(&op);
        }
        for (op, entry) in undo.spent {
            self.utxos.insert(op, entry);
        }

        self.issued = self.issued.saturating_sub(undo.subsidy_created);
        let work = ChainState::block_work(&tip_block);
        if self.total_work >= work {
            self.total_work -= work;
        } else {
            self.total_work = BigUint::zero();
        }

        self.chain.pop();
        self.height = self.chain.len() as u64;
        self.best_tip = self
            .chain
            .last()
            .map(|b| b.header.hash())
            .unwrap_or([0u8; 32]);
        Ok(tip_block)
    }

    fn find_reorg_path(&self, new_tip: [u8; 32]) -> Result<(u64, Vec<Block>), String> {
        let mut cur = new_tip;
        let mut new_branch_rev: Vec<Block> = Vec::new();
        loop {
            if let Some(h) = self.is_hash_on_main_chain(&cur) {
                new_branch_rev.reverse();
                return Ok((h, new_branch_rev));
            }
            let block = self
                .block_by_hash(&cur)
                .ok_or_else(|| "missing block for reorg path".to_string())?;
            let prev = block.header.prev_hash;
            new_branch_rev.push(block);
            if prev == [0u8; 32] {
                return Err("reorg path has no common ancestor".to_string());
            }
            cur = prev;
        }
    }

    fn reorg_to_tip(&mut self, new_tip: [u8; 32]) -> Result<(), String> {
        let (ancestor_height, new_branch) = self.find_reorg_path(new_tip)?;
        let current_tip_height = self.tip_height();
        if ancestor_height >= current_tip_height {
            return Ok(());
        }

        let mut disconnected: Vec<Block> = Vec::new();
        while self.tip_height() > ancestor_height {
            disconnected.push(self.disconnect_tip_block()?);
        }

        let mut connected_new: Vec<Block> = Vec::new();
        for block in &new_branch {
            if let Err(e) = self.connect_block(block.clone()) {
                for _ in 0..connected_new.len() {
                    let _ = self.disconnect_tip_block();
                }
                for old in disconnected.iter().rev() {
                    let _ = self.connect_block(old.clone());
                }
                return Err(format!("reorg connect failed: {}", e));
            }
            connected_new.push(block.clone());
        }
        Ok(())
    }

    /// Try to connect a block at an explicit height and return true if accepted.
    pub fn try_connect_at(&mut self, height: u64, block: Block) -> bool {
        if height != self.height {
            return false;
        }
        self.connect_block(block).is_ok()
    }

    /// Add a header to the header tree if it extends a known header and compute cumulative work.

    pub fn add_header(&mut self, header: BlockHeader) -> Result<u64, String> {
        let hash = header.hash();
        if self.headers.contains_key(&hash) || self.heights.contains_key(&hash) {
            if let Some(h) = self.heights.get(&hash) {
                return Ok(*h);
            }
            return Ok(self.headers.get(&hash).map(|hw| hw.height).unwrap_or(0));
        }

        let prev_hash = header.prev_hash;
        let (parent_height, parent_work) = if let Some(h) = self.headers.get(&prev_hash) {
            (h.height, h.work.clone())
        } else if let Some(h) = self.heights.get(&prev_hash) {
            let work = self
                .cumulative_work
                .get(&prev_hash)
                .cloned()
                .unwrap_or_else(BigUint::zero);
            (*h, work)
        } else {
            return Err("unknown parent".to_string());
        };

        // Basic PoW check.
        if !meets_target(&hash, header.target()) {
            return Err("header does not meet target".to_string());
        }

        let work = parent_work + Self::work_for_target(header.target());
        let height = parent_height + 1;
        self.headers.insert(
            hash,
            HeaderWork {
                header: header.clone(),
                height,
                work: work.clone(),
            },
        );
        // Track header chain for best-work selection.
        self.header_chain.push(hash);
        Ok(height)
    }

    /// Best header hash by total work (main chain tip or best header).
    pub fn best_header_hash(&self) -> [u8; 32] {
        let mut best = (
            self.total_work.clone(),
            self.chain.last().map(|b| b.header.hash()),
        );
        for hw in self.headers.values() {
            if hw.work > best.0 {
                best = (hw.work.clone(), Some(hw.header.hash()));
            }
        }
        best.1.unwrap_or([0u8; 32])
    }

    /// Best-work header entry if it beats the current chain tip.
    pub fn best_header_if_better(&self) -> Option<HeaderWork> {
        let mut best: Option<HeaderWork> = None;
        for hw in self.headers.values() {
            if hw.work > self.total_work {
                if best.as_ref().map(|b| &b.work < &hw.work).unwrap_or(true) {
                    best = Some(hw.clone());
                }
            }
        }
        best
    }

    /// Check if a header connects to current tip.
    pub fn connects_to_tip(&self, header: &BlockHeader) -> bool {
        self.chain
            .last()
            .map(|b| b.header.hash() == header.prev_hash)
            .unwrap_or(false)
    }

    /// Attempt to reorganize to the best-work header by requesting/connecting supplied blocks.
    /// The caller is responsible for providing blocks in order for the target fork.
    pub fn try_reorg(&mut self, new_blocks: &[Block]) -> Result<bool, String> {
        if let Some(_best_header) = self.best_header_if_better() {
            // Simple sanity: the provided blocks must connect from current tip.
            let mut current_hash = self
                .chain
                .last()
                .map(|b| b.header.hash())
                .unwrap_or([0u8; 32]);
            for block in new_blocks {
                if block.header.prev_hash != current_hash {
                    return Err("Reorg block does not connect".to_string());
                }
                self.connect_block(block.clone())?;
                current_hash = block.header.hash();
            }
            // Clear headers since we have advanced main chain.
            self.headers.clear();
            self.header_chain.clear();
            return Ok(true);
        }
        Ok(false)
    }

    fn connect_genesis(&mut self, block: Block) -> Result<(), String> {
        if !self.chain.is_empty() {
            return Err("Genesis block already connected".to_string());
        }
        self.validate_block_header(&block, 0, None)?;
        let (_fees, _coinbase_total, subsidy_created, _undo) =
            self.validate_and_apply_transactions(&block, 0, 0, false, Some(MAX_MONEY))?;

        self.total_work = ChainState::block_work(&block);
        let h = block.header.hash();
        self.chain.push(block);
        self.height = 1;
        self.issued = subsidy_created;
        self.best_tip = h;
        Ok(())
    }

    fn validate_block_header(
        &self,
        block: &Block,
        height: u64,
        previous: Option<&Block>,
    ) -> Result<(), String> {
        if let Some(prev) = previous {
            if block.header.prev_hash != prev.header.hash() {
                return Err("Block does not extend the current tip".to_string());
            }
        } else if block.header.prev_hash != [0u8; 32] {
            return Err("Genesis block must reference null hash".to_string());
        }

        // Timestamp validation
        let current_time = Utc::now().timestamp() as i64;
        if (block.header.time as i64) > current_time + MAX_FUTURE_BLOCK_TIME {
            return Err("Block timestamp too far in future".to_string());
        }
        if let Some(prev) = previous {
            if block.header.time <= prev.header.time {
                return Err("Block timestamp must be greater than previous block".to_string());
            }
        }

        // Merkle root (skip recompute for genesis; trust locked genesis file)
        if height > 0 {
            let recalculated_root = block.merkle_root();
            if block.header.merkle_root != recalculated_root {
                return Err("Block merkle root mismatch".to_string());
            }
        }

        // POW / bits
        let header_hash = block.header.hash();
        let target = self.target_for_height(height);
        if block.header.target().bits != target.bits {
            return Err("Block bits mismatch".to_string());
        }
        if !meets_target(&header_hash, target) {
            return Err("Block does not satisfy proof-of-work target".to_string());
        }

        Ok(())
    }

    fn validate_and_apply_transactions(
        &mut self,
        block: &Block,
        block_reward_value: u64,
        height: u64,
        enforce_reward: bool,
        max_subsidy: Option<u64>,
    ) -> Result<(u64, u64, u64, BlockUndo), String> {
        if block.transactions.is_empty() {
            return Err("Block must include transactions".to_string());
        }
        let coinbase = &block.transactions[0];
        if !is_coinbase(coinbase) {
            return Err("First transaction must be coinbase".to_string());
        }
        if coinbase.outputs.is_empty() {
            return Err("Coinbase transaction must create outputs".to_string());
        }

        let mut created: Vec<(OutPoint, TxOutput, bool)> = Vec::new();
        let mut fees: i64 = 0;
        let mut seen_inputs: HashSet<OutPoint> = HashSet::new();

        for tx in block.transactions.iter().skip(1) {
            self.validate_transaction_internal(tx, height, &mut seen_inputs, &mut fees)?;
            let txid = tx.txid();
            for (index, output) in tx.outputs.iter().cloned().enumerate() {
                let op = OutPoint {
                    txid,
                    index: index as u32,
                };
                created.push((op, output, false));
            }
        }

        let mut coinbase_total: u64 = 0;
        for output in &coinbase.outputs {
            validate_output(output, self.htlcv1_active_at(height))?;
            coinbase_total = coinbase_total
                .checked_add(output.value)
                .ok_or_else(|| "Coinbase outputs overflow".to_string())?;
            if coinbase_total > MAX_MONEY {
                return Err("Coinbase outputs overflow".to_string());
            }
        }
        if enforce_reward && coinbase_total > block_reward_value + (fees as u64) {
            return Err("Coinbase transaction exceeds allowed reward".to_string());
        }

        let coinbase_txid = coinbase.txid();
        for (index, output) in coinbase.outputs.iter().cloned().enumerate() {
            let op = OutPoint {
                txid: coinbase_txid,
                index: index as u32,
            };
            created.push((op, output, true));
        }

        let available_fees = std::cmp::min(fees, coinbase_total as i64);
        let mut subsidy_created = coinbase_total.saturating_sub(available_fees as u64);
        if enforce_reward {
            subsidy_created = std::cmp::min(block_reward_value, subsidy_created);
        }
        if let Some(max_subsidy) = max_subsidy {
            if subsidy_created > max_subsidy {
                return Err("Coinbase subsidy would exceed permitted supply".to_string());
            }
        }

        let mut spent_for_undo: Vec<(OutPoint, UtxoEntry)> = Vec::new();
        for key in &seen_inputs {
            if let Some(entry) = self.utxos.get(key) {
                spent_for_undo.push((key.clone(), entry.clone()));
            }
        }

        for key in &seen_inputs {
            self.utxos.remove(key);
        }
        let mut created_outpoints: Vec<OutPoint> = Vec::new();
        for (op, output, is_coinbase) in created {
            created_outpoints.push(op.clone());
            self.utxos.insert(
                op,
                UtxoEntry {
                    output,
                    height,
                    is_coinbase,
                },
            );
        }

        let undo = BlockUndo {
            spent: spent_for_undo,
            created: created_outpoints,
            subsidy_created,
        };

        Ok((fees as u64, coinbase_total, subsidy_created, undo))
    }

    /// Validate a single transaction against the current UTXO set,
    /// using similar rules as block validation but without mutating state.
    #[allow(dead_code)]
    pub fn validate_transaction(&self, tx: &Transaction) -> Result<(), String> {
        if tx.inputs.is_empty() {
            return Err("Transaction must have at least one input".to_string());
        }
        if tx.outputs.is_empty() {
            return Err("Transaction must have at least one output".to_string());
        }

        let mut seen_inputs: HashSet<OutPoint> = HashSet::new();
        let mut fees: i64 = 0;
        self.validate_transaction_internal(tx, self.height, &mut seen_inputs, &mut fees)
    }

    /// Calculate transaction fees against the current UTXO set without mutating state.
    pub fn calculate_fees(&self, tx: &Transaction) -> Result<u64, String> {
        let mut seen_inputs: HashSet<OutPoint> = HashSet::new();
        let mut fees: i64 = 0;
        self.validate_transaction_internal(tx, self.height, &mut seen_inputs, &mut fees)?;
        Ok(fees as u64)
    }

    /// Hash of the current main chain tip.
    pub fn tip_hash(&self) -> [u8; 32] {
        self.chain
            .last()
            .map(|b| b.header.hash())
            .unwrap_or([0u8; 32])
    }

    /// Path of header hashes from the nearest known block up to the provided header tip.
    pub fn header_path_to_known(&self, tip: [u8; 32]) -> Option<Vec<[u8; 32]>> {
        let mut path = Vec::new();
        let mut current = tip;
        loop {
            if self.heights.contains_key(&current) {
                path.reverse();
                return Some(path);
            }
            let hw = self.headers.get(&current)?;
            path.push(current);
            if hw.header.prev_hash == [0u8; 32] {
                return None;
            }
            current = hw.header.prev_hash;
        }
    }

    fn gather_branch_to_genesis(&self, tip: [u8; 32]) -> Result<Vec<Block>, String> {
        let mut path = Vec::new();
        let mut current = tip;
        loop {
            let block = self
                .block_by_hash(&current)
                .ok_or_else(|| "missing block in store".to_string())?;
            let prev_hash = block.header.prev_hash;
            path.push(block);
            if prev_hash == [0u8; 32] {
                break;
            }
            current = prev_hash;
        }
        path.reverse();
        Ok(path)
    }

    fn rebuild_to_tip(&self, tip_hash: [u8; 32]) -> Result<ChainState, String> {
        let mut new_state = ChainState {
            params: self.params.clone(),
            height: 0,
            chain: Vec::new(),
            total_work: BigUint::zero(),
            utxos: HashMap::new(),
            issued: 0,
            headers: self.headers.clone(),
            header_chain: Vec::new(),
            orphan_pool: self.orphan_pool.clone(),
            block_store: self.block_store.clone(),
            heights: self.heights.clone(),
            cumulative_work: self.cumulative_work.clone(),
            anchors: self.anchors.clone(),
            best_tip: self.best_tip,
            undo_logs: self.undo_logs.clone(),
        };

        let branch = self.gather_branch_to_genesis(tip_hash)?;
        if branch.is_empty() {
            return Err("empty branch".to_string());
        }
        let genesis = &branch[0];
        new_state.connect_genesis(genesis.clone())?;
        let mut cumulative = ChainState::block_work(genesis);
        new_state
            .block_store
            .insert(genesis.header.hash(), genesis.clone());
        new_state.heights.insert(genesis.header.hash(), 0);
        new_state
            .cumulative_work
            .insert(genesis.header.hash(), cumulative.clone());

        for (idx, block) in branch.iter().enumerate().skip(1) {
            if let Err(e) = new_state.connect_block(block.clone()) {
                return Err(format!("failed applying block {}: {}", idx, e));
            }
            cumulative += ChainState::block_work(block);
            let h = block.header.hash();
            new_state.block_store.insert(h, block.clone());
            new_state.heights.insert(h, idx as u64);
            new_state.cumulative_work.insert(h, cumulative.clone());
        }

        Ok(new_state)
    }

    /// Store a block and update best chain incrementally.
    pub fn process_block(&mut self, block: Block) -> Result<(u64, [u8; 32]), String> {
        let hash = block.header.hash();
        if self.heights.contains_key(&hash) {
            return Err("duplicate block".to_string());
        }

        let parent_hash = block.header.prev_hash;
        if parent_hash != [0u8; 32] && !self.heights.contains_key(&parent_hash) {
            self.orphan_pool.entry(parent_hash).or_default().push(block);
            self.prune_orphan_pool();
            return Err("block stored as orphan (prev hash unknown)".to_string());
        }

        if !meets_target(&hash, block.header.target()) {
            return Err("block does not satisfy proof-of-work target".to_string());
        }

        let parent_height = if parent_hash == [0u8; 32] {
            0
        } else {
            *self.heights.get(&parent_hash).unwrap_or(&0)
        };
        let parent_work = if parent_hash == [0u8; 32] {
            BigUint::zero()
        } else {
            self.cumulative_work
                .get(&parent_hash)
                .cloned()
                .unwrap_or_else(BigUint::zero)
        };
        let cumulative = parent_work + ChainState::block_work(&block);
        let height = parent_height + 1;

        if let Some(a) = &self.anchors {
            let hhex = hex::encode(hash);
            if !a.verify_block_against_anchors(height, &hhex) {
                return Err("block violates anchor checkpoint".to_string());
            }
        }

        self.block_store.insert(hash, block.clone());
        self.heights.insert(hash, height);
        self.cumulative_work.insert(hash, cumulative.clone());

        if parent_hash == self.tip_hash() && cumulative > self.total_work {
            self.connect_block(block)?;
        } else if cumulative > self.total_work {
            self.reorg_to_tip(hash)?;
        }

        let mut new_hash = self.tip_hash();
        while let Some(children) = self.orphan_pool.remove(&new_hash) {
            for child in children {
                if let Ok((_h, c_hash)) = self.process_block(child) {
                    new_hash = c_hash;
                }
            }
        }
        self.prune_caches();

        Ok((self.height, self.tip_hash()))
    }

    fn validate_transaction_internal(
        &self,
        tx: &Transaction,
        height: u64,
        seen_inputs: &mut HashSet<OutPoint>,
        fees: &mut i64,
    ) -> Result<(), String> {
        let mut input_total: i64 = 0;
        for (input_index, txin) in tx.inputs.iter().enumerate() {
            if txin.prev_txid.len() != 32 {
                return Err("Transaction input has invalid txid length".to_string());
            }
            let key = OutPoint {
                txid: txin.prev_txid,
                index: txin.prev_index,
            };
            if seen_inputs.contains(&key) {
                return Err("Transaction input double spent within block".to_string());
            }
            let utxo_entry = self
                .utxos
                .get(&key)
                .ok_or_else(|| "Referenced UTXO is missing".to_string())?;

            if utxo_entry.is_coinbase {
                let confirmations = height.saturating_sub(utxo_entry.height);
                if confirmations < COINBASE_MATURITY {
                    return Err("Coinbase UTXO not mature".to_string());
                }
            }

            if !verify_transaction_signature(
                tx,
                input_index,
                txin,
                &utxo_entry.output,
                height,
                self.htlcv1_active_at(height),
            ) {
                return Err("Transaction signature verification failed".to_string());
            }

            seen_inputs.insert(key);
            input_total += utxo_entry.output.value as i64;
        }

        let mut output_total: i64 = 0;
        for output in &tx.outputs {
            validate_output(output, self.htlcv1_active_at(height))?;
            output_total += output.value as i64;
        }
        if input_total < output_total {
            return Err("Transaction spends more than available inputs".to_string());
        }
        *fees += input_total - output_total;
        if *fees < 0 || *fees as u64 > MAX_MONEY {
            return Err("Fee accounting overflow".to_string());
        }

        Ok(())
    }
}

fn is_coinbase(tx: &Transaction) -> bool {
    if tx.inputs.len() != 1 {
        return false;
    }
    let coinbase_input = &tx.inputs[0];
    coinbase_input.prev_txid == [0u8; 32] && coinbase_input.prev_index == 0xffff_ffff
}

fn validate_output(output: &TxOutput, htlcv1_active: bool) -> Result<(), String> {
    if output.value > MAX_MONEY {
        return Err("Output value out of range".to_string());
    }
    if output.script_pubkey.len() > 0xff {
        return Err("script_pubkey too large".to_string());
    }

    if output.script_pubkey.first() == Some(&HTLC_V1_SCRIPT_TAG) {
        if !htlcv1_active {
            return Err("HTLCv1 output before activation".to_string());
        }
        if parse_htlcv1_script(&output.script_pubkey).is_none() {
            return Err("Malformed HTLCv1 output".to_string());
        }
    }

    Ok(())
}

fn hash160(data: &[u8]) -> [u8; 20] {
    let sha = Sha256::digest(data);
    let rip = Ripemd160::digest(sha);
    let mut out = [0u8; 20];
    out.copy_from_slice(&rip);
    out
}

fn signature_digest(tx: &Transaction, input_index: usize, script_pubkey: &[u8]) -> [u8; 32] {
    let mut tx_copy = tx.clone();
    for (idx, input) in tx_copy.inputs.iter_mut().enumerate() {
        if idx == input_index {
            input.script_sig = script_pubkey.to_vec();
        } else {
            input.script_sig.clear();
        }
    }
    let mut data = tx_copy.serialize();
    data.extend_from_slice(&1u32.to_le_bytes());
    sha256d(&data)
}

fn verify_sig_with_pubkey(
    tx: &Transaction,
    input_index: usize,
    script_pubkey: &[u8],
    sig: &[u8],
    pubkey: &[u8],
) -> bool {
    use k256::ecdsa::signature::hazmat::PrehashVerifier;
    use k256::ecdsa::{Signature, VerifyingKey};

    if sig.len() < 2 || sig.last() != Some(&0x01) {
        return false;
    }
    if !(pubkey.len() == 33 || pubkey.len() == 65) {
        return false;
    }
    if input_index >= tx.inputs.len() {
        return false;
    }

    let der = &sig[..sig.len() - 1];
    let signature = match Signature::from_der(der) {
        Ok(s) => s,
        Err(_) => return false,
    };
    if signature.normalize_s().is_some() {
        return false;
    }
    let vk = match VerifyingKey::from_sec1_bytes(pubkey) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let digest = signature_digest(tx, input_index, script_pubkey);
    vk.verify_prehash(&digest, &signature).is_ok()
}

fn verify_transaction_signature(
    tx: &Transaction,
    input_index: usize,
    txin: &TxInput,
    utxo: &TxOutput,
    spend_height: u64,
    htlcv1_active: bool,
) -> bool {
    match parse_output_encumbrance(&utxo.script_pubkey) {
        OutputEncumbrance::P2pkh(expected_pkh) => {
            let witness = parse_input_witness(&txin.script_sig);
            let (sig, pubkey) = match witness {
                InputWitness::P2pkh { sig, pubkey } => (sig, pubkey),
                _ => return false,
            };
            if hash160(&pubkey) != expected_pkh {
                return false;
            }
            verify_sig_with_pubkey(tx, input_index, &utxo.script_pubkey, &sig, &pubkey)
        }
        OutputEncumbrance::HtlcV1(htlc) => {
            if !htlcv1_active {
                return false;
            }
            match parse_input_witness(&txin.script_sig) {
                InputWitness::HtlcClaim {
                    sig,
                    pubkey,
                    preimage,
                } => {
                    if preimage.is_empty() || preimage.len() > 64 {
                        return false;
                    }
                    let pre_hash = Sha256::digest(&preimage);
                    if pre_hash[..] != htlc.expected_hash {
                        return false;
                    }
                    if hash160(&pubkey) != htlc.recipient_pkh {
                        return false;
                    }
                    let script = encode_htlcv1_script(&htlc);
                    verify_sig_with_pubkey(tx, input_index, &script, &sig, &pubkey)
                }
                InputWitness::HtlcRefund { sig, pubkey } => {
                    if spend_height < htlc.timeout_height {
                        return false;
                    }
                    if hash160(&pubkey) != htlc.refund_pkh {
                        return false;
                    }
                    let script = encode_htlcv1_script(&htlc);
                    verify_sig_with_pubkey(tx, input_index, &script, &sig, &pubkey)
                }
                _ => false,
            }
        }
        OutputEncumbrance::Unknown => false,
    }
}

pub fn block_from_locked(gen: &LockedGenesis) -> Result<Block, String> {
    // Decode header fields
    let header = &gen.header;
    let prev_hash = hex::decode(&header.prev_hash)
        .map_err(|e| format!("invalid locked genesis prev_hash hex: {e}"))?;
    let merkle_root = hex::decode(&header.merkle_root)
        .map_err(|e| format!("invalid locked genesis merkle_root hex: {e}"))?;
    if prev_hash.len() != 32 {
        return Err(format!(
            "invalid locked genesis prev_hash length: expected 32 bytes, got {}",
            prev_hash.len()
        ));
    }
    if merkle_root.len() != 32 {
        return Err(format!(
            "invalid locked genesis merkle_root length: expected 32 bytes, got {}",
            merkle_root.len()
        ));
    }
    let mut prev = [0u8; 32];
    prev.copy_from_slice(&prev_hash);
    let mut merkle = [0u8; 32];
    merkle.copy_from_slice(&merkle_root);

    let bits = u32::from_str_radix(header.bits.trim_start_matches("0x"), 16)
        .or_else(|_| u32::from_str_radix(header.bits.as_str(), 16))
        .map_err(|e| format!("invalid locked genesis bits field '{}': {e}", header.bits))?;

    let block_header = BlockHeader {
        version: header.version,
        prev_hash: prev,
        merkle_root: merkle,
        time: header.time as u32,
        bits,
        nonce: header.nonce,
    };

    let mut txs: Vec<Transaction> = Vec::new();
    for tx_hex in &gen.transactions {
        let raw = decode_hex(tx_hex)
            .map_err(|e| format!("invalid locked genesis tx hex '{}': {e}", tx_hex))?;
        let tx = decode_compact_tx(&raw);
        txs.push(tx);
    }

    Ok(Block {
        header: block_header,
        transactions: txs,
    })
}

/// Decode the compact transaction format used in `genesis-locked.json`.
pub fn decode_compact_tx(raw: &[u8]) -> Transaction {
    let mut offset = 0usize;

    let read_u8 = |buf: &[u8], off: &mut usize| -> u8 {
        let v = buf[*off];
        *off += 1;
        v
    };
    let read_u32 = |buf: &[u8], off: &mut usize| -> u32 {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&buf[*off..*off + 4]);
        *off += 4;
        u32::from_le_bytes(bytes)
    };
    let read_u64 = |buf: &[u8], off: &mut usize| -> u64 {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&buf[*off..*off + 8]);
        *off += 8;
        u64::from_le_bytes(bytes)
    };
    let read_bytes = |buf: &[u8], off: &mut usize, len: usize| -> Vec<u8> {
        let out = buf[*off..*off + len].to_vec();
        *off += len;
        out
    };

    let version = read_u32(raw, &mut offset);
    let input_count = read_u8(raw, &mut offset) as usize;
    let mut inputs = Vec::with_capacity(input_count);
    for _ in 0..input_count {
        let prev_len = read_u8(raw, &mut offset) as usize;
        let prev_txid_bytes = read_bytes(raw, &mut offset, prev_len);
        let mut prev_txid = [0u8; 32];
        if prev_txid_bytes.len() == 32 {
            prev_txid.copy_from_slice(&prev_txid_bytes);
        } else {
            let start = 32 - prev_txid_bytes.len();
            prev_txid[start..].copy_from_slice(&prev_txid_bytes);
        }
        let prev_index = read_u32(raw, &mut offset);
        let script_sig_len = read_u8(raw, &mut offset) as usize;
        let script_sig = read_bytes(raw, &mut offset, script_sig_len);
        let sequence = read_u32(raw, &mut offset);
        inputs.push(TxInput {
            prev_txid,
            prev_index,
            script_sig,
            sequence,
        });
    }

    let output_count = read_u8(raw, &mut offset) as usize;
    let mut outputs = Vec::with_capacity(output_count);
    for _ in 0..output_count {
        let value = read_u64(raw, &mut offset);
        let script_len = read_u8(raw, &mut offset) as usize;
        let script_pubkey = read_bytes(raw, &mut offset, script_len);
        outputs.push(TxOutput {
            value,
            script_pubkey,
        });
    }

    let locktime = read_u32(raw, &mut offset);

    Transaction {
        version,
        inputs,
        outputs,
        locktime,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis::load_locked_genesis;
    use crate::pow::Target;
    use crate::tx::{
        encode_htlcv1_claim_witness, encode_htlcv1_refund_witness, encode_htlcv1_script,
        p2pkh_script, HtlcV1Output,
    };
    use k256::ecdsa::signature::hazmat::PrehashSigner;
    use k256::ecdsa::{Signature, SigningKey};

    fn base_chain(activation: Option<u64>) -> ChainState {
        let locked = load_locked_genesis().expect("locked genesis");
        let genesis = block_from_locked(&locked).expect("genesis block");
        let pow_limit = Target { bits: 0x1f00ffff };
        let params = ChainParams {
            genesis_block: genesis,
            pow_limit,
            htlcv1_activation_height: activation,
            lwma: LwmaParams::new(None, pow_limit),
        lwma_v2: None,
        };
        ChainState::new(params)
    }

    fn signing_key(seed: u8) -> SigningKey {
        let mut sk = [0u8; 32];
        sk.fill(seed);
        SigningKey::from_bytes((&sk).into()).expect("signing key")
    }

    fn key_hash(sk: &SigningKey) -> [u8; 20] {
        let pubkey = sk.verifying_key().to_encoded_point(true);
        hash160(pubkey.as_bytes())
    }

    fn p2pkh_witness(
        tx: &Transaction,
        input_index: usize,
        script: &[u8],
        sk: &SigningKey,
    ) -> Vec<u8> {
        let digest = signature_digest(tx, input_index, script);
        let sig: Signature = sk.sign_prehash(&digest).expect("sign");
        let sig = sig.normalize_s().unwrap_or(sig);
        let mut sig_bytes = sig.to_der().as_bytes().to_vec();
        sig_bytes.push(0x01);
        let pubkey = sk
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec();

        let mut out = Vec::with_capacity(1 + sig_bytes.len() + 1 + pubkey.len());
        out.push(sig_bytes.len() as u8);
        out.extend_from_slice(&sig_bytes);
        out.push(pubkey.len() as u8);
        out.extend_from_slice(&pubkey);
        out
    }

    fn add_spendable_p2pkh_utxo(chain: &mut ChainState, sk: &SigningKey, value: u64) -> OutPoint {
        let pkh = key_hash(sk);
        let op = OutPoint {
            txid: [7u8; 32],
            index: 0,
        };
        chain.utxos.insert(
            op.clone(),
            UtxoEntry {
                output: TxOutput {
                    value,
                    script_pubkey: p2pkh_script(&pkh),
                },
                height: chain.tip_height(),
                is_coinbase: false,
            },
        );
        op
    }

    fn difficulty_chain(lwma_activation: Option<u64>, pow_limit_bits: u32) -> ChainState {
        let locked = load_locked_genesis().expect("locked genesis");
        let genesis = block_from_locked(&locked).expect("genesis block");
        let pow_limit = Target {
            bits: pow_limit_bits,
        };
        let params = ChainParams {
            genesis_block: genesis,
            pow_limit,
            htlcv1_activation_height: None,
            lwma: LwmaParams::new(lwma_activation, pow_limit),
        lwma_v2: None,
        };
        ChainState::new(params)
    }

    fn push_synthetic_block(chain: &mut ChainState, time: u32, bits: u32) {
        let prev_hash = chain.chain.last().expect("prev block").header.hash();
        chain.chain.push(Block {
            header: BlockHeader {
                version: 1,
                prev_hash,
                merkle_root: [chain.chain.len() as u8; 32],
                time,
                bits,
                nonce: chain.chain.len() as u32,
            },
            transactions: Vec::new(),
        });
        chain.height = chain.chain.len() as u64;
    }

    fn synthetic_working_bits(chain: &ChainState) -> u32 {
        let target = chain.params.lwma.max_target.to_target() / BigUint::from(2u8);
        Target::from_target(&target).bits
    }

    fn manual_legacy_target(chain: &ChainState, height: u64) -> Target {
        if height == 0 {
            return chain.params.genesis_block.header.target();
        }
        let last_block = chain.chain.last().expect("last block");
        if height < DIFFICULTY_RETARGET_INTERVAL || height % DIFFICULTY_RETARGET_INTERVAL != 0 {
            return last_block.header.target();
        }
        let interval = DIFFICULTY_RETARGET_INTERVAL as usize;
        if chain.chain.len() <= interval {
            return last_block.header.target();
        }
        let prev_block = &chain.chain[chain.chain.len() - interval];
        let actual_time = (last_block.header.time as i64) - (prev_block.header.time as i64);
        let mut expected_time = (DIFFICULTY_RETARGET_INTERVAL * BLOCK_TARGET_INTERVAL) as i64;
        if expected_time <= 0 {
            expected_time = 1;
        }
        let mut adj_num = if actual_time <= 0 {
            expected_time / 4
        } else {
            actual_time
        };
        let adj_den = expected_time;
        if adj_num * 4 < adj_den {
            adj_num = adj_den / 4;
        } else if adj_num > adj_den * 4 {
            adj_num = adj_den * 4;
        }
        let mut new_target = last_block.header.target().to_target() * BigUint::from(adj_num as u64);
        new_target /= BigUint::from(adj_den as u64);
        Target::from_target(&new_target)
    }

    #[test]
    fn htlc_activation_boundary_n_minus_1_n_n_plus_1() {
        let mut chain = base_chain(Some(10));
        let sender = signing_key(1);
        let recipient = signing_key(2);
        let refund = signing_key(3);
        let prev = add_spendable_p2pkh_utxo(&mut chain, &sender, 5_000);

        let mut tx = Transaction {
            version: 1,
            inputs: vec![TxInput {
                prev_txid: prev.txid,
                prev_index: prev.index,
                script_sig: Vec::new(),
                sequence: 0xffff_ffff,
            }],
            outputs: vec![TxOutput {
                value: 4_000,
                script_pubkey: encode_htlcv1_script(&HtlcV1Output {
                    expected_hash: [0x42; 32],
                    recipient_pkh: key_hash(&recipient),
                    refund_pkh: key_hash(&refund),
                    timeout_height: 20,
                }),
            }],
            locktime: 0,
        };
        let utxo_script = chain.utxos.get(&prev).unwrap().output.script_pubkey.clone();
        tx.inputs[0].script_sig = p2pkh_witness(&tx, 0, &utxo_script, &sender);

        chain.height = 9;
        assert!(
            chain.validate_transaction(&tx).is_err(),
            "N-1 must reject HTLC output"
        );

        chain.height = 10;
        assert!(
            chain.validate_transaction(&tx).is_ok(),
            "N must allow HTLC output"
        );

        chain.height = 11;
        assert!(
            chain.validate_transaction(&tx).is_ok(),
            "N+1 must allow HTLC output"
        );
    }

    #[test]
    fn htlc_output_rejected_before_activation() {
        let mut chain = base_chain(Some(100));
        let sender = signing_key(1);
        let recipient = signing_key(2);
        let refund = signing_key(3);
        let prev = add_spendable_p2pkh_utxo(&mut chain, &sender, 5_000);

        let mut tx = Transaction {
            version: 1,
            inputs: vec![TxInput {
                prev_txid: prev.txid,
                prev_index: prev.index,
                script_sig: Vec::new(),
                sequence: 0xffff_ffff,
            }],
            outputs: vec![TxOutput {
                value: 4_000,
                script_pubkey: encode_htlcv1_script(&HtlcV1Output {
                    expected_hash: [11u8; 32],
                    recipient_pkh: key_hash(&recipient),
                    refund_pkh: key_hash(&refund),
                    timeout_height: 10,
                }),
            }],
            locktime: 0,
        };
        let utxo_script = chain.utxos.get(&prev).unwrap().output.script_pubkey.clone();
        tx.inputs[0].script_sig = p2pkh_witness(&tx, 0, &utxo_script, &sender);

        let err = chain
            .validate_transaction(&tx)
            .expect_err("must reject pre-activation");
        assert!(err.contains("HTLCv1 output before activation"));
    }

    #[test]
    fn htlc_output_accepted_after_activation() {
        let mut chain = base_chain(Some(1));
        let sender = signing_key(4);
        let recipient = signing_key(5);
        let refund = signing_key(6);
        let prev = add_spendable_p2pkh_utxo(&mut chain, &sender, 10_000);

        let mut tx = Transaction {
            version: 1,
            inputs: vec![TxInput {
                prev_txid: prev.txid,
                prev_index: prev.index,
                script_sig: Vec::new(),
                sequence: 0xffff_ffff,
            }],
            outputs: vec![TxOutput {
                value: 8_000,
                script_pubkey: encode_htlcv1_script(&HtlcV1Output {
                    expected_hash: [22u8; 32],
                    recipient_pkh: key_hash(&recipient),
                    refund_pkh: key_hash(&refund),
                    timeout_height: 10,
                }),
            }],
            locktime: 0,
        };

        let utxo_script = chain.utxos.get(&prev).unwrap().output.script_pubkey.clone();
        tx.inputs[0].script_sig = p2pkh_witness(&tx, 0, &utxo_script, &sender);
        assert!(chain.validate_transaction(&tx).is_ok());
    }

    fn add_htlc_utxo(
        chain: &mut ChainState,
        value: u64,
        recipient: &SigningKey,
        refund: &SigningKey,
        expected_hash: [u8; 32],
        timeout_height: u64,
    ) -> (OutPoint, HtlcV1Output) {
        let htlc = HtlcV1Output {
            expected_hash,
            recipient_pkh: key_hash(recipient),
            refund_pkh: key_hash(refund),
            timeout_height,
        };
        let op = OutPoint {
            txid: [9u8; 32],
            index: 1,
        };
        chain.utxos.insert(
            op.clone(),
            UtxoEntry {
                output: TxOutput {
                    value,
                    script_pubkey: encode_htlcv1_script(&htlc),
                },
                height: chain.tip_height(),
                is_coinbase: false,
            },
        );
        (op, htlc)
    }

    #[test]
    fn htlc_claim_valid_and_wrong_preimage() {
        let mut chain = base_chain(Some(1));
        chain.height = 50;
        let recipient = signing_key(7);
        let refund = signing_key(8);
        let preimage = b"secret-htlc";
        let mut expected_hash = [0u8; 32];
        expected_hash.copy_from_slice(&Sha256::digest(preimage));

        let (prev, htlc) =
            add_htlc_utxo(&mut chain, 10_000, &recipient, &refund, expected_hash, 60);

        let out_script = p2pkh_script(&key_hash(&recipient));
        let mut tx = Transaction {
            version: 1,
            inputs: vec![TxInput {
                prev_txid: prev.txid,
                prev_index: prev.index,
                script_sig: Vec::new(),
                sequence: 0xffff_fffe,
            }],
            outputs: vec![TxOutput {
                value: 9_000,
                script_pubkey: out_script,
            }],
            locktime: 0,
        };

        let htlc_script = encode_htlcv1_script(&htlc);
        let digest = signature_digest(&tx, 0, &htlc_script);
        let sig: Signature = recipient.sign_prehash(&digest).expect("sign claim");
        let sig = sig.normalize_s().unwrap_or(sig);
        let mut sig_bytes = sig.to_der().as_bytes().to_vec();
        sig_bytes.push(0x01);
        let pubkey = recipient
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec();
        tx.inputs[0].script_sig =
            encode_htlcv1_claim_witness(&sig_bytes, &pubkey, preimage).expect("claim witness");

        assert!(chain.validate_transaction(&tx).is_ok());

        let mut wrong = tx.clone();
        wrong.inputs[0].script_sig = encode_htlcv1_claim_witness(&sig_bytes, &pubkey, b"wrong")
            .expect("claim witness wrong");
        assert!(chain.validate_transaction(&wrong).is_err());
    }

    #[test]
    fn htlc_refund_respects_timeout() {
        let mut chain = base_chain(Some(1));
        let recipient = signing_key(9);
        let refund = signing_key(10);
        let (prev, htlc) = add_htlc_utxo(&mut chain, 10_000, &recipient, &refund, [44u8; 32], 120);

        let out_script = p2pkh_script(&key_hash(&refund));
        let mut tx = Transaction {
            version: 1,
            inputs: vec![TxInput {
                prev_txid: prev.txid,
                prev_index: prev.index,
                script_sig: Vec::new(),
                sequence: 0xffff_fffe,
            }],
            outputs: vec![TxOutput {
                value: 9_000,
                script_pubkey: out_script,
            }],
            locktime: 0,
        };

        let htlc_script = encode_htlcv1_script(&htlc);
        let digest = signature_digest(&tx, 0, &htlc_script);
        let sig: Signature = refund.sign_prehash(&digest).expect("sign refund");
        let sig = sig.normalize_s().unwrap_or(sig);
        let mut sig_bytes = sig.to_der().as_bytes().to_vec();
        sig_bytes.push(0x01);
        let pubkey = refund
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec();
        tx.inputs[0].script_sig =
            encode_htlcv1_refund_witness(&sig_bytes, &pubkey).expect("refund witness");

        chain.height = 119;
        assert!(chain.validate_transaction(&tx).is_err());

        chain.height = 120;
        assert!(chain.validate_transaction(&tx).is_ok());
    }

    #[test]
    fn htlc_malformed_witness_fails() {
        let mut chain = base_chain(Some(1));
        chain.height = 50;
        let recipient = signing_key(11);
        let refund = signing_key(12);
        let (prev, _htlc) = add_htlc_utxo(&mut chain, 10_000, &recipient, &refund, [55u8; 32], 10);

        let mut tx = Transaction {
            version: 1,
            inputs: vec![TxInput {
                prev_txid: prev.txid,
                prev_index: prev.index,
                script_sig: vec![0x01, 0x02],
                sequence: 0xffff_fffe,
            }],
            outputs: vec![TxOutput {
                value: 9_000,
                script_pubkey: p2pkh_script(&key_hash(&recipient)),
            }],
            locktime: 0,
        };

        assert!(chain.validate_transaction(&tx).is_err());

        tx.inputs[0].script_sig = vec![];
        assert!(chain.validate_transaction(&tx).is_err());
    }

    #[test]
    fn htlc_claim_wrong_recipient_pubkey_fails() {
        let mut chain = base_chain(Some(1));
        chain.height = 50;
        let recipient = signing_key(15);
        let wrong = signing_key(16);
        let refund = signing_key(17);
        let mut expected_hash = [0u8; 32];
        expected_hash.copy_from_slice(&Sha256::digest(b"ok-secret"));

        let (prev, htlc) =
            add_htlc_utxo(&mut chain, 10_000, &recipient, &refund, expected_hash, 80);
        let mut tx = Transaction {
            version: 1,
            inputs: vec![TxInput {
                prev_txid: prev.txid,
                prev_index: prev.index,
                script_sig: Vec::new(),
                sequence: 0xffff_fffe,
            }],
            outputs: vec![TxOutput {
                value: 9_000,
                script_pubkey: p2pkh_script(&key_hash(&recipient)),
            }],
            locktime: 0,
        };

        let htlc_script = encode_htlcv1_script(&htlc);
        let digest = signature_digest(&tx, 0, &htlc_script);
        let sig: Signature = wrong.sign_prehash(&digest).expect("sign wrong");
        let sig = sig.normalize_s().unwrap_or(sig);
        let mut sig_bytes = sig.to_der().as_bytes().to_vec();
        sig_bytes.push(0x01);
        let pubkey = wrong
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec();
        tx.inputs[0].script_sig =
            encode_htlcv1_claim_witness(&sig_bytes, &pubkey, b"ok-secret").expect("claim witness");

        assert!(chain.validate_transaction(&tx).is_err());
    }

    #[test]
    fn htlc_refund_wrong_pubkey_fails() {
        let mut chain = base_chain(Some(1));
        chain.height = 500;
        let recipient = signing_key(18);
        let refund = signing_key(19);
        let wrong = signing_key(20);
        let (prev, htlc) = add_htlc_utxo(&mut chain, 10_000, &recipient, &refund, [66u8; 32], 120);

        let mut tx = Transaction {
            version: 1,
            inputs: vec![TxInput {
                prev_txid: prev.txid,
                prev_index: prev.index,
                script_sig: Vec::new(),
                sequence: 0xffff_fffe,
            }],
            outputs: vec![TxOutput {
                value: 9_000,
                script_pubkey: p2pkh_script(&key_hash(&refund)),
            }],
            locktime: 0,
        };

        let htlc_script = encode_htlcv1_script(&htlc);
        let digest = signature_digest(&tx, 0, &htlc_script);
        let sig: Signature = wrong.sign_prehash(&digest).expect("sign wrong refund");
        let sig = sig.normalize_s().unwrap_or(sig);
        let mut sig_bytes = sig.to_der().as_bytes().to_vec();
        sig_bytes.push(0x01);
        let pubkey = wrong
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec();
        tx.inputs[0].script_sig =
            encode_htlcv1_refund_witness(&sig_bytes, &pubkey).expect("refund witness");

        assert!(chain.validate_transaction(&tx).is_err());
    }

    #[test]
    fn pre_activation_uses_legacy_retarget_exactly() {
        let mut chain = difficulty_chain(Some(30_000), 0x207fffff);
        let mut time = chain.chain[0].header.time;
        for _ in 1..DIFFICULTY_RETARGET_INTERVAL {
            time += (BLOCK_TARGET_INTERVAL * 2) as u32;
            push_synthetic_block(&mut chain, time, 0x207fffff);
        }

        let expected = manual_legacy_target(&chain, DIFFICULTY_RETARGET_INTERVAL);
        assert_eq!(
            chain.target_for_height(DIFFICULTY_RETARGET_INTERVAL),
            expected
        );
    }

    #[test]
    fn activation_boundary_switches_to_lwma() {
        let activation = 70;
        let mut chain = difficulty_chain(Some(activation), 0x207fffff);
        let mut time = chain.chain[0].header.time;
        for i in 1..activation {
            time += if i < 60 {
                BLOCK_TARGET_INTERVAL as u32
            } else {
                60
            };
            push_synthetic_block(&mut chain, time, 0x207fffff);
        }

        assert_eq!(
            chain.target_for_height(activation - 1),
            chain.legacy_target_for_height(activation - 1)
        );
        assert_eq!(
            chain.target_for_height(activation),
            chain.lwma_target_for_height()
        );
    }

    #[test]
    fn lwma_is_deterministic_for_same_headers() {
        let activation = 70;
        let mut chain_a = difficulty_chain(Some(activation), 0x207fffff);
        let mut chain_b = difficulty_chain(Some(activation), 0x207fffff);
        let mut time = chain_a.chain[0].header.time;
        for i in 1..activation {
            time += if i % 2 == 0 { 300 } else { 900 };
            push_synthetic_block(&mut chain_a, time, 0x207fffff);
            push_synthetic_block(&mut chain_b, time, 0x207fffff);
        }

        let first = chain_a.target_for_height(activation);
        let second = chain_a.target_for_height(activation);
        let repeated = chain_b.target_for_height(activation);
        assert_eq!(first, second);
        assert_eq!(first, repeated);
    }

    #[test]
    fn lwma_recovers_from_hashrate_increase_with_step_clamp() {
        let activation = 70;
        let mut chain = difficulty_chain(Some(activation), 0x207fffff);
        let test_bits = synthetic_working_bits(&chain);
        let mut time = chain.chain[0].header.time;
        for i in 1..activation {
            time += if i < 40 { 600 } else { 60 };
            push_synthetic_block(&mut chain, time, test_bits);
        }

        let prev_target = chain.chain.last().unwrap().header.target().to_target();
        let next_target = chain.target_for_height(activation).to_target();
        let min_step_target =
            Target::from_target(&(prev_target.clone() / BigUint::from(2u8))).to_target();
        assert!(
            next_target < prev_target,
            "difficulty should rise after faster blocks"
        );
        assert!(
            next_target >= min_step_target,
            "hardening must respect 2x step clamp"
        );
    }

    #[test]
    fn lwma_recovers_from_hashrate_drop_with_step_clamp() {
        let activation = 70;
        let mut chain = difficulty_chain(Some(activation), 0x207fffff);
        let test_bits = synthetic_working_bits(&chain);
        let mut time = chain.chain[0].header.time;
        for i in 1..activation {
            time += if i < 40 { 600 } else { 1800 };
            push_synthetic_block(&mut chain, time, test_bits);
        }

        let prev_target = chain.chain.last().unwrap().header.target().to_target();
        let next_target = chain.target_for_height(activation).to_target();
        let max_step_target =
            Target::from_target(&(prev_target.clone() * BigUint::from(2u8))).to_target();
        assert!(
            next_target > prev_target,
            "difficulty should ease after slower blocks"
        );
        assert!(
            next_target <= max_step_target,
            "easing must respect 2x step clamp"
        );
        assert!(next_target <= chain.params.lwma.max_target.to_target());
    }

    #[test]
    fn lwma_clamps_forward_timestamp_spikes() {
        let activation = 70;
        let mut clamped = difficulty_chain(Some(activation), 0x207fffff);
        let mut time_a = clamped.chain[0].header.time;
        for i in 1..activation {
            time_a += if i == activation - 1 {
                (BLOCK_TARGET_INTERVAL * 6) as u32
            } else {
                BLOCK_TARGET_INTERVAL as u32
            };
            push_synthetic_block(&mut clamped, time_a, 0x207fffff);
        }

        let mut spiked = difficulty_chain(Some(activation), 0x207fffff);
        let mut time_b = spiked.chain[0].header.time;
        for i in 1..activation {
            time_b += if i == activation - 1 {
                200_000
            } else {
                BLOCK_TARGET_INTERVAL as u32
            };
            push_synthetic_block(&mut spiked, time_b, 0x207fffff);
        }

        assert_eq!(
            spiked.target_for_height(activation),
            clamped.target_for_height(activation)
        );
    }

    #[test]
    fn lwma_clamps_non_monotonic_timestamps_to_one_second() {
        let activation = 70;
        let mut monotonic = difficulty_chain(Some(activation), 0x207fffff);
        let mut time_a = monotonic.chain[0].header.time;
        for i in 1..activation {
            time_a += if i == activation - 1 {
                1
            } else {
                BLOCK_TARGET_INTERVAL as u32
            };
            push_synthetic_block(&mut monotonic, time_a, 0x207fffff);
        }

        let mut non_monotonic = difficulty_chain(Some(activation), 0x207fffff);
        let mut time_b = non_monotonic.chain[0].header.time;
        for i in 1..activation {
            if i == activation - 1 {
                time_b = time_b.saturating_sub(500);
            } else {
                time_b += BLOCK_TARGET_INTERVAL as u32;
            }
            push_synthetic_block(&mut non_monotonic, time_b, 0x207fffff);
        }

        assert_eq!(
            non_monotonic.target_for_height(activation),
            monotonic.target_for_height(activation)
        );
    }

    #[test]
    fn lwma_respects_post_activation_max_target_floor() {
        let activation = 70;
        let mut chain = difficulty_chain(Some(activation), 0x207fffff);
        let mut time = chain.chain[0].header.time;
        for _ in 1..activation {
            time += 3600;
            push_synthetic_block(&mut chain, time, 0x207fffff);
        }

        let next = chain.target_for_height(activation).to_target();
        assert_eq!(next, chain.params.lwma.max_target.to_target());
        assert!(next <= chain.params.pow_limit.to_target());
    }

    #[test]
    fn activation_future_does_not_rewrite_historical_targets() {
        let mut future = difficulty_chain(Some(30_000), 0x207fffff);
        let mut disabled = difficulty_chain(None, 0x207fffff);
        let mut time = future.chain[0].header.time;
        for _ in 1..DIFFICULTY_RETARGET_INTERVAL {
            time += 1200;
            push_synthetic_block(&mut future, time, 0x207fffff);
            push_synthetic_block(&mut disabled, time, 0x207fffff);
        }

        assert_eq!(
            future.target_for_height(DIFFICULTY_RETARGET_INTERVAL),
            disabled.target_for_height(DIFFICULTY_RETARGET_INTERVAL)
        );
    }

    #[test]
    fn reorg_across_activation_boundary_recomputes_safely() {
        let activation = 70;
        let mut chain = difficulty_chain(Some(activation), 0x207fffff);
        let mut time = chain.chain[0].header.time;
        for i in 1..activation {
            time += if i < 60 { 600 } else { 120 };
            push_synthetic_block(&mut chain, time, 0x207fffff);
        }
        let target_at_activation = chain.target_for_height(activation);

        time += 120;
        push_synthetic_block(&mut chain, time, target_at_activation.bits);
        let _post_activation_target = chain.target_for_height(activation + 1);

        chain.chain.pop();
        chain.height = chain.chain.len() as u64;

        assert_eq!(chain.target_for_height(activation), target_at_activation);
        assert_eq!(
            chain.target_for_height(activation - 1),
            chain.legacy_target_for_height(activation - 1)
        );
    }

    #[test]
    fn legacy_p2pkh_unchanged() {
        let mut chain = base_chain(None);
        let sender = signing_key(13);
        let recipient = signing_key(14);
        let prev = add_spendable_p2pkh_utxo(&mut chain, &sender, 20_000);

        let mut tx = Transaction {
            version: 1,
            inputs: vec![TxInput {
                prev_txid: prev.txid,
                prev_index: prev.index,
                script_sig: Vec::new(),
                sequence: 0xffff_ffff,
            }],
            outputs: vec![TxOutput {
                value: 18_000,
                script_pubkey: p2pkh_script(&key_hash(&recipient)),
            }],
            locktime: 0,
        };

        let utxo_script = chain.utxos.get(&prev).unwrap().output.script_pubkey.clone();
        tx.inputs[0].script_sig = p2pkh_witness(&tx, 0, &utxo_script, &sender);

        assert!(chain.validate_transaction(&tx).is_ok());
    }

    // -----------------------------------------------------------------------
    // LWMA v2 tests (N=30, clamp=10T) -- gated behind lwma_v2 activation
    // -----------------------------------------------------------------------

    fn difficulty_chain_v2(lwma_v1_activation: Option<u64>, v2_activation: Option<u64>, pow_limit_bits: u32) -> ChainState {
        let locked = crate::genesis::load_locked_genesis().expect("locked genesis");
        let genesis = block_from_locked(&locked).expect("genesis block");
        let pow_limit = Target { bits: pow_limit_bits };
        let v2 = v2_activation.map(|h| LwmaParams::new_v2(Some(h), pow_limit));
        let params = ChainParams {
            genesis_block: genesis,
            pow_limit,
            htlcv1_activation_height: None,
            lwma: LwmaParams::new(lwma_v1_activation, pow_limit),
            lwma_v2: v2,
        };
        ChainState::new(params)
    }

    #[test]
    fn lwma_v2_inactive_when_field_is_none() {
        // With lwma_v2: None the chain must behave identically to v1.
        let activation = 70u64;
        let mut v1_chain = difficulty_chain(Some(activation), 0x207fffff);
        let mut v2_chain = difficulty_chain_v2(Some(activation), None, 0x207fffff);
        let bits = synthetic_working_bits(&v1_chain);
        let mut time = v1_chain.chain[0].header.time;
        for _ in 1..=80 {
            time += 600;
            push_synthetic_block(&mut v1_chain, time, bits);
            push_synthetic_block(&mut v2_chain, time, bits);
        }
        let t1 = v1_chain.target_for_height(80);
        let t2 = v2_chain.target_for_height(80);
        assert_eq!(t1.bits, t2.bits, "v2=None must produce same target as pure v1");
    }

    #[test]
    fn lwma_v2_activates_at_boundary() {
        let v1_act = 10u64;
        let v2_act = 50u64;
        let mut chain = difficulty_chain_v2(Some(v1_act), Some(v2_act), 0x207fffff);
        let bits = synthetic_working_bits(&chain);
        let mut time = chain.chain[0].header.time;
        for _ in 1..=80 {
            time += 600;
            push_synthetic_block(&mut chain, time, bits);
        }
        let below = chain.target_for_height(v2_act - 1);
        let at    = chain.target_for_height(v2_act);
        let above = chain.target_for_height(v2_act + 5);
        let pow_limit = chain.params.pow_limit.to_target();
        assert!(below.to_target() <= pow_limit);
        assert!(at.to_target()    <= pow_limit);
        assert!(above.to_target() <= pow_limit);
        assert_ne!(at.bits, 0, "v2 target must be non-zero at activation");
        assert_ne!(above.bits, 0, "v2 target must be non-zero above activation");
    }

    #[test]
    fn lwma_v2_step_clamp_unchanged() {
        let v1_act = 10u64;
        let v2_act = 20u64;
        let mut chain = difficulty_chain_v2(Some(v1_act), Some(v2_act), 0x207fffff);
        let bits = synthetic_working_bits(&chain);
        let mut time = chain.chain[0].header.time;
        for _ in 1..=20 {
            time += 600;
            push_synthetic_block(&mut chain, time, bits);
        }
        // Slow blocks at v2 max solvetime (10T = 6000s).
        let mut prev_target = chain.target_for_height(20);
        for h in 21u64..=50 {
            time += 6000;
            push_synthetic_block(&mut chain, time, bits);
            let next_target = chain.target_for_height(h);
            let max_allowed = Target::from_target(
                &(prev_target.to_target() * BigUint::from(2u8))
            );
            assert!(
                next_target.to_target() <= max_allowed.to_target(),
                "v2 step clamp violated at height {}: {:?} > 2x {:?}", h, next_target, prev_target
            );
            prev_target = next_target;
        }
    }

    #[test]
    fn lwma_v2_recovers_faster_than_v1_after_hashrate_drop() {
        // Use a hard initial bits (well below max_target) so there is ample room to
        // ease without saturating.  After exactly 35 moderate-slow blocks (900s =
        // 1.5x T, step-clamp NOT binding), v2's 30-block window is fully refreshed
        // while v1's 60-block window is still half diluted by old fast blocks.
        // Therefore v2 must produce a strictly higher (easier) target.
        let v1_act = 10u64;
        let v2_act = 10u64;
        // Hard bits: far from the 0x207fffff max_target so no saturation.
        let hard_bits: u32 = 0x1a007fff;
        let slow_st: u32 = 900; // 1.5x T; ratio < 2x so step clamp never fires

        let mut v1 = difficulty_chain(Some(v1_act), 0x207fffff);
        let mut v2 = difficulty_chain_v2(Some(v1_act), Some(v2_act), 0x207fffff);

        let mut time_v1 = v1.chain[0].header.time;
        let mut time_v2 = v2.chain[0].header.time;

        // 70 normal blocks to fill both windows with fast-block history.
        for _ in 1..=70 {
            time_v1 += 600;
            push_synthetic_block(&mut v1, time_v1, hard_bits);
            time_v2 += 600;
            push_synthetic_block(&mut v2, time_v2, hard_bits);
        }

        // 35 slow blocks.  After 30 slow blocks v2 window is fully refreshed;
        // v1 still carries 25 old fast blocks in its 60-block window.
        for _ in 0..35 {
            time_v1 += slow_st;
            push_synthetic_block(&mut v1, time_v1, hard_bits);
            time_v2 += slow_st;
            push_synthetic_block(&mut v2, time_v2, hard_bits);
        }

        let h = v1.height;
        let t_v1 = v1.target_for_height(h).to_target();
        let t_v2 = v2.target_for_height(h).to_target();

        // v2: weighted_avg_st=900s -> ratio 1.5x; v1: ~847s -> ratio ~1.41x
        // Both < 2x so step clamp does not fire. v2 must be strictly easier.
        assert!(
            t_v2 > t_v1,
            "v2 (N=30) should ease faster: v2_target={} v1_target={}",
            t_v2, t_v1
        );
    }

    #[test]
    fn lwma_v2_steady_state_stable() {
        let v1_act = 10u64;
        let v2_act = 20u64;
        let mut chain = difficulty_chain_v2(Some(v1_act), Some(v2_act), 0x207fffff);
        let bits = synthetic_working_bits(&chain);
        let mut time = chain.chain[0].header.time;
        for _ in 1..=20 {
            time += 600;
            push_synthetic_block(&mut chain, time, bits);
        }
        let base = chain.target_for_height(v2_act).to_target();
        for h in (v2_act + 1)..=(v2_act + 100) {
            time += 600;
            push_synthetic_block(&mut chain, time, bits);
            let t = chain.target_for_height(h).to_target();
            let lo = &base / BigUint::from(4u8);
            let hi = &base * BigUint::from(4u8);
            assert!(
                t >= lo && t <= hi,
                "v2 target drifted out of 4x band at height {}: {} vs base {}", h, t, base
            );
        }
    }

    // -----------------------------------------------------------------------
    // Activation boundary simulation: heights 19723-19726
    // -----------------------------------------------------------------------

    #[test]
    fn lwma_v2_boundary_no_off_by_one() {
        // Build a chain with v1 active from height 16462 and v2 from 19725.
        // Populate synthetic blocks up to height 19726 and verify:
        //   heights < 19725  => v1 params (N=60, clamp=6T)
        //   heights >= 19725 => v2 params (N=30, clamp=10T)
        // Also verifies: no panic, deterministic target, no off-by-one.
        let v1_act: u64 = 16_462;
        let v2_act: u64 = 19_725;

        let mut chain = difficulty_chain_v2(Some(v1_act), Some(v2_act), 0x207fffff);
        let bits = synthetic_working_bits(&chain);
        let mut time = chain.chain[0].header.time;

        // Push 19726 blocks at 600s each to go past the activation boundary.
        for _ in 1..=19_726u64 {
            time += 600;
            push_synthetic_block(&mut chain, time, bits);
        }

        // Collect targets at the four boundary heights.
        let t_19723 = chain.target_for_height(19_723);
        let t_19724 = chain.target_for_height(19_724);
        let t_19725 = chain.target_for_height(19_725);
        let t_19726 = chain.target_for_height(19_726);

        // All must be non-zero and within pow_limit.
        let pow_limit = chain.params.pow_limit.to_target();
        for (h, t) in [(19_723u64, &t_19723), (19_724, &t_19724),
                       (19_725, &t_19725), (19_726, &t_19726)] {
            assert_ne!(t.bits, 0, "target at height {} must be non-zero", h);
            assert!(t.to_target() <= pow_limit,
                "target at height {} must not exceed pow_limit", h);
        }

        // Below activation: lwma_v2_active_at must be false.
        assert!(!chain.lwma_v2_active_at(19_724),
            "lwma_v2 must NOT be active at height 19724 (one below activation)");

        // At and above activation: lwma_v2_active_at must be true.
        assert!(chain.lwma_v2_active_at(19_725),
            "lwma_v2 must be active at height 19725 (activation height)");
        assert!(chain.lwma_v2_active_at(19_726),
            "lwma_v2 must be active at height 19726 (above activation)");

        // Under steady-state 600s intervals the target should be stable across the
        // boundary -- no sudden jump.  Allow a 4x band around the pre-activation
        // target to account for legitimate parameter differences.
        let base = t_19724.to_target();
        let lo = &base / BigUint::from(4u8);
        let hi = &base * BigUint::from(4u8);
        assert!(
            t_19725.to_target() >= lo && t_19725.to_target() <= hi,
            "target at activation (19725) must not jump more than 4x from prior block:              19724={} 19725={}", t_19724.bits, t_19725.bits
        );
        assert!(
            t_19726.to_target() >= lo && t_19726.to_target() <= hi,
            "target at 19726 must not jump more than 4x from prior block:              19724={} 19726={}", t_19724.bits, t_19726.bits
        );
    }

}
