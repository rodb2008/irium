# Irium Wallet Guide

## Initialize Wallet File
```bash
./target/release/irium-wallet init
```
Creates `~/.irium/wallet.json` (or `IRIUM_WALLET_FILE`) and prints a new address.

## Add a New Address
```bash
./target/release/irium-wallet new-address
```
Adds a new key to the wallet file and prints the address.

## List Addresses
```bash
./target/release/irium-wallet list-addresses
```

## Convert Address to PKH
```bash
./target/release/irium-wallet address-to-pkh <base58_address>
```

## Check Balance
```bash
./target/release/irium-wallet balance <base58_address>
```
- Defaults to `https://127.0.0.1:38300`.
- For self-signed TLS, the wallet auto-loads `/etc/irium/tls/irium-ca.crt` if present.
- Override with `IRIUM_RPC_URL` or `IRIUM_RPC_CA` as needed.

## List Spendable UTXOs
```bash
./target/release/irium-wallet list-unspent <base58_address>
```
Coinbase UTXOs are filtered until they reach maturity.

## Send a Transaction
```bash
./target/release/irium-wallet send <from_addr> <to_addr> <amount_irm>
```
Optional fee override:
```bash
./target/release/irium-wallet send <from_addr> <to_addr> <amount_irm> --fee 0.01
```
If `--fee` is not provided, the wallet uses a default fee of 1 atom/byte.

## Public Balance Access
For public read-only access, run the wallet API on `0.0.0.0` and expose `/balance` and `/utxos`:
- `IRIUM_WALLET_API_HOST=0.0.0.0`
- Optional TLS: `IRIUM_WALLET_API_TLS_CERT` and `IRIUM_WALLET_API_TLS_KEY`

Keep `submit_tx` protected with `IRIUM_WALLET_API_TOKEN`.
