# LWMA Difficulty Upgrade Rollout

This change introduces a height-gated consensus upgrade for difficulty adjustment.

## Activation

- Mainnet activation constant: `MAINNET_LWMA_ACTIVATION_HEIGHT`
- Current value: `16462`
- Coordinated activation basis: observed mainnet height `15962` + safety buffer `500` = `16462`
- Pre-activation blocks continue using the legacy 2016-block retarget unchanged
- Post-activation blocks use the deterministic per-block LWMA algorithm

## Exact Formula

Let:
- `T = BLOCK_TARGET_INTERVAL`
- `N = LWMA_WINDOW`
- `solvetime_i = clamp(time_i - time_{i-1}, 1, 6*T)`
- `weight_i = i` for `i = 1..N`
- `target_i = full target decoded from compact bits`

Then the implementation computes:
- `weighted_solvetimes = sum_i(weight_i * solvetime_i)`
- `avg_target = sum_i(target_i) / N`
- `expected = T * sum_i(weight_i)`
- `next_target = avg_target * weighted_solvetimes / expected`

Then it applies deterministic bounds:
- lower bound: `previous_target / LWMA_MAX_TARGET_DOWN_FACTOR`
- upper bound: `previous_target * LWMA_MAX_TARGET_UP_FACTOR`
- final cap: `min(pow_limit, lwma.max_target)`

All arithmetic is integer-only. No floating point and no randomized jitter are used.

## Floor / Cap Choice

- Current mainnet default: `LWMA_MIN_DIFFICULTY_FLOOR = 1`
- That means post-activation `lwma.max_target == pow_limit`
- So the upgrade currently adds no stricter extra minimum-difficulty floor on mainnet

Tradeoff:
- keeping the extra cap disabled is safer for initial rollout because it cannot make
  blocks harder than the existing consensus maximum target on a tiny network
- enabling a stricter cap later may improve resistance to pathological over-easing,
  but it should only be done after replay data and live observation justify it

## Operator Notes

- This is a consensus change
- All mainnet nodes, miners, and pools must upgrade before the activation height
- Do not activate immediately by editing the constant to a past or near-tip height without coordination
- Current scheduled mainnet activation height is exactly `16462`
- Testnet/devnet may use `IRIUM_LWMA_ACTIVATION_HEIGHT` for rehearsal
- `IRIUM_TRACE_LWMA=1` enables diagnostic logging comparing legacy target, LWMA target, and selected target

## Safety Notes

- Historical validation is unchanged before activation
- No block hash, PoW hash, serialization, timestamp rule, subsidy, or chain-selection logic is changed
- Difficulty remains fully deterministic; there is no randomized jitter
