# HTLCv1 Review Notes

Date: 2026-03-09

## Scope Reviewed
- Activation plumbing and boundary behavior (`N-1`, `N`, `N+1`)
- Miner/template inclusion at activation boundary
- Mempool vs consensus consistency
- Signature/hash checks for claim/refund
- Legacy P2PKH path regression risk with activation unset

## Key Findings

1. Activation plumbing issue (fixed)
- Issue: miner path used `htlcv1_activation_height: None` regardless of env.
- Impact: HTLC txs accepted by RPC/mempool but skipped in mining template path.
- Fix: miner now reads `IRIUM_HTLCV1_ACTIVATION_HEIGHT` and uses it in `ChainParams`.

2. Miner/template boundary issue (fixed)
- Added explicit coverage proving HTLC template inclusion at activation height.
- Activation interpretation aligned on candidate block height semantics.

3. Consensus/mempool consistency
- Before activation: HTLC funding/spend rejected.
- At/after activation: HTLC funding/spend validated according to rules.
- Mempool acceptance and block validation behavior align in covered scenarios.

4. Legacy safety with activation unset
- Default remains disabled when `IRIUM_HTLCV1_ACTIVATION_HEIGHT` is unset.
- Legacy P2PKH behavior remains normal in tested paths.

## Remaining Known Limitations
1. Full reorg lifecycle for HTLC spends not yet exhaustively tested.
2. Mempool persistence/reload matrix coverage is partial.
3. No coordinator/routing/app-layer orchestration (out of scope for HTLCv1 primitive).

## Recommendation
- Safe to park code on `main` with HTLCv1 activation OFF by default.
- Do not activate on mainnet yet.
- Continue testnet/devnet soak plus reorg/persistence expansion before any activation proposal.
