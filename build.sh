#!/bin/bash
#==============================================================================
# Go Magic Desktop - Process Isolation Build Script
#
# Features:
# - Auto build go-magic backend
# - Package frontend resources
# - Generate installers
#==============================================================================

set -e

# Colors
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'

log() { echo -e "${BLUE}[INFO]${NC} $*"; }
succ() { echo -e "${GREEN}[OK]${NC} $*"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
err() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

# Detect operating system
detect_os() {
    case "$(uname -s)" in
        Darwin*)  echo "macos" ;;
        Linux*)   echo "linux" ;;
        MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
        *)        echo "unknown" ;;
    esac
}

# Get backend binary extension
get_backend_ext() {
    case "$(detect_os)" in
        windows) echo ".exe" ;;
        *)       echo "" ;;
    esac
}

# Directories
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"
TAURI_DIR="$PROJECT_ROOT/src-tauri"
GOMAGIC_DIR="$PROJECT_ROOT/../go-magic"
RESOURCES_DIR="$TAURI_DIR/resources"

#==============================================================================
# Step 1: Check Environment
#==============================================================================
check_environment() {
    log "Checking build environment..."
    log "OS: $(detect_os)"

    command -v rustc >/dev/null || err "Rust not installed"
    command -v node >/dev/null || err "Node.js not installed"
    command -v go >/dev/null || err "Go not installed"

    # Check Tauri CLI
    if ! command -v tauri >/dev/null; then
        log "Installing Tauri CLI..."
        npm install -g @tauri-apps/cli
    fi

    succ "Environment check completed"
}

#==============================================================================
# Step 2: Build Frontend
#==============================================================================
build_frontend() {
    log "Building frontend..."

    # Determine frontend source and output locations
    local src_dir=""
    local output_dir=""

    if [ -d "$GOMAGIC_DIR/web" ]; then
        src_dir="$GOMAGIC_DIR/web"
        output_dir="$GOMAGIC_DIR/internal/server/dist"
    elif [ -d "$GOMAGIC_DIR/internal/server" ]; then
        src_dir="$GOMAGIC_DIR/internal/server"
        output_dir="$GOMAGIC_DIR/internal/server/dist"
    else
        warn "go-magic source not found, skipping frontend build"
        if [ ! -d "$PROJECT_ROOT/web-dist" ]; then
            err "web-dist directory also not found, cannot continue"
        fi
        warn "Using existing web-dist"
        return 0
    fi

    log "Frontend source: $src_dir"
    log "Output directory: $output_dir"

    cd "$src_dir"
    npm install
    npm run build

    # Copy to web-dist
    if [ -d "$output_dir" ]; then
        rm -rf "$PROJECT_ROOT/web-dist"
        cp -r "$output_dir" "$PROJECT_ROOT/web-dist"
        succ "Frontend built -> $PROJECT_ROOT/web-dist"
    else
        err "Frontend build failed: output directory not found"
    fi

    cd "$PROJECT_ROOT"
}

#==============================================================================
# Step 3: Build Backend
#==============================================================================
build_backend() {
    log "Building go-magic backend..."

    if [ ! -d "$GOMAGIC_DIR/cmd/magic" ]; then
        err "go-magic source directory not found: $GOMAGIC_DIR/cmd/magic"
    fi

    # Ensure resources directory exists
    mkdir -p "$RESOURCES_DIR"

    local ext=$(get_backend_ext)
    local output_file="$RESOURCES_DIR/go-magic$ext"

    cd "$GOMAGIC_DIR"

    log "Compiling go-magic to: $output_file"

    # Static compilation
    CGO_ENABLED=0 go build -ldflags="-s -w" \
        -o "$output_file" \
        ./cmd/magic

    if [ -f "$output_file" ]; then
        succ "Backend built successfully"
        ls -lh "$output_file"
    else
        err "Backend build failed"
    fi

    cd "$PROJECT_ROOT"
}

#==============================================================================
# Step 4: Package Tauri App
#==============================================================================
build_tauri() {
    log "Packaging Tauri application..."

    # Ensure web-dist exists
    if [ ! -d "$PROJECT_ROOT/web-dist" ]; then
        err "web-dist directory not found, please build frontend first"
    fi

    cd "$TAURI_DIR"

    local os=$(detect_os)
    local target=""

    case "$os" in
        macos)
            if [ "$(uname -m)" = "arm64" ]; then
                target="--target aarch64-apple-darwin"
            else
                target="--target x86_64-apple-darwin"
            fi
            ;;
        linux)
            target="--target x86_64-unknown-linux-gnu"
            ;;
        windows)
            target="--target x86_64-pc-windows-msvc"
            ;;
    esac

    log "Build target: ${target:-default}"

    if [ -n "$target" ]; then
        tauri build $target
    else
        tauri build
    fi

    cd "$PROJECT_ROOT"
    succ "Tauri application packaged successfully"
}

#==============================================================================
# Step 5: Show Results
#==============================================================================
show_result() {
    log "Build output:"

    local bundle_dir="$TAURI_DIR/target/release/bundle"

    echo ""
    echo "============================================"
    echo " Installer Locations"
    echo "============================================"

    find "$bundle_dir" -type f \( -name "*.exe" -o -name "*.msi" -o -name "*.dmg" -o -name "*.AppImage" -o -name "*.deb" \) 2>/dev/null | while read -r f; do
        echo "  $f"
        ls -lh "$f" 2>/dev/null | awk '{print "    Size: " $5}'
    done

    echo ""
    echo "============================================"
    echo " Resource Files"
    echo "============================================"
    ls -lh "$RESOURCES_DIR/" 2>/dev/null || true

    echo ""
}

#==============================================================================
# Main Function
#==============================================================================
main() {
    log "Go Magic Desktop Build Script"
    log "Mode: Process Isolation (Tauri + go-magic)"
    echo ""

    # Parse arguments
    case "${1:-all}" in
        all)
            check_environment
            build_frontend
            build_backend
            build_tauri
            show_result
            ;;
        frontend)
            check_environment
            build_frontend
            ;;
        backend)
            check_environment
            build_backend
            ;;
        tauri)
            check_environment
            build_tauri
            ;;
        clean)
            log "Cleaning..."
            rm -rf "$PROJECT_ROOT/web-dist"
            rm -rf "$RESOURCES_DIR/go-magic"*
            rm -rf "$TAURI_DIR/target"
            succ "Clean completed"
            ;;
        help|--help|-h)
            echo "Usage: $0 [options]"
            echo ""
            echo "Options:"
            echo "  all       Full build (default)"
            echo "  frontend  Build frontend only"
            echo "  backend   Build backend only"
            echo "  tauri     Package Tauri only"
            echo "  clean     Clean build artifacts"
            echo "  help      Show this help"
            ;;
        *)
            err "Unknown argument: $1"
            ;;
    esac
}

main "$@"