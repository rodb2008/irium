# Understanding Your IRM Balance

## How Balances Work
Irium uses a UTXO model, like Bitcoin. Your spendable balance is the sum of unspent outputs that pay to your address.

## Check Your Balance
```bash
./target/release/irium-wallet balance <base58_address>
```
This shows the spendable balance and the number of mined blocks that are still unspent.

## List Spendable UTXOs
```bash
./target/release/irium-wallet list-unspent <base58_address>
```
Coinbase outputs are filtered until they reach maturity, so only spendable UTXOs are listed.

## Mining Rewards
- Rewards are paid to the address you set in `IRIUM_MINER_ADDRESS` or `IRIUM_MINER_PKH`.
- Coinbase rewards are locked until `COINBASE_MATURITY` confirmations.

## Wallet File
By default, wallet keys are stored in:
```
~/.irium/wallet.json
```
Override with `IRIUM_WALLET_FILE`.
