#!/bin/bash
# Package v1.1.9 release

VERSION="v1.1.9"
PACKAGE_NAME="irium-bootstrap-${VERSION}.tar.gz"

echo "📦 Packaging Irium ${VERSION}..."

# Create temp directory
TMP_DIR="/tmp/irium-${VERSION}"
rm -rf "$TMP_DIR"
mkdir -p "$TMP_DIR"

# Copy essential files
cp -r irium "$TMP_DIR/"
cp -r scripts "$TMP_DIR/"
cp -r configs "$TMP_DIR/"
cp -r bootstrap "$TMP_DIR/"
cp VERSION "$TMP_DIR/"
cp README.md "$TMP_DIR/"
cp WHITEPAPER.md "$TMP_DIR/"
cp MINING.md "$TMP_DIR/"
cp WALLET.md "$TMP_DIR/"
cp LICENSE "$TMP_DIR/"
cp install.sh "$TMP_DIR/"
cp RELEASE_NOTES_${VERSION}.md "$TMP_DIR/"

# Create tarball
cd /tmp
tar -czf "$PACKAGE_NAME" "irium-${VERSION}/"

# Move to releases
mv "$PACKAGE_NAME" /home/irium/irium/releases/${VERSION}/

# Calculate SHA256
cd /home/irium/irium/releases/${VERSION}/
sha256sum "$PACKAGE_NAME" > "${PACKAGE_NAME}.sha256"

echo "✅ Package created: releases/${VERSION}/${PACKAGE_NAME}"
echo "✅ SHA256: $(cat ${PACKAGE_NAME}.sha256)"

# Cleanup
rm -rf "$TMP_DIR"
