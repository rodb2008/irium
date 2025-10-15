# Irium Quick Reference

## 🔗 Important URLs
- **Bootstrap Node**: 207.244.247.86:19444
- **Wallet API**: https://207.244.247.86/api
- **Logo URL**: http://207.244.247.86:8080/irium-logo-wallet.svg
- **GitHub**: https://github.com/iriumlabs/irium

## 🎯 Key Commands
```bash
# Start Irium node
./scripts/irium-zero.sh

# Create wallet
python3 scripts/irium-wallet-integration.py create-wallet

# Check status
python3 scripts/irium-wallet-integration.py status

# Test API
python3 scripts/irium-wallet-integration.py api-test

# Show logo
python3 scripts/show-logo.py
```

## 📱 External Wallet Integration
- **Chain ID**: 1
- **Chain Name**: Irium Mainnet
- **Symbol**: IRM
- **Decimals**: 8
- **RPC URL**: https://207.244.247.86/api
- **Logo URL**: http://207.244.247.86:8080/irium-logo-wallet.svg

## 🔧 Services
- **iriumd.service**: Main blockchain daemon
- **irium-wallet-api.service**: Wallet API server
- **nginx**: Reverse proxy with SSL
