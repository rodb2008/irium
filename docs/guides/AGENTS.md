# Repository Guidelines

## Project Structure & Module Organization
The repo centers on Rust sources in `src/` (consensus, wallet, networking) with binaries defined under `src/bin`. Long-lived network data such as anchors, bootstrap peers, and genesis headers live under `bootstrap/` and `configs/`. Systemd templates live in `systemd/`, and optional shell helpers live in `scripts/` (no Python entrypoints). Documentation for operators and researchers is kept in the Markdown guides at the top level plus `docs/`. Runtime artifacts (`state/`, `~/.irium/**`) must stay out of git.

## Build, Test, and Development Commands
- `source ~/.cargo/env` — load Rust toolchain (if needed).
- `cargo build --release` — build node/miner/wallet.
- `./target/release/iriumd` — run the node (HTTP + P2P).
- `./target/release/irium-miner` — run the miner (use `IRIUM_MINER_ADDRESS`).
- `./target/release/irium-wallet` — wallet CLI (new-address, balance).
- `./target/release/irium-spv` — SPV proof tooling.
- `cargo test --quiet` — run Rust tests.

## Coding Style & Naming Conventions
Follow idiomatic Rust style (`rustfmt`, `clippy`) and keep modules cohesive (`chain.rs`, `pow.rs`, `wallet.rs`). Keep consensus-critical constants in `configs/` or `bootstrap/` rather than hardcoded literals. Log via the structured logger utilities instead of ad-hoc `println!` statements for network paths.

## Testing Guidelines
Add unit tests inline with modules and integration tests under `tests/` where needed. Cover consensus-critical flows (block validation, PoW, wallet, networking) and include fixtures for sample blocks under `tests/fixtures/` when relevant.

## Commit & Pull Request Guidelines
Use short, imperative messages, optionally emoji-prefixed (see git history). Describe why the change matters, what was touched, and how it was validated. Pull requests should link to the corresponding tracker entry, summarize behavioural changes, list testing commands, and mention any follow-up tasks. Include screenshots or telemetry snippets only when the docs or monitoring outputs change.

## Security & Configuration Tips
Never commit private keys, WIFs, or node credentials. Rely on environment variables like `IRIUM_RPC_TOKEN`, `IRIUM_TLS_CERT`, `IRIUM_TLS_KEY`, `IRIUM_RPC_CA`, and JSON configs under `configs/`. When exposing APIs publicly, place them behind TLS and rate limiting (see `src/rate_limiter.rs`). Rotate anchors and bootstrap signatures whenever consensus parameters shift, and document the new fingerprints in `docs/security.md`.
