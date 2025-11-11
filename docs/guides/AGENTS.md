# Repository Guidelines

## Project Structure & Module Organization
The repo centers on `irium/` (consensus, wallet, networking) with operational wrappers living in `scripts/`. Long-lived network data such as anchors, bootstrap peers, and genesis headers live under `bootstrap/`, `config/`, and `configs/`. Documentation for operators and researchers is kept in the Markdown guides at the top level plus `docs/`. Runtime artifacts (`state/`, `~/.irium/**`) must stay out of git.

## Build, Test, and Development Commands
- `python3 -m venv .venv && source .venv/bin/activate && pip install -r requirements.txt` — provision a clean environment.
- `PYTHONPATH=$PWD python3 scripts/irium-node.py 38291` — run a node on the chosen P2P port.
- `PYTHONPATH=$PWD python3 scripts/irium-miner.py 38292` — launch the reference miner aimed at the node.
- `PYTHONPATH=$PWD python3 scripts/verify_genesis.py` — sanity-check the locked genesis metadata.
- `PYTHONPATH=$PWD python3 -m pytest tests` — execute the Python test suite (unit + smoke tests).

## Coding Style & Naming Conventions
Adhere to PEP 8 with 4-space indents, snake_case functions, PascalCase classes, and meaningful module names. Keep consensus-critical constants in config files rather than hardcoded literals. Prefer dataclasses and type hints for anything serialized over the network or persisted on disk. Log via the existing structured logger utilities instead of ad-hoc `print` statements.

## Testing Guidelines
Every new consensus or networking change needs a matching test under `tests/`, mirroring the module path (`tests/test_chain.py` for `irium/chain.py`, etc.). Use realistic fixtures (e.g., `tests/fixtures/genesis.json`) and cover both valid and invalid flows. Run `pytest -k <component>` for focused checks and capture relevant log excerpts in PR descriptions.

## Commit & Pull Request Guidelines
Use short, imperative messages, optionally emoji-prefixed (see git history). Describe why the change matters, what was touched, and how it was validated. Pull requests should link to the corresponding tracker entry, summarize behavioural changes, list testing commands, and mention any follow-up tasks. Include screenshots or telemetry snippets only when the docs or monitoring outputs change.

## Security & Configuration Tips
Never commit private keys, WIFs, or node credentials. Rely on environment variables like `IRIUM_WALLET_FILE`, `IRIUM_RPC_BIND`, and `IRIUM_GENESIS_PATH` or per-node JSON files under `configs/`. When exposing APIs publicly, place them behind TLS and rate limiting (see `scripts/irium-wallet-api-ssl.py`). Rotate anchors and bootstrap signatures whenever consensus parameters shift, and document the new fingerprints in `docs/security.md`.
