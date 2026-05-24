#!/bin/bash
#==============================================================================
# Go Magic Desktop - Multi-platform Build Script
#==============================================================================

set -e

# Color definitions
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Log functions
log_info() { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Variables
PROJECT_NAME="go-magic-desktop"
BUILD_DIR="src-tauri/target/release"
BUNDLE_DIR="$BUILD_DIR/bundle"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

# Show help
show_help() {
    cat << EOF
Go Magic Desktop Build Script

Usage: $0 [options]

Options:
    all         Build all platforms (default)
    windows     Build Windows version
    macos       Build macOS version (Intel)
    macos-arm   Build macOS version (Apple Silicon)
    linux       Build Linux version (x64)
    linux-arm   Build Linux version (ARM64)
    web         Build frontend only
    clean       Clean build artifacts
    help        Show this help message

Examples:
    $0 all          # Build all platforms
    $0 windows      # Build Windows only
    $0 web clean    # Clean and rebuild frontend

EOF
}

# Check dependencies
check_dependencies() {
    log_info "Checking build dependencies..."

    # Check Rust
    if ! command -v rustc &> /dev/null; then
        log_error "Rust not installed"
        exit 1
    fi

    # Check Node.js
    if ! command -v node &> /dev/null; then
        log_error "Node.js not installed"
        exit 1
    fi

    # Check npm
    if ! command -v npm &> /dev/null; then
        log_error "npm not installed"
        exit 1
    fi

    # Check Tauri CLI
    if ! command -v tauri &> /dev/null; then
        log_warn "Tauri CLI not installed, installing..."
        npm install -g @tauri-apps/cli
    fi

    log_success "Dependency check completed"
}

# Install npm dependencies
install_deps() {
    log_info "Installing project dependencies..."
    npm install
    log_success "Dependencies installed"
}

# Build frontend
build_frontend() {
    log_info "Building frontend..."

    # Check go-magic/web directory
    if [ -d "../go-magic/web" ]; then
        cd ../go-magic/web
        npm install
        npm run build
        cd - > /dev/null
        log_success "Frontend built successfully"
    else
        log_warn "go-magic/web directory not found, skipping frontend build"
        log_info "Ensure web-dist directory exists"
    fi
}

# Clean build artifacts
clean_build() {
    log_info "Cleaning build artifacts..."
    cd src-tauri
    cargo clean
    rm -rf target 2>/dev/null || true
    rm -rf "$BUNDLE_DIR" 2>/dev/null || true
    cd - > /dev/null
    log_success "Clean completed"
}

# Build Windows version
build_windows() {
    log_info "Building Windows version..."
    tauri build --target x86_64-pc-windows-msvc
    log_success "Windows build completed"
}

# Build macOS Intel version
build_macos() {
    log_info "Building macOS version (Intel)..."
    tauri build --target x86_64-apple-darwin
    log_success "macOS (Intel) build completed"
}

# Build macOS ARM version
build_macos_arm() {
    log_info "Building macOS version (Apple Silicon)..."
    tauri build --target aarch64-apple-darwin
    log_success "macOS (Apple Silicon) build completed"
}

# Build Linux x64 version
build_linux() {
    log_info "Building Linux version (x64)..."
    tauri build --target x86_64-unknown-linux-gnu
    log_success "Linux (x64) build completed"
}

# Build Linux ARM64 version
build_linux_arm() {
    log_info "Building Linux version (ARM64)..."
    tauri build --target aarch64-unknown-linux-gnu
    log_success "Linux (ARM64) build completed"
}

# Build all platforms
build_all() {
    log_info "Starting build for all platforms..."

    # Build frontend
    build_frontend

    # Detect current platform and build
    case "$(uname -s)" in
        Linux*)
            log_info "Detected Linux system"
            build_linux
            ;;
        Darwin*)
            log_info "Detected macOS system"
            if [ "$(uname -m)" = "arm64" ]; then
                build_macos_arm
            else
                build_macos
            fi
            ;;
        MINGW*|CYGWIN*|MSYS*)
            log_info "Detected Windows system"
            build_windows
            ;;
        *)
            log_error "Unsupported operating system"
            exit 1
            ;;
    esac

    log_success "All builds completed!"
    show_output_info
}

# Show build output info
show_output_info() {
    echo ""
    echo "=========================================="
    echo "Build Output:"
    echo "=========================================="

    if [ -d "$BUNDLE_DIR/nsis" ]; then
        echo -e "${GREEN}Windows NSIS:${NC}"
        ls -la "$BUNDLE_DIR/nsis/"*.exe 2>/dev/null || true
    fi

    if [ -d "$BUNDLE_DIR/msi" ]; then
        echo -e "${GREEN}Windows MSI:${NC}"
        ls -la "$BUNDLE_DIR/msi/"*.msi 2>/dev/null || true
    fi

    if [ -d "$BUNDLE_DIR/dmg" ]; then
        echo -e "${GREEN}macOS DMG:${NC}"
        ls -la "$BUNDLE_DIR/dmg/"*.dmg 2>/dev/null || true
    fi

    if [ -d "$BUNDLE_DIR/appimage" ]; then
        echo -e "${GREEN}Linux AppImage:${NC}"
        ls -la "$BUNDLE_DIR/appimage/"*.AppImage 2>/dev/null || true
    fi

    if [ -d "$BUNDLE_DIR/deb" ]; then
        echo -e "${GREEN}Linux DEB:${NC}"
        ls -la "$BUNDLE_DIR/deb/"*.deb 2>/dev/null || true
    fi

    echo "=========================================="
}

# Main function
main() {
    # Switch to project root directory
    cd "$(dirname "$0")"

    # Parse arguments
    case "${1:-all}" in
        all)
            check_dependencies
            build_all
            ;;
        windows)
            check_dependencies
            build_frontend
            build_windows
            ;;
        macos)
            check_dependencies
            build_frontend
            build_macos
            ;;
        macos-arm)
            check_dependencies
            build_frontend
            build_macos_arm
            ;;
        linux)
            check_dependencies
            build_frontend
            build_linux
            ;;
        linux-arm)
            check_dependencies
            build_frontend
            build_linux_arm
            ;;
        web)
            build_frontend
            ;;
        clean)
            clean_build
            ;;
        help|--help|-h)
            show_help
            ;;
        *)
            log_error "Unknown argument: $1"
            show_help
            exit 1
            ;;
    esac
}

# Execute main function
main "$@"