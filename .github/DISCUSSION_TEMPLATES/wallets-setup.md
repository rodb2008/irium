# Wallet Setup & Management Guide

Learn how to create and manage your Irium wallet.

## Creating a New Wallet

```bash
python3 scripts/irium-wallet-proper.py new
```

This will:
- Generate a new HD wallet
- Create your first address
- Save wallet data securely

## Generating New Addresses

```bash
python3 scripts/irium-wallet-proper.py address
```

## Checking Your Balance

```bash
python3 scripts/irium-wallet-proper.py balance
```

## Sending IRM

```bash
python3 scripts/irium-wallet-proper.py send <address> <amount>
```

## Important Notes

⚠️ **BACKUP YOUR WALLET!**
- Your wallet file is `irium-wallet.json`
- Keep it safe and backed up
- Never share your private keys

⚠️ **Address Prefixes**
- Irium addresses start with `P` or `Q`
- Always double-check addresses before sending

## Need Help?

Having wallet issues? Reply below with:
- Your OS
- What you're trying to do
- Any error messages

We're here to help! 🤝
