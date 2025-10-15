#!/usr/bin/env bash
set -euo pipefail

echo "=== Updating GitHub Release with Wallet Integration ==="

# Configuration
REPO="iriumlabs/irium"
TAG="Irium-bootstrap-v1"
GITHUB_TOKEN="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"
RELEASE_API="https://api.github.com/repos/$REPO/releases"

# Get release ID
echo "📋 Getting release information..."
RELEASE_RESPONSE=$(curl -s -H "Authorization: token $GITHUB_TOKEN" "$RELEASE_API/tags/$TAG")
RELEASE_ID=$(echo "$RELEASE_RESPONSE" | jq -r '.id')

if [ "$RELEASE_ID" = "null" ]; then
    echo "❌ Release not found: $TAG"
    exit 1
fi

echo "✅ Found release ID: $RELEASE_ID"

# Download existing assets
echo "📥 Downloading existing assets..."
mkdir -p temp_release
cd temp_release

# Get existing assets
ASSETS=$(curl -s -H "Authorization: token $GITHUB_TOKEN" "$RELEASE_API/$RELEASE_ID/assets" | jq -r '.[].name')

for asset in $ASSETS; do
    if [ "$asset" != "null" ] && [ -n "$asset" ]; then
        echo "  Downloading: $asset"
        curl -s -L -H "Authorization: token $GITHUB_TOKEN" \
            "$RELEASE_API/$RELEASE_ID/assets" | \
            jq -r ".[] | select(.name==\"$asset\") | .browser_download_url" | \
            xargs curl -s -L -o "$asset"
    fi
done

# Extract existing archive if it exists
if [ -f "Irium-bootstrap-v1-all.tar.gz" ]; then
    echo "📦 Extracting existing archive..."
    tar -xzf "Irium-bootstrap-v1-all.tar.gz"
    rm "Irium-bootstrap-v1-all.tar.gz"
fi

# Copy new wallet integration files from VPS
echo "🔄 Merging new wallet integration files..."
rsync -av --ignore-existing ../scripts/irium-wallet-* ./
rsync -av --ignore-existing ../scripts/irium-web3-provider.js ./
rsync -av --ignore-existing ../docs/wallet-integration.md ./

# Copy all other updated files
rsync -av --ignore-existing ../ ./

# Create comprehensive archive
echo "📦 Creating comprehensive archive..."
tar -czf "Irium-bootstrap-v1-wallet-integration.tar.gz" \
    --exclude="temp_release" \
    --exclude=".git" \
    --exclude="target" \
    --exclude="*.log" \
    .

# Upload new archive
echo "⬆️ Uploading comprehensive archive..."
UPLOAD_URL="$RELEASE_API/$RELEASE_ID/assets?name=Irium-bootstrap-v1-wallet-integration.tar.gz"

curl -X POST \
    -H "Authorization: token $GITHUB_TOKEN" \
    -H "Content-Type: application/gzip" \
    --data-binary "@Irium-bootstrap-v1-wallet-integration.tar.gz" \
    "$UPLOAD_URL"

# Clean up old assets (keep only the newest)
echo "🧹 Cleaning up old assets..."
ASSET_IDS=$(curl -s -H "Authorization: token $GITHUB_TOKEN" "$RELEASE_API/$RELEASE_ID/assets" | jq -r '.[] | select(.name | contains("Irium-bootstrap-v1")) | .id')

for asset_id in $ASSET_IDS; do
    if [ "$asset_id" != "null" ] && [ -n "$asset_id" ]; then
        ASSET_NAME=$(curl -s -H "Authorization: token $GITHUB_TOKEN" "$RELEASE_API/$RELEASE_ID/assets" | jq -r ".[] | select(.id==$asset_id) | .name")
        if [ "$ASSET_NAME" != "Irium-bootstrap-v1-wallet-integration.tar.gz" ]; then
            echo "  Deleting old asset: $ASSET_NAME"
            curl -X DELETE -H "Authorization: token $GITHUB_TOKEN" "$RELEASE_API/$RELEASE_ID/assets/$asset_id"
        fi
    fi
done

# Update release description
echo "📝 Updating release description..."
NEW_DESCRIPTION="## Irium Bootstrap v1 - Complete Wallet Integration

### 🚀 Features
- ✅ **Zero-DNS Bootstrap** - Signed seedlist.txt and anchors.json
- ✅ **Self-healing Peer Discovery** - libp2p + gossip protocol
- ✅ **Genesis Vesting** - On-chain CLTV with 3.5M IRM locked
- ✅ **SSL-enabled Wallet API** - HTTPS support for external wallets
- ✅ **Web3 Provider** - MetaMask, Trust Wallet integration
- ✅ **Systemd Service** - Auto-start on boot
- ✅ **Real Genesis Block** - Calculated merkle root and proof-of-work

### 🔗 Wallet Integration
- **API Endpoint**: https://207.244.247.86/api
- **Web3 Provider**: Compatible with MetaMask, Trust Wallet
- **SSL Certificate**: Self-signed (production-ready with domain)
- **CORS Support**: Full cross-origin support

### 📦 Contents
- Complete Irium node implementation
- Wallet CLI tools and API server
- Web3 provider for external wallet integration
- SSL-enabled HTTPS API
- Systemd service configuration
- Bootstrap files with signatures
- Real genesis block with calculated values

### 🛠️ Quick Start
\`\`\`bash
# Extract and run
tar -xzf Irium-bootstrap-v1-wallet-integration.tar.gz
cd irium
./scripts/irium-zero.sh

# Create wallet
python3 scripts/irium-wallet-integration.py create-wallet

# Test API
python3 scripts/irium-wallet-integration.py api-test
\`\`\`

### 🔒 Security
- Founder WIF key derived and used for genesis vesting
- Real calculated merkle root and genesis hash
- SSL-enabled API with CORS support
- Signed bootstrap files (dev mode)

**Bootstrap Node**: 207.244.247.86:19444
**Wallet API**: https://207.244.247.86/api"

curl -X PATCH \
    -H "Authorization: token $GITHUB_TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"body\": $(echo "$NEW_DESCRIPTION" | jq -Rs .)}" \
    "$RELEASE_API/$RELEASE_ID"

# Cleanup
cd ..
rm -rf temp_release

echo "✅ GitHub release updated successfully!"
echo "🔗 Release URL: https://github.com/$REPO/releases/tag/$TAG"
echo "📦 New asset: Irium-bootstrap-v1-wallet-integration.tar.gz"
