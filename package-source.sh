#!/bin/bash

# Source Code Packaging Script for Go Magic Desktop (Desktop Only)
# This script packages only go-magic-desktop repository

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Default version (from package.json if not provided)
VERSION="${1:-$(node -p "require('./package.json').version")}"
PKG_NAME="go-magic-desktop-${VERSION}-source"

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}  Packaging Go Magic Desktop Source${NC}"
echo -e "${GREEN}  Version: ${VERSION}${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""

# Clean up previous package
if [ -d "${PKG_NAME}" ]; then
    echo -e "${YELLOW}Removing existing package directory...${NC}"
    rm -rf "${PKG_NAME}"
fi

if [ -f "${PKG_NAME}.tar.gz" ]; then
    echo -e "${YELLOW}Removing existing tar.gz archive...${NC}"
    rm -f "${PKG_NAME}.tar.gz"
fi

if [ -f "${PKG_NAME}.zip" ]; then
    echo -e "${YELLOW}Removing existing zip archive...${NC}"
    rm -f "${PKG_NAME}.zip"
fi

# Create package directory
echo -e "${GREEN}Creating package directory...${NC}"
mkdir -p "${PKG_NAME}"

# Copy desktop repo files (excluding unnecessary directories)
echo -e "${GREEN}Copying go-magic-desktop files...${NC}"
rsync -av \
    --exclude='.git' \
    --exclude='go-magic' \
    --exclude='target' \
    --exclude='node_modules' \
    --exclude='src-tauri/target' \
    --exclude='*.tar.gz' \
    --exclude='*.zip' \
    --exclude="${PKG_NAME}" \
    ./ "${PKG_NAME}/"

# Create version info file
echo -e "${GREEN}Creating version info...${NC}"
cat > "${PKG_NAME}/VERSION.txt" << EOF
Go Magic Desktop Source Package
Version: ${VERSION}
Packaged: $(date -u +"%Y-%m-%d %H:%M:%S UTC")

Contents:
- go-magic-desktop/ : Desktop application source (Tauri + Web)

For build instructions, see:
- README.md
EOF

# Create tar.gz archive
echo -e "${GREEN}Creating tar.gz archive...${NC}"
tar -czf "${PKG_NAME}.tar.gz" "${PKG_NAME}"

# Create zip archive
echo -e "${GREEN}Creating zip archive...${NC}"
zip -r "${PKG_NAME}.zip" "${PKG_NAME}"

# Generate checksums
echo -e "${GREEN}Generating checksums...${NC}"
sha256sum "${PKG_NAME}.tar.gz" "${PKG_NAME}.zip" > "${PKG_NAME}-checksums.txt"

# Clean up package directory
echo -e "${GREEN}Cleaning up...${NC}"
rm -rf "${PKG_NAME}"

# Display results
echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}  Source Package Created Successfully!${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo -e "${YELLOW}Files:${NC}"
ls -lh "${PKG_NAME}".*
echo ""
echo -e "${YELLOW}Checksums:${NC}"
cat "${PKG_NAME}-checksums.txt"
echo ""
echo -e "${GREEN}Done!${NC}"
