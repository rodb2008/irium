# HTLCv1 Activation Source (Authoritative)

## Main Principle
Consensus activation for **mainnet** is code-defined, deterministic, and independent of operator runtime env.

## Activation Sources by Network
- **Mainnet**
  - Source: `src/activation.rs` constant `MAINNET_HTLCV1_ACTIVATION_HEIGHT`.
  - Runtime env `IRIUM_HTLCV1_ACTIVATION_HEIGHT` is ignored.
- **Testnet/Devnet**
  - Source: runtime env `IRIUM_HTLCV1_ACTIVATION_HEIGHT` (for testing only).

## Code Paths
- Source selection:
  - `src/activation.rs`
    - `MAINNET_HTLCV1_ACTIVATION_HEIGHT`
    - `network_kind_from_env()`
    - `resolved_htlcv1_activation_height(...)`
- Applied into `ChainParams::htlcv1_activation_height` by:
  - `src/bin/iriumd.rs`
  - `src/bin/irium-miner.rs`
  - `src/bin/irium-p2p.rs`
  - `src/main.rs`
- Consensus/mempool/template/block validation consume `ChainParams` in `src/chain.rs`.

## Current Mainnet State
- `MAINNET_HTLCV1_ACTIVATION_HEIGHT = None`.
- Effect: HTLCv1 is disabled on mainnet.

## Future Activation Procedure (Code-Defined)
1. choose activation height via governance
2. set `MAINNET_HTLCV1_ACTIVATION_HEIGHT = Some(<height>)`
3. release software
4. operators/miners upgrade
5. activation occurs automatically on-chain at height

No per-node env var is required for mainnet activation.
