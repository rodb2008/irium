# PoAW-X build & test guide

Repo root = `irium-node-rs` crate (lib + bins `iriumd`, `irium-wallet`); the pool is a
separate crate at `pool/irium-stratum`. Toolchain: stable Rust (developed on 1.92). Branch
`testnet/poawx-phase20-blueprint-completion-local` @ `4a3c596`.

## Format

```
cargo fmt -- --check
( cd pool/irium-stratum && cargo fmt -- --check )
```

## Full test suites

```
# Node library (run single-threaded to avoid a pre-existing env-var race in a couple of
# chain tests; green single-threaded):
cargo test --lib -- --test-threads=1            # expect 724 passed

# Node binaries:
cargo test --bin irium-wallet                    # expect 428 passed
cargo test --bin iriumd -- --test-threads=1      # expect 256 passed (slow, ~13 min)

# Pool crate:
( cd pool/irium-stratum && cargo test )          # expect 96 passed
```

## Focused PoAW-X filters

```
cargo test --lib poawx -- --test-threads=1       # 128 passed
cargo test --lib poawx_candidate                 # AssignmentProofV1/V2 + candidate set
cargo test --lib poawx_admission                 # candidate admission (+V2)
cargo test --lib poawx_committed_admission
cargo test --lib poawx_finality
cargo test --lib poawx_puzzle
cargo test --lib poawx_ticket
cargo test --lib poawx_dominance
cargo test --lib poawx_penalty
cargo test --lib phase20 -- --test-threads=1     # 33 passed
cargo test --lib reward  -- --test-threads=1     # 9 passed
cargo test --bin irium-wallet poawx              # 6 passed

( cd pool/irium-stratum && cargo test phase20 )         # 21 passed
( cd pool/irium-stratum && cargo test delegation )      # 42 passed
( cd pool/irium-stratum && cargo test native_rewardable ) # 6 passed
```

## Dependency-tree checks

```
cargo tree | grep -Ei 'openssl|secp256k1-sys|bindgen|native-tls' || echo NONE
cargo tree -i vrf_fun
cargo tree -i secp256kfun
```

## Docs grep checks

```
grep -RIl 'mainnet hard-off' docs/
grep -RIl 'not mainnet-ready' docs/
grep -RIl 'AssignmentProofV2' docs/ src/
```

## Known test caveats

- **Single-threaded lib:** a couple of pre-existing chain tests mutate `IRIUM_NETWORK` and can
  flake under full parallel `cargo test --lib`; they are green with `--test-threads=1`. The
  PoAW-X env-mutating tests use a shared `poawx_test_env_lock()` to serialize.
- **GPU miner target:** `irium-miner-gpu` requires `--features gpu` (not part of the default
  verification suite; out of PoAW-X scope).
- **`ring`:** appears in `cargo tree` as the **pre-existing rustls TLS backend** (via
  rustls/reqwest/axum-server), NOT pulled by the VRF crates. It is not a violation of the
  no-OpenSSL posture.
- **iriumd bin suite is slow** (~13 min single-threaded) — budget for it.
