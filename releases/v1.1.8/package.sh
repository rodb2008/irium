#!/bin/bash
VERSION="1.1.8"
PACKAGE="irium-bootstrap-v${VERSION}"

echo "📦 Creating ${PACKAGE}..."

# Create package directory
mkdir -p ${PACKAGE}

# Copy files
cp -r ../../bootstrap ${PACKAGE}/
cp -r ../../irium ${PACKAGE}/
cp -r ../../scripts ${PACKAGE}/
cp -r ../../configs ${PACKAGE}/
cp ../../install.sh ${PACKAGE}/
cp ../../README.md ${PACKAGE}/
cp ../../WHITEPAPER.md ${PACKAGE}/
cp ../../MINING.md ${PACKAGE}/
cp ../../LICENSE ${PACKAGE}/
cp ../../VERSION ${PACKAGE}/
cp ../../RELEASE_NOTES_v1.1.8.md ${PACKAGE}/

# Create archives
tar -czf ${PACKAGE}.tar.gz ${PACKAGE}
zip -r ${PACKAGE}.zip ${PACKAGE} > /dev/null

# Cleanup
rm -rf ${PACKAGE}

echo "✅ Packages created:"
ls -lh ${PACKAGE}.*

# Calculate checksums
sha256sum ${PACKAGE}.tar.gz
sha256sum ${PACKAGE}.zip
