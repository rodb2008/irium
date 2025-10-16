# Irium Wallet Integration Guide

## Overview
This guide explains how to integrate Irium wallets with external wallet UIs like MetaMask, Trust Wallet, and other Web3-compatible wallets.

## Features
- ✅ SSL-enabled REST API
- ✅ Web3 Provider interface
- ✅ MetaMask integration
- ✅ Trust Wallet compatibility
- ✅ CORS support for web applications
- ✅ Self-signed SSL certificate (production-ready with domain)

## API Endpoints

### Base URL
- **HTTP**: `http://207.244.247.86:8080/api`
- **HTTPS**: `https://207.244.247.86/api` (SSL-enabled)

### Available Endpoints

#### 1. Wallet Status
```bash
GET /api/wallet/status
```
**Response:**
```json
{
  "status": "success",
  "data": {
    "addresses": ["Q5uT1k6DR7WpxqYuiy7sQQXp8pYDx6U4eS"],
    "balance": 0,
    "network": "irium-mainnet",
    "ssl_enabled": true
  }
}
```

#### 2. Get Addresses
```bash
GET /api/wallet/addresses
```

#### 3. Get Balance
```bash
GET /api/wallet/balance
```

#### 4. Network Info
```bash
GET /api/network/info
```

## Web3 Provider Integration

### For MetaMask Users

1. **Add Irium Network to MetaMask:**
```javascript
// Load the Web3 provider
const iriumProvider = new IriumWeb3Provider();

// Add to MetaMask
await iriumProvider.addToMetaMask();
```

2. **Connect to Irium Network:**
```javascript
// Request accounts
const accounts = await window.ethereum.request({
    method: 'eth_requestAccounts'
});

// Get balance
const balance = await window.ethereum.request({
    method: 'eth_getBalance',
    params: [accounts[0], 'latest']
});
```

### For Trust Wallet Users

Trust Wallet users can manually add the Irium network:
- **Network Name**: Irium Mainnet
- **RPC URL**: `https://207.244.247.86/api`
- **Chain ID**: 1
- **Symbol**: IRM
- **Block Explorer**: `https://207.244.247.86`

## CLI Wallet Tools

### Create Wallet
```bash
python3 scripts/irium-wallet-integration.py create-wallet
```

### Check Status
```bash
python3 scripts/irium-wallet-integration.py status
```

### Test API
```bash
python3 scripts/irium-wallet-integration.py api-test
```

## Security Notes

1. **SSL Certificate**: Currently using self-signed certificate. For production, obtain a valid SSL certificate for your domain.

2. **CORS**: API allows all origins (`*`). Restrict in production.

3. **Private Keys**: Never expose private keys in client-side code.

4. **HTTPS Only**: Always use HTTPS in production environments.

## Supported Wallets

- ✅ MetaMask
- ✅ Trust Wallet
- ✅ Coinbase Wallet
- ✅ WalletConnect-compatible wallets
- ✅ Any Web3-compatible wallet

## Network Details

- **Chain ID**: 1
- **Ticker**: IRM
- **Decimals**: 8
- **Block Time**: 600 seconds
- **Consensus**: Proof-of-Work (SHA-256d)
- **Bootstrap Node**: `207.244.247.86:19444`

## Troubleshooting

### SSL Certificate Warnings
If you see SSL certificate warnings, this is expected with self-signed certificates. Users can:
1. Accept the certificate in their browser
2. Add an exception for the site
3. Use the HTTP endpoint for testing

### CORS Issues
If you encounter CORS issues, ensure your application is making requests to the correct HTTPS endpoint.

### Network Connection
If the API is unreachable, check:
1. The VPS is running
2. Nginx is running: `sudo systemctl status nginx`
3. The wallet API service is running: `sudo systemctl status irium-wallet-api`
4. Firewall allows HTTPS traffic: `sudo ufw status`

## Development

### Local Testing
```bash
# Start the wallet API locally
python3 scripts/irium-wallet-api-ssl.py

# Test endpoints
curl -k https://207.244.247.86/api/wallet/status
```

### Production Deployment
1. Obtain a valid SSL certificate for your domain
2. Update nginx configuration with the certificate
3. Restrict CORS to your domain
4. Set up proper firewall rules
5. Monitor API usage and performance
