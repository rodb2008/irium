# Irium Wallet Guide

## Create Address
```bash
./target/release/irium-wallet new-address
```
The command prints `address`, `pubkey`, and `privkey`. Save the private key securely; the CLI does not store a wallet file for you.

## Convert Address to PKH
```bash
./target/release/irium-wallet address-to-pkh <base58_address>
```

## Check Balance
```bash
IRIUM_RPC_URL=http://127.0.0.1:38300 IRIUM_RPC_TOKEN=<node_token> ./target/release/irium-wallet balance <base58_address>
```
- For HTTPS with a self-signed cert, set `IRIUM_RPC_CA=/etc/irium/tls/irium-ca.crt`.
- For dev-only, you can set `IRIUM_RPC_INSECURE=1` to skip TLS validation.
