#!/bin/bash
# Auto-update v1.1.8 release with latest fixes

VERSION="1.1.8"

echo "🔄 Updating v${VERSION} release..."

# Make sure we're on main
cd /home/irium/irium
git checkout main

# Build package
cd releases/v${VERSION}
./package.sh

# Upload to GitHub release only
gh release upload v${VERSION} irium-bootstrap-v${VERSION}.tar.gz --clobber

echo "✅ v${VERSION} GitHub release updated!"
echo "ℹ️  Website update skipped (manual if needed)"
