Irium v1.0

- Mining: Rust miner stable; run `./target/release/irium-miner` (or `cargo run --release --bin irium-miner`).
- Multicore: `--threads <n>` controls CPU utilization and nonce search concurrency.
- Env: node/miner now read `IRIUM_*` env vars and `configs/node.json` for overrides.
- Docs: README/QUICKSTART/MINING aligned with Rust binaries and systemd usage.
