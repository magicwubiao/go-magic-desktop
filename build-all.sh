#!/bin/bash
# Build Go Magic Desktop for all platforms

set -e

echo "=========================================="
echo "Go Magic Desktop - Multi-Platform Build"
echo "=========================================="

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check prerequisites
check_prerequisites() {
    echo -e "${YELLOW}Checking prerequisites...${NC}"
    
    # Check Node.js
    if ! command -v node &> /dev/null; then
        echo -e "${RED}Node.js is not installed. Please install Node.js 18+${NC}"
        exit 1
    fi
    
    # Check Rust
    if ! command -v rustc &> /dev/null; then
        echo -e "${RED}Rust is not installed. Installing...${NC}"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source ~/.cargo/env
    fi
    
    # Check Tauri CLI
    if ! command -v tauri &> /dev/null; then
        echo -e "${YELLOW}Installing Tauri CLI...${NC}"
        npm install -g @tauri-apps/cli
    fi
    
    echo -e "${GREEN}Prerequisites check passed!${NC}"
}

# Build frontend
build_frontend() {
    echo -e "${YELLOW}Building frontend...${NC}"
    cd ../go-magic/web
    npm install
    npm run build
    cd ../../go-magic-tauri
    echo -e "${GREEN}Frontend build complete!${NC}"
}

# Build for current platform
build_current_platform() {
    echo -e "${YELLOW}Building for current platform...${NC}"
    
    # Install dependencies
    npm install
    
    # Build Tauri app
    tauri build
    
    echo -e "${GREEN}Build complete!${NC}"
}

# Build for Windows (cross-compile from Linux/Mac)
build_windows() {
    echo -e "${YELLOW}Building for Windows...${NC}"
    
    # Check if cross-compilation is possible
    if command -v x86_64-w64-mingw32-gcc &> /dev/null; then
        rustup target add x86_64-pc-windows-msvc || true
        rustup target add x86_64-pc-windows-gnu
        
        # Build with cargo directly for Windows target
        cd src-tauri
        cargo build --release --target x86_64-pc-windows-gnu
        cd ..
        
        echo -e "${GREEN}Windows build complete!${NC}"
    else
        echo -e "${YELLOW}Windows cross-compilation not available. Install mingw-w64 for cross-compilation.${NC}"
        echo -e "${YELLOW}Skipping Windows build.${NC}"
    fi
}

# Build for Linux ARM64 (for Raspberry Pi, etc.)
build_linux_arm64() {
    echo -e "${YELLOW}Building for Linux ARM64...${NC}"
    
    rustup target add aarch64-unknown-linux-gnu || true
    
    cd src-tauri
    cargo build --release --target aarch64-unknown-linux-gnu || {
        echo -e "${YELLOW}ARM64 build failed. You may need to install cross-compilation tools.${NC}"
    }
    cd ..
}

# Package builds
package_builds() {
    echo -e "${YELLOW}Packaging builds...${NC}"
    
    mkdir -p dist
    
    # Copy current platform build
    if [ -d "src-tauri/target/release/bundle" ]; then
        cp -r src-tauri/target/release/bundle/* dist/ 2>/dev/null || true
    fi
    
    # List outputs
    echo -e "${GREEN}Build outputs:${NC}"
    find dist -type f -name "*.msi" -o -name "*.dmg" -o -name "*.deb" -o -name "*.AppImage" 2>/dev/null || echo "No bundles found"
    
    # Also copy the binary directly
    if [ -f "src-tauri/target/release/go-magic-desktop" ]; then
        cp src-tauri/target/release/go-magic-desktop dist/go-magic-desktop-linux
    fi
    
    if [ -f "src-tauri/target/release/go-magic-desktop.exe" ]; then
        cp src-tauri/target/release/go-magic-desktop.exe dist/go-magic-desktop-windows.exe
    fi
}

# Main
main() {
    check_prerequisites
    build_frontend
    build_current_platform
    
    # Try to build for other platforms if tools are available
    if [[ "$OSTYPE" == "linux-gnu"* ]]; then
        build_windows
    fi
    
    package_builds
    
    echo -e "${GREEN}==========================================${NC}"
    echo -e "${GREEN}Build process complete!${NC}"
    echo -e "${GREEN}==========================================${NC}"
    echo -e "Check the ${YELLOW}dist/${NC} directory for build outputs."
}

# Run main function
main "$@"
