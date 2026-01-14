# Irium Security Notes

This document summarizes security expectations for running Irium nodes,
wallets, and APIs in production.

## Threat Model (High Level)

Irium assumes a hostile internet: peers may send malformed data, attempt
resource exhaustion (DoS), try to isolate nodes (eclipse), or relay
invalid blocks/transactions. The Rust node defends against these via:

- PoW and header continuity validation for incoming blocks.
- Bounded message sizes in the P2P layer.
- Sybil-resistant handshake proof-of-work.
- Peer reputation, timeouts, and bans for misbehaving peers.
- Signed bootstrap artifacts for initial peer discovery.

Operators are responsible for:

- Protecting wallet private keys and backups.
- Securing any HTTP-facing APIs.
- Keeping software and dependencies up to date.

## Keys and Trust Roots

Bootstrap artifacts are signed and verified via `ssh-keygen` against
`bootstrap/trust/allowed_signers`:

- `bootstrap/seedlist.txt` (initial peers)
- `bootstrap/anchors.json` (checkpoints / trust anchors)

Note: anchor enforcement is planned; current consensus validation relies
on PoW + header continuity. Anchors are still published and should be
kept in sync with the project trust roots.

## Node and P2P Security

Recommendations:

- Run public nodes on dedicated hosts.
- Use systemd to auto-restart services and capture logs.
- Apply basic network protections (firewall rules, rate limits) on the
  P2P port.

## Wallet Security

### Local Wallet Files

- Wallets are stored in `~/.irium/irium-wallet.json` (override with
  `IRIUM_WALLET_FILE`).
- The file contains private keys and must be protected with strict
  permissions (e.g., `chmod 600`).
- Never commit wallet files or `.irium/` to version control.

### Wallet CLI RPC Trust

The wallet CLI uses the node RPC. For HTTPS:

- Preferred: set `IRIUM_RPC_CA=/path/to/ca.crt`.
- `IRIUM_RPC_INSECURE=1` is allowed only for
  `https://localhost` / `https://127.0.0.1` (dev-only).

## HTTP APIs

### Node RPC (iriumd)

- `IRIUM_RPC_TOKEN` can be required for write endpoints
  (e.g., `/rpc/submit_block`, `/rpc/submit_tx`).
- Bind RPC to localhost or place it behind a trusted reverse proxy if
  exposed publicly.

### Explorer API (irium-explorer)

- Read-only API; safe to expose publicly with rate limiting.
- Protect with a bearer token if needed (`IRIUM_EXPLORER_TOKEN`).

### Wallet API (irium-wallet-api)

- Read/write API; should be localhost-only or behind a secured reverse
  proxy.
- Use `IRIUM_WALLET_API_TOKEN` and rate limiting.
- Do not expose wallet APIs publicly without strong access control.

## Services and Secrets

Systemd service env files live under `/etc/irium/`:

- `iriumd.env`
- `miner.env`
- `explorer.env`
- `wallet-api.env`

Keep these files root-readable only (e.g., `chmod 600`) if they contain
secrets such as API tokens.

## Updating and Auditing

- Track `main` or tagged releases and rebuild regularly.
- Verify signed bootstrap artifacts after updates.
- Review `README.md`, `QUICKSTART.md`, and this file for changes.

## Reporting Security Issues

Report security issues privately to: info@iriumlabs.org
Do not open public issues for active vulnerabilities.
