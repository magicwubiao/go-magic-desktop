#!/bin/bash

# Source Code Packaging Script for Go Magic Desktop
# Version is derived from git tags — no manual version maintenance needed.
#
# Usage:
#   ./package-source.sh

set -e

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Version from git describe (e.g. v0.5.0 -> 0.5.0); fallback to "dev"
VERSION="$(git describe --tags --abbrev=0 2>/dev/null | sed 's/^v//' || echo "dev")"
GIT_COMMIT="$(git rev-parse --short=8 HEAD 2>/dev/null || echo 'unknown')"
PKG_NAME="go-magic-desktop-${VERSION}-source"

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}  Packaging Go Magic Desktop Source${NC}"
echo -e "${GREEN}  Version: ${VERSION}${NC}"
echo -e "${GREEN}  Commit:  ${GIT_COMMIT}${NC}"
echo -e "${GREEN}========================================${NC}"

rm -rf "${PKG_NAME}" "${PKG_NAME}.tar.gz" "${PKG_NAME}.zip"
mkdir -p "${PKG_NAME}"

rsync -av \
    --exclude='.git' \
    --exclude='.coze' \
    --exclude='go-magic' \
    --exclude='target' \
    --exclude='node_modules' \
    --exclude='src-tauri/target' \
    --exclude='src-tauri/gen' \
    --exclude='*.tar.gz' \
    --exclude='*.zip' \
    --exclude='*-checksums.txt' \
    --exclude='VERSION.txt' \
    --exclude="${PKG_NAME}" \
    ./ "${PKG_NAME}/" > /dev/null

cat > "${PKG_NAME}/VERSION.txt" << EOF
Go Magic Desktop Source Package
Version: ${VERSION}
Commit:  ${GIT_COMMIT}
Packaged: $(date -u +"%Y-%m-%d %H:%M:%S UTC")

For build instructions, see README.md
EOF

echo -e "${YELLOW}Creating archives...${NC}"
tar -czf "${PKG_NAME}.tar.gz" "${PKG_NAME}"
zip -rq "${PKG_NAME}.zip" "${PKG_NAME}"
sha256sum "${PKG_NAME}.tar.gz" "${PKG_NAME}.zip" > "${PKG_NAME}-checksums.txt"
rm -rf "${PKG_NAME}"

echo ""
echo -e "${GREEN}Done!${NC}"
ls -lh "${PKG_NAME}".*
echo ""
cat "${PKG_NAME}-checksums.txt"
