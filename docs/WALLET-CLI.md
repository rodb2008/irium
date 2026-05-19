# irium-wallet CLI Reference

`irium-wallet` is the command-line wallet for Irium. It manages keys, queries the chain, creates and funds agreements, and handles the full OTC marketplace lifecycle.

All chain query and broadcast commands accept `--rpc <url>` to specify a custom node. The default is `http://127.0.0.1:38300`.

> Address prefixes: single-signature P2PKH addresses start with `Q` (base58 version byte `0x39`). Multisig addresses (2-of-2, 2-of-3) start with `P` (version byte `0x28`). Any field documented as `<addr>` accepts either form unless explicitly noted.

---

## Wallet Initialisation and Key Management

### `irium-wallet init [--seed <64hex>]`

Initialises a new wallet. If `--seed` is provided, initialises from the given 64-character hex seed. Otherwise generates a new random seed.

```
irium-wallet init
irium-wallet init --seed a3f1...64hexchars...
```

---

### `irium-wallet new-address`

Derives the next address from the wallet seed and stores it.

```
irium-wallet new-address
```

Output: the new address in Q-prefix format.

---

### `irium-wallet list-addresses`

Lists all addresses stored in the wallet.

```
irium-wallet list-addresses
```

---

### `irium-wallet export-wif <addr> --out <file>`

Exports the private key for `<addr>` in WIF (Wallet Import Format) to a file.

```
irium-wallet export-wif Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa --out privkey.wif
```

Keep the output file secure. Anyone with this file can spend funds from that address.

---

### `irium-wallet import-wif <wif>`

Imports an address from a WIF-encoded private key.

```
irium-wallet import-wif 5HueCGU8rMjxECyDigwEXBH...
```

---

### `irium-wallet export-seed --out <file>`

Exports the wallet seed (32 bytes, hex-encoded) to a file.

```
irium-wallet export-seed --out seed.txt
```

The seed is the master secret. Anyone with the seed can derive all wallet addresses.

---

### `irium-wallet import-seed <64hex> [--force]`

Imports a seed, replacing the current wallet seed. Use `--force` to overwrite without confirmation.

```
irium-wallet import-seed a3f1...64hexchars...
irium-wallet import-seed a3f1...64hexchars... --force
```

---

### `irium-wallet backup [--out <file>]`

Creates a wallet backup. If `--out` is omitted, writes to a default location.

```
irium-wallet backup --out wallet-backup.json
```

---

### `irium-wallet restore-backup <file> [--force]`

Restores wallet from a backup file.

```
irium-wallet restore-backup wallet-backup.json
irium-wallet restore-backup wallet-backup.json --force
```

---

### `irium-wallet address-to-pkh <addr>`

Converts an Irium address to its public key hash (hex). Useful for constructing scripts manually.

```
irium-wallet address-to-pkh Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa
```

Output: `79dbb6fd908884fc994b8aa34dcef392fe2d9d65`

---

## Chain Queries

All commands accept `--rpc <url>` to override the default node URL (`http://127.0.0.1:38300`).

### `irium-wallet balance <addr> [--rpc <url>]`

Returns the balance for an address.

```
irium-wallet balance Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa
irium-wallet balance Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa --rpc http://192.0.2.1:38300
```

---

### `irium-wallet list-unspent <addr> [--rpc <url>]`

Returns all UTXOs for an address.

```
irium-wallet list-unspent Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa
```

---

### `irium-wallet history <addr> [--rpc <url>]`

Returns the transaction history for an address.

```
irium-wallet history Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa
```

---

### `irium-wallet estimate-fee [--rpc <url>]`

Returns the current minimum fee per byte from the node.

```
irium-wallet estimate-fee
```

---

## Sending

### `irium-wallet send <from_addr> <to_addr> <amount_irm> [options]`

Builds and broadcasts a transaction.

**Options:**

| Flag | Description |
|------|-------------|
| `--fee <irm>` | Fee in IRM (default: auto from fee estimate) |
| `--coin-select smallest\|largest` | UTXO selection strategy |
| `--rpc <url>` | Node URL |

**Examples:**
```
irium-wallet send Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa QDestinationAddr... 1.5

irium-wallet send Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa QDestinationAddr... 1.5 \
  --fee 0.0001 \
  --coin-select smallest
```

Amount is in IRM (decimal). 1.5 IRM = 150,000,000 satoshis.

---

## Offer Lifecycle (OTC Marketplace)

### `irium-wallet offer-create`

Creates a new OTC sell offer.

**Flags:**

| Flag | Required | Description |
|------|----------|-------------|
| `--seller <addr>` | Yes | Seller's Irium address |
| `--amount <irm>` | Yes | Amount in IRM |
| `--payment-method <text>` | Yes | Payment method (e.g. `bank-transfer`, `cash`) |
| `--timeout <height>` | Yes | Block height at which the offer expires |
| `--price-note <text>` | No | Human-readable price note (e.g. current rate) |
| `--payment-instructions <text>` | No | Instructions for the buyer |
| `--offer-id <id>` | No | Custom offer ID (auto-generated if omitted) |

```
irium-wallet offer-create \
  --seller Q9KxBRfrnb6v9Vb8vuHjwkZaxj3ZRhJWpg \
  --amount 1.0 \
  --payment-method bank-transfer \
  --timeout 25000 \
  --price-note "1 IRM = 0.10 USD at time of listing" \
  --payment-instructions "IBAN: DE89 ..."
```

---

### `irium-wallet offer-list [options]`

Lists offers known to the wallet (the union of locally created offers, locally
imported offers, and offers synced from remote feeds — see `feed-list` and
`offer-feed-sync`). Each row includes the seller's reputation summary so you
can scan trust signals at a glance.

**Flags:**

| Flag | Description |
|------|-------------|
| `--status open\|taken\|settled` | Filter by lifecycle status |
| `--source local\|imported\|remote\|all` | Filter by where the offer originated (default: `all`) |
| `--seller <pubkey\|addr>` | Show only offers from this seller |
| `--payment <method>` | Substring match against `payment_method` (e.g. `bank`, `revolut`, `usdt`) |
| `--min-amount <irm>` | Lower amount bound (decimal IRM) |
| `--max-amount <irm>` | Upper amount bound (decimal IRM) |
| `--sort score\|newest\|amount\|seller` | Sort order. `score` ranks by the seller's reputation `ranking_score`; `newest` by `created_at` desc; `amount` by IRM ascending; `seller` alphabetically by seller address |
| `--limit <n>` | Maximum number of rows to print |
| `--summary` | Compact one-line-per-offer output (handy when piping into `grep`, `awk`, or the desktop app's offer feed) |
| `--json` | Emit the full machine-readable feed structure to stdout |

```
# Default ranking by trust score — best counterparties first
irium-wallet offer-list --sort score --limit 20

# Only bank-transfer offers between 1 and 50 IRM
irium-wallet offer-list --payment bank --min-amount 1 --max-amount 50

# Everything you imported manually, terse output
irium-wallet offer-list --source imported --summary

# Just one seller, full JSON
irium-wallet offer-list --seller Q9KxBRfrnb6v9Vb8vuHjwkZaxj3ZRhJWpg --json
```

Each row also prints the seller's reputation summary — see
[Reputation](#reputation) below for the full field reference. Quick reading:

- `agreements: N` — total completed trades on this seller's address
- `default_count: N` — count of agreements that ended in `timeout` or `unsatisfied`
- `risk_signal: low|moderate|high|very_high` — categorical risk derived from the default ratio
- `ranking_score` (only used internally by `--sort score`) — composite score combining completed-agreement count, recency, and inverse default rate

---

### `irium-wallet offer-show --offer <offer_id>`

Shows full details of a single offer.

```
irium-wallet offer-show --offer d1-gossip-t4
```

---

### `irium-wallet offer-take --offer <offer_id> --buyer <addr> [--rpc <url>]`

Takes an open offer as buyer. This initiates the agreement creation process.

```
irium-wallet offer-take --offer d1-gossip-t4 --buyer QBuyerAddress...
```

---

### `irium-wallet offer-export --offer <offer_id> --out <file>`

Exports an offer to a JSON file for sharing with a counterparty.

```
irium-wallet offer-export --offer d1-gossip-t4 --out offer-d1-gossip-t4.json
```

---

### `irium-wallet offer-import --file <file>`

Imports an offer from a JSON file.

```
irium-wallet offer-import --file offer-d1-gossip-t4.json
```

---

### `irium-wallet offer-fetch --url <url>`

Fetches a single offer from a URL.

```
irium-wallet offer-fetch --url https://example.com/offers/my-offer.json
```

---

### `irium-wallet offer-feed-fetch --url <feed-endpoint>`

Fetches all offers from a feed endpoint URL.

```
irium-wallet offer-feed-fetch --url http://node.example.com:38300/offers/feed
```

---

### `irium-wallet offer-feed-sync [--json]`

Syncs offers from all configured feed URLs.

```
irium-wallet offer-feed-sync
irium-wallet offer-feed-sync --json
```

---

### `irium-wallet offer-feed-export [--out <file>] [--limit <n>]`

Exports the locally cached offer feed as a single JSON document — the same shape served by a node's `/offers/feed` endpoint. Useful for republishing your own offer set on a separate static host, or for taking an offline snapshot before pruning.

| Flag | Description |
|------|-------------|
| `--out <file>` | Output path (default: stdout) |
| `--limit <n>` | Cap the number of offers exported |

```
irium-wallet offer-feed-export --out my-feed.json
irium-wallet offer-feed-export --limit 100 --out top100.json
```

---

### `irium-wallet offer-feed-prune [--older-than-days <n>] [--dry-run] [--json]`

Removes expired or stale offers from the local feed cache. By default prunes anything past its `--timeout` height. Pass `--older-than-days` to also prune offers older than the given calendar age, regardless of timeout.

| Flag | Description |
|------|-------------|
| `--older-than-days <n>` | Also prune offers older than `n` days |
| `--dry-run` | Report what would be pruned without modifying state |
| `--json` | Emit machine-readable output |

```
irium-wallet offer-feed-prune --dry-run
irium-wallet offer-feed-prune --older-than-days 30
```

---

## Feed Management

The feed registry is stored in plain JSON at `~/.irium/feeds.json` (or
`%USERPROFILE%\.irium\feeds.json` on Windows). It contains the list of remote
offer-feed endpoints synced by `offer-feed-sync`. You can edit it by hand, but
the commands below cover the common cases.

### `irium-wallet feed-add <url>`

Adds a feed URL to the list of feeds synced by `offer-feed-sync`.

```
irium-wallet feed-add http://node.example.com:38300/offers/feed
```

---

### `irium-wallet feed-remove <url>`

Removes a feed URL.

```
irium-wallet feed-remove http://node.example.com:38300/offers/feed
```

---

### `irium-wallet feed-list`

Lists all configured feed URLs.

```
irium-wallet feed-list
```

---

### `irium-wallet feed-bootstrap`

Adds default bootstrap feed URLs from the built-in seed list.

```
irium-wallet feed-bootstrap
```

---

## Reputation

Reputation is derived locally on every node from observed agreement outcomes —
there is no central reputation server. Every full node arrives at the same
numbers because the inputs are deterministic on-chain data.

### `irium-wallet reputation-show <seller_pubkey|address> [--json]`

Shows the reputation record for a seller, including outcome history.

```
irium-wallet reputation-show Q9KxBRfrnb6v9Vb8vuHjwkZaxj3ZRhJWpg
irium-wallet reputation-show 03e918af472e63de044c983df9f09bae57d4c78a70998d5d5fded408672886f868
irium-wallet reputation-show Q9KxBRfrnb6v9Vb8vuHjwkZaxj3ZRhJWpg --json
```

**JSON output (key fields):**

| Field | Type | Meaning |
|-------|------|---------|
| `total_agreements` | integer | Total completed agreements involving this seller |
| `satisfied_count` | integer | Agreements that completed with a satisfying release |
| `default_count` | integer | Agreements that ended in `timeout` or `unsatisfied` outcome |
| `disputed_count` | integer | Agreements with a recorded dispute |
| `risk_signal` | string | One of `low`, `moderate`, `high`, `very_high` — derived from the default ratio |
| `ranking_score` | float | Composite score blending completion count, recency, and inverse defaults — used by `offer-list --sort score` |
| `recent_default_count` | integer | Defaults within the recent block window (see WHITEPAPER §10 for thresholds) |
| `last_outcome_height` | integer | Block height at which the last outcome was recorded |
| `outcomes[]` | array | Per-agreement outcome history (newest first) |

In `offer-list` rows, the same record drives the inline `reputation:` summary —
which is why the same fields appear without prefix in that listing.

---

### `irium-wallet reputation-record-outcome`

Records a trade outcome for a seller. Used after an agreement concludes.

**Flags:**

| Flag | Required | Description |
|------|----------|-------------|
| `--seller <addr>` | Yes | Seller's address |
| `--outcome <type>` | Yes | One of: `satisfied`, `failed`, `disputed`, `timeout` |

```
irium-wallet reputation-record-outcome \
  --seller Q9KxBRfrnb6v9Vb8vuHjwkZaxj3ZRhJWpg \
  --outcome satisfied
```

---

## Mining (companion binaries)

The wallet binary does not mine; mining is handled by the dedicated
`irium-miner` (CPU) and `irium-miner-gpu` (GPU + Stratum pool) binaries
installed alongside `irium-wallet`. All three share the same wallet store, so
rewards mined to one of your wallet addresses appear directly in
`irium-wallet balance`.

### Solo CPU mining — `irium-miner`

Mines directly against a local `iriumd` via the `/rpc/getblocktemplate`
endpoint. Configuration is via environment variables; there are no CLI flags
beyond `--version` / `--help`.

| Env var | Description |
|---------|-------------|
| `IRIUM_MINER_ADDRESS` | Coinbase payout address (base58, `Q…` or `P…`). Required. Alternatively set `IRIUM_MINER_PKH` to a 40-hex public key hash. |
| `IRIUM_NODE_RPC` | iriumd RPC URL (default `http://127.0.0.1:38300`) |
| `IRIUM_MINER_THREADS` | Worker thread count (default: all cores) |
| `IRIUM_ADVERTISE_ADDR` | Optional `ip:port`. When set, the miner embeds the address in every coinbase as a peer-discovery hint. |

```bash
IRIUM_MINER_ADDRESS=Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa \
  irium-miner
```

The miner reads `/etc/irium/miner.env` (if present) on startup so packaged
deployments can keep secrets out of the shell history.

---

### Solo & pool GPU mining — `irium-miner-gpu`

OpenCL-based SHA-256d miner. Supports both solo (RPC) and pool (Stratum v1)
modes from the same binary. Auto-detects discrete NVIDIA / AMD GPUs in
preference to integrated Intel iGPUs.

**Flags:**

| Flag | Description |
|------|-------------|
| `--wallet <addr>` | Mining/payout address (same as `IRIUM_MINER_ADDRESS`) |
| `--pool <url>` | Stratum URL (e.g. `stratum+tcp://pool.iriumlabs.org:3335`). When set, the miner runs in pool mode and ignores `--rpc`. |
| `--rpc <url>` | Solo-mode node RPC URL (default `http://127.0.0.1:38300`) |
| `--platform <n\|name>` | OpenCL platform index, or vendor substring (`nvidia`, `amd`, `intel`). Default: auto, prefers NVIDIA / AMD. |
| `--device <n>` | Device index within the selected platform (default `0`) |
| `--devices <n,n,…>` | Comma-separated multi-GPU list (overrides `--device`) |
| `--batch <n>` | Nonces per GPU dispatch (default `4194304` = 2²²) |
| `--list-platforms` | Print every detected OpenCL platform and device, then exit |
| `--help` | Show usage and exit |

CLI flags take priority over the equivalent environment variables
(`IRIUM_STRATUM_URL`, `IRIUM_MINER_ADDRESS`, `IRIUM_GPU_PLATFORM`,
`IRIUM_GPU_DEVICE`, `IRIUM_GPU_DEVICES`, `IRIUM_GPU_BATCH`, `IRIUM_NODE_RPC`).

**Pool mining against the official pool:**

```bash
# CPU/GPU profile
irium-miner-gpu \
  --pool stratum+tcp://pool.iriumlabs.org:3335 \
  --wallet Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa

# ASIC profile (use any SHA-256 ASIC's pool-config UI instead — the URL is what matters)
# stratum+tcp://pool.iriumlabs.org:3333  worker = your Q-address
```

**Solo GPU mining:**

```bash
irium-miner-gpu \
  --wallet Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa \
  --rpc http://127.0.0.1:38300
```

**Multi-GPU on a single host:**

```bash
irium-miner-gpu --wallet Q… --platform nvidia --devices 0,1,2,3
```

Public pool stats (active miners, blocks found, rolling-window hashrate
estimate per profile) are served as JSON from
`http://pool.iriumlabs.org:3337/stats` and surfaced in the `irium-core`
desktop app under Explorer → Pool Stats.

---

## Agreement Creation

### `irium-wallet agreement-create-simple-settlement`

Creates a simple two-party settlement agreement.

**Required flags:**

| Flag | Description |
|------|-------------|
| `--agreement-id <id>` | Unique agreement ID |
| `--creation-time <unix>` | Unix timestamp |
| `--party-a <id\|name\|addr\|role>` | First party identifier |
| `--party-b <id\|name\|addr\|role>` | Second party identifier |
| `--amount <irm>` | Amount in IRM |
| `--secret-hash <32bytehex>` | Hash of the unlock secret |
| `--refund-timeout <height>` | Block height for refund eligibility |
| `--document-hash <32bytehex>` | SHA256 of the agreement document |
| `--out <file>` | Output file path (optional) |

```
irium-wallet agreement-create-simple-settlement \
  --agreement-id settle-001 \
  --creation-time 1777624133 \
  --party-a addr=QPartyAAddress... \
  --party-b addr=QPartyBAddress... \
  --amount 1.0 \
  --secret-hash abcdef01234567890abcdef01234567890abcdef01234567890abcdef01234567 \
  --refund-timeout 20500 \
  --document-hash fedcba98765432100fedcba98765432100fedcba98765432100fedcba98765432 \
  --out settle-001.json
```

---

### `irium-wallet agreement-create-otc`

Creates an OTC trade agreement (buyer and seller, asset reference, payment reference).

**Required flags:** (same structure as `agreement-create-simple-settlement` plus):

| Flag | Description |
|------|-------------|
| `--buyer <...>` | Buyer identifier |
| `--seller <...>` | Seller identifier |
| `--asset-reference <text>` | Asset being traded (e.g. `50 USDT`) |
| `--payment-reference <text>` | Payment reference (e.g. bank transfer ref) |

```
irium-wallet agreement-create-otc \
  --agreement-id otc-002 \
  --creation-time 1777624133 \
  --buyer addr=QBuyerAddress... \
  --seller addr=QSellerAddress... \
  --amount 1.0 \
  --asset-reference "50 USDT" \
  --payment-reference "SEPA transfer ref #12345" \
  --secret-hash abcdef01234567890abcdef01234567890abcdef01234567890abcdef01234567 \
  --refund-timeout 20500 \
  --document-hash fedcba98765432100fedcba98765432100fedcba98765432100fedcba98765432 \
  --out otc-002.json
```

---

### `irium-wallet agreement-create-deposit`

Creates a deposit agreement. Same flag structure as `agreement-create-otc` but for payer/payee deposit flows with a purpose reference and refund summary.

---

### `irium-wallet agreement-create-milestone`

Creates a milestone-based agreement. Milestones each have their own amount and timeout height, allowing partial release at multiple checkpoints.

---

## Agreement Operations

All commands accept `--rpc <url>`.

### `irium-wallet agreement-fund <ref> [--broadcast] [--rpc <url>]`

Builds the funding transaction for an agreement. Pass `--broadcast` to submit it immediately.

`<ref>` can be a path to an `agreement.json`, a `bundle.json`, an agreement ID, or an agreement hash.

```
irium-wallet agreement-fund otc-002.json --broadcast --rpc http://localhost:38300
```

---

### `irium-wallet agreement-status <ref> [--rpc <url>]`

Returns the current on-chain status.

```
irium-wallet agreement-status otc-002.json --rpc http://localhost:38300
```

---

### `irium-wallet agreement-timeline <ref> [--rpc <url>]`

Returns the full event timeline.

```
irium-wallet agreement-timeline otc-002.json
```

---

### `irium-wallet agreement-release <ref> [--secret <hex>] [--broadcast] [--rpc <url>]`

Builds (and optionally broadcasts) the release transaction. Requires the unlock secret.

```
irium-wallet agreement-release otc-002.json \
  --secret abcdef01234567890abcdef01234567890abcdef01234567890abcdef01234567 \
  --broadcast
```

---

### `irium-wallet agreement-refund <ref> [--broadcast] [--rpc <url>]`

Builds (and optionally broadcasts) the refund transaction. Only valid after the refund timeout height.

```
irium-wallet agreement-refund otc-002.json --broadcast
```

---

### `irium-wallet agreement-release-eligibility <ref> [--rpc <url>]`

Checks whether the agreement is currently eligible for release.

```
irium-wallet agreement-release-eligibility otc-002.json
```

---

### `irium-wallet agreement-refund-eligibility <ref> [--rpc <url>]`

Checks whether the agreement is currently eligible for refund.

```
irium-wallet agreement-refund-eligibility otc-002.json
```

---

### `irium-wallet agreement-milestones <ref> [--rpc <url>]`

Returns milestone status for milestone-type agreements.

```
irium-wallet agreement-milestones milestone-003.json
```

---

### `irium-wallet agreement-hash <ref>`

Computes and prints the deterministic hash of an agreement.

```
irium-wallet agreement-hash otc-002.json
```

---

### `irium-wallet agreement-inspect <ref>`

Prints the parsed fields of an agreement for verification.

```
irium-wallet agreement-inspect otc-002.json
```

---

### `irium-wallet agreement-list`

Lists all agreements stored locally in the wallet.

```
irium-wallet agreement-list
```

---

### `irium-wallet agreement-save <ref> [--label <label>]`

Saves an agreement to local wallet storage.

```
irium-wallet agreement-save otc-002.json --label "USDT trade Nov 2026"
```

---

### `irium-wallet agreement-audit <ref> [--rpc <url>]`

Returns a full audit record including on-chain events, proofs, and policy evaluations.

```
irium-wallet agreement-audit otc-002.json
```

---

## Proof Operations

### `irium-wallet agreement-proof-create`

Creates a signed proof attesting that a condition has been met.

**Flags:**

| Flag | Required | Description |
|------|----------|-------------|
| `--agreement-hash <hex>` | Yes | Agreement hash |
| `--proof-type <type>` | Yes | Type of proof (e.g. `delivery_confirmed`) |
| `--attested-by <id>` | Yes | Attester identifier |
| `--address <addr>` | Yes | Attester's Irium address |
| `--evidence-summary <text>` | No | Human-readable description of evidence |
| `--evidence-hash <hex>` | No | SHA256 hash of evidence file |
| `--out <file>` | No | Output file path |

```
irium-wallet agreement-proof-create \
  --agreement-hash abcdef01...32bytehex... \
  --proof-type delivery_confirmed \
  --attested-by attestor-id \
  --address QAttestorAddress... \
  --evidence-summary "Goods delivered, tracking #ABC123" \
  --out proof.json
```

---

### `irium-wallet agreement-proof-submit --proof <proof.json|-> [--rpc <url>]`

Submits a proof to the node. Use `-` to read from stdin.

```
irium-wallet agreement-proof-submit --proof proof.json --rpc http://localhost:38300
```

---

### `irium-wallet agreement-proof-list [--agreement-hash <hex>] [--rpc <url>]`

Lists proofs. If `--agreement-hash` is provided, filters to that agreement.

```
irium-wallet agreement-proof-list --agreement-hash abcdef01...
```

---

### `irium-wallet agreement-proof-get --proof-id <id> [--rpc <url>]`

Returns a single proof by ID.

```
irium-wallet agreement-proof-get --proof-id proof-abc123
```

---

## Policy Operations

### `irium-wallet policy-build-otc`

Builds a release policy for an OTC agreement.

**Flags:**

| Flag | Description |
|------|-------------|
| `--policy-id <id>` | Policy identifier |
| `--agreement-hash <hash>` | Agreement hash |
| `--attestor <id>:<pubkey_or_addr>` | Required attestor |
| `--release-proof-type <type>` | Required proof type to trigger release |

```
irium-wallet policy-build-otc \
  --policy-id policy-001 \
  --agreement-hash abcdef01...32bytehex... \
  --attestor attestor-id:QAttestorAddress... \
  --release-proof-type delivery_confirmed
```

---

### `irium-wallet agreement-policy-set --policy <policy.json> [--rpc <url>]`

Stores a policy on the node.

```
irium-wallet agreement-policy-set --policy policy.json
```

---

### `irium-wallet agreement-policy-get --agreement-hash <hex> [--rpc <url>]`

Returns the stored policy for an agreement.

```
irium-wallet agreement-policy-get --agreement-hash abcdef01...
```

---

### `irium-wallet agreement-policy-evaluate --agreement <hash|id> [--rpc <url>]`

Evaluates the policy against currently submitted proofs.

```
irium-wallet agreement-policy-evaluate --agreement abcdef01...
```

---

### `irium-wallet agreement-policy-list [--active-only] [--rpc <url>]`

Lists stored policies.

```
irium-wallet agreement-policy-list --active-only
```

---

## Signing and Bundles

### `irium-wallet agreement-sign --agreement <agreement.json|-> --signer <addr>`

Signs an agreement with the private key of `<addr>`.

```
irium-wallet agreement-sign --agreement otc-002.json --signer QSellerAddress...
```

---

### `irium-wallet agreement-bundle-create <ref> --out <file>`

Creates a bundle wrapping an agreement and its signatures.

```
irium-wallet agreement-bundle-create otc-002.json --out bundle-002.json
```

---

### `irium-wallet agreement-bundle-inspect <ref>`

Prints the contents of a bundle.

```
irium-wallet agreement-bundle-inspect bundle-002.json
```

---

### `irium-wallet agreement-bundle-verify <ref>`

Verifies all signatures in a bundle.

```
irium-wallet agreement-bundle-verify bundle-002.json
```

---

### `irium-wallet agreement-bundle-sign --bundle <ref> --signer <addr>`

Adds a signature to a bundle.

```
irium-wallet agreement-bundle-sign --bundle bundle-002.json --signer QBuyerAddress...
```

---

## Agreement Pack / Unpack

`agreement-pack` and `agreement-unpack` are the fastest way to ship a full
agreement — including its policy, signatures, funding-tx record, and any
already-submitted proofs — to a counterparty or attestor without exposing
the wallet that owns it. The pack is a single self-describing JSON blob
which `agreement-unpack` can fully verify against the chain before any
write is performed locally.

### `irium-wallet agreement-pack --agreement <id|hash> --out <file> [--rpc <url>] [--json]`

Bundles an agreement's on-chain identity, stored policy, signatures, and any submitted proofs into a single JSON document at `<file>`. Pulls live state from the node so the pack is always consistent with the chain at the time of export.

```
irium-wallet agreement-pack --agreement otc-002 --out otc-002.pack.json
irium-wallet agreement-pack --agreement abcdef0123456789...32bytehex --out otc-002.pack.json
```

---

### `irium-wallet agreement-unpack --file <file> [--rpc <url>] [--json]`

Verifies and imports an agreement pack. Validates the document hash, agreement hash, every embedded signature, and confirms the on-chain status before adding anything to the local wallet store. Pass `--rpc` to point verification at a specific node.

```
irium-wallet agreement-unpack --file otc-002.pack.json
irium-wallet agreement-unpack --file otc-002.pack.json --rpc http://localhost:38300
```

---

## Share Packages

Share packages are used to exchange agreements and bundles between counterparties.

```
# Create a share package
irium-wallet agreement-share-package --out package.json

# Inspect a received package
irium-wallet agreement-share-package-inspect package.json

# Verify a package against the chain
irium-wallet agreement-share-package-verify package.json --rpc http://localhost:38300

# Import a package
irium-wallet agreement-share-package-import package.json --rpc http://localhost:38300

# List received packages
irium-wallet agreement-share-package-list
```

---

## OTC Shortcuts

These commands wrap the full agreement lifecycle into simpler single commands for common OTC flows.

### `irium-wallet otc-create`

Creates an OTC agreement with minimal flags.

```
irium-wallet otc-create \
  --seller QSellerAddress... \
  --buyer QBuyerAddress... \
  --amount 1.0 \
  --asset "50 USDT" \
  --payment-method bank-transfer \
  --timeout 20500
```

---

### `irium-wallet otc-attest`

Adds an attestation message to an OTC agreement.

```
irium-wallet otc-attest \
  --agreement otc-002.json \
  --message "Payment received in full" \
  --address QAttestorAddress...
```

---

### `irium-wallet otc-settle`

Executes the full settlement flow for an OTC agreement.

```
irium-wallet otc-settle --agreement otc-002.json --rpc http://localhost:38300
```

---

### `irium-wallet otc-status`

Returns the status of an OTC agreement.

```
irium-wallet otc-status --agreement otc-002.json
```

---

## Seller and Buyer Status

### `irium-wallet seller-status [--address <addr>] [--rpc <url>]`

Returns active agreements and reputation summary for a seller.

```
irium-wallet seller-status --address QSellerAddress...
```

---

### `irium-wallet buyer-status [--address <addr>] [--rpc <url>]`

Returns active agreements for a buyer.

```
irium-wallet buyer-status --address QBuyerAddress...
```
