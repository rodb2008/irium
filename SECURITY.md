# Irium Security Notes

  This document summarizes recommended practices for running Irium nodes,
  wallets, and services in a way that protects users and preserves network
  safety.

  ## Threat Model (High Level)

  Irium assumes a hostile internet: peers may send malformed data, attempt
  resource exhaustion (DoS), or try to isolate nodes (eclipse). The
  protocol defends against these via:

  - Signed bootstrap artifacts (`bootstrap/seedlist.txt`, `bootstrap/anchors.json`).
  - Anchor-based checkpoints enforced by `irium/anchors.py`.
  - Bounded message sizes (`MAX_MESSAGE_SIZE`) in the P2P layer.
  - Peer directory and seedlist maintenance with uptime-based pruning.

  Operators are responsible for:

  - Protecting private keys and wallet files.
  - Securing any HTTP-facing APIs (wallet/explorer).
  - Keeping software and dependencies up to date.

  ## Keys and Trust Roots

  - Consensus trust roots are defined by:
    - `bootstrap/anchors.json` / `irium/bootstrap/anchors.json` (anchor checkpoints).
    - `bootstrap/seedlist.txt` / `irium/bootstrap/seedlist.txt` (bootstrap seeds).
  - These files are signed with maintainer keys listed in
    `bootstrap/trust/allowed_signers` and verified via `ssh-keygen`.
  - The `v1.0` git tag and current main commits are GPG-signed by maintainers.

  To decentralize further over time, additional independent anchor signers
  and seeds should be added to these files and signatures.

  ## Node and P2P Security

  - Nodes perform the following checks on incoming blocks and peers:
    - Validate PoW and header continuity.
    - Enforce anchor checkpoints to reject chains that diverge from signed
      anchors.
    - Enforce a global `MAX_MESSAGE_SIZE` bound on incoming P2P messages to
      limit the impact of oversized message floods.
  - P2P nodes keep a bounded peer set and avoid duplicate connections from
    the same IP:PORT where possible.

  Recommended operational practices:

  - Run public nodes on dedicated VPSes or servers, not on personal
    workstations.
  - Use systemd or similar tooling to restart nodes automatically and
    capture logs for monitoring.
  - Consider basic network-level protections (firewall rules, rate
    limiting) around the P2P port.

  ## Wallet Security

  ### Local Wallet Files

  - Wallets are stored in `~/.irium/irium-wallet.json` (or the path in
    `IRIUM_WALLET_FILE`).
  - This file contains WIF private keys and must be protected with strict
    filesystem permissions (e.g. `chmod 600`).
  - Never commit wallet files or `.irium/` directories to version control.

  ### Wallet API (`scripts/irium-wallet-api-ssl.py`)

  The wallet API is a convenience interface for node-operated wallets. It
  is designed to run on `127.0.0.1` and be fronted by a reverse proxy for
  TLS and authentication.

  Security-related environment variables:

  - `IRIUM_WALLET_ALLOWED_ORIGIN` – restricts CORS to a specific origin
    (e.g. `https://wallet.example.org`). If unset, CORS defaults to `*`.
  - `IRIUM_WALLET_API_TOKEN` – optional bearer token; when set, all API
    calls must include `Authorization: Bearer <token>`.
  - `IRIUM_WALLET_EXPOSE_WIF` – when `true`, `/api/wallet/new-address`
    includes WIF in the response; by default it is `false` and WIF is not
    returned over HTTP.
  - `IRIUM_WALLET_RPM` – per-IP rate limit (requests per minute) enforced
    via the `RateLimiter` helper.

  Recommendations:

  - Run the wallet API only on localhost (`IRIUM_WALLET_HOST=127.0.0.1`) or
    behind a secured reverse proxy that terminates TLS and enforces auth.
  - Do **not** expose the wallet API directly to the public internet.
  - Leave `IRIUM_WALLET_EXPOSE_WIF` unset/false in production; use it only
    in tightly controlled environments.

  ## Explorer and HTTP APIs

  - The explorer API (`scripts/irium-explorer-api.py`) exposes read-only
    blockchain data. It is safe to front with a public reverse proxy as
    long as TLS and rate limiting are configured appropriately.
  - The wallet API is more sensitive; see the guidance above.

  ## Bootstrap and Seeds

  - The root bootstrap seedlist (`bootstrap/seedlist.txt`) and nested
    seedlist (`irium/bootstrap/seedlist.txt`) are signed and should only be
    updated with reviewed, reachable peers. Unreachable seeds are pruned at
    runtime via the peer directory.
  - Operators contributing new seeds should run stable, well-connected
    nodes and coordinate signature updates via the project maintainers.

  ## Updating and Auditing

  - Always run tagged releases (e.g. `v1.0`) and verify signatures on
    tags/anchors/seedlists.
  - Periodically review `docs/whitepaper.md`, `README.md`, and
    `SECURITY.md` for updated guidance.
  - External security audits and additional anchor signers are planned to
    further decentralize trust and harden the network.
