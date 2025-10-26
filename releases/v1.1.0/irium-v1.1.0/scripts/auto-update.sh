#!/bin/bash
# Irium Auto-Update Script (Opt-in)
# Run this with: bash scripts/auto-update.sh

set -e

REPO_DIR="/home/irium/irium"
BACKUP_DIR="/home/irium/irium-backup-$(date +%Y%m%d-%H%M%S)"

echo "=========================================="
echo "  Irium Auto-Update Script"
echo "=========================================="
echo ""

# Check current version
cd "$REPO_DIR"
CURRENT_VERSION=$(python3 -c "import sys; sys.path.insert(0, '.'); import irium; print(irium.__version__)")
echo "Current version: $CURRENT_VERSION"

# Fetch latest from GitHub
echo "Checking for updates..."
git fetch origin main

# Check if there are updates
if git diff --quiet HEAD origin/main; then
    echo "✅ Already up to date!"
    exit 0
fi

echo "⚠️  Updates available. Creating backup..."
# Backup current installation
cp -r "$REPO_DIR" "$BACKUP_DIR"
echo "✅ Backup created: $BACKUP_DIR"

# Pull updates
echo "Downloading updates..."
git pull origin main

# Get new version
NEW_VERSION=$(python3 -c "import sys; sys.path.insert(0, '.'); import irium; print(irium.__version__)")
echo "New version: $NEW_VERSION"

# Restart services
echo "Restarting services..."
sudo systemctl restart irium-node.service
sudo systemctl restart irium-miner.service 2>/dev/null || echo "Miner not running"
sudo systemctl restart irium-explorer-api.service 2>/dev/null || true

sleep 2

# Verify node started
if sudo systemctl is-active --quiet irium-node.service; then
    echo "✅ Update successful! Node running on version $NEW_VERSION"
else
    echo "❌ Update failed! Rolling back..."
    rm -rf "$REPO_DIR"
    mv "$BACKUP_DIR" "$REPO_DIR"
    sudo systemctl restart irium-node.service
    echo "✅ Rolled back to version $CURRENT_VERSION"
    exit 1
fi

echo ""
echo "=========================================="
echo "✅ AUTO-UPDATE COMPLETE"
echo "=========================================="
