#!/bin/bash
#==============================================================================
# Go Magic Desktop - 多平台构建脚本
#==============================================================================

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 日志函数
log_info() { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# 变量
PROJECT_NAME="go-magic-desktop"
BUILD_DIR="src-tauri/target/release"
BUNDLE_DIR="$BUILD_DIR/bundle"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

# 显示帮助
show_help() {
    cat << EOF
Go Magic Desktop 构建脚本

用法: $0 [选项]

选项:
    all         构建所有平台 (默认)
    windows     构建 Windows 版本
    macos       构建 macOS 版本 (Intel)
    macos-arm   构建 macOS 版本 (Apple Silicon)
    linux       构建 Linux 版本 (x64)
    linux-arm   构建 Linux 版本 (ARM64)
    web         仅构建前端
    clean       清理构建产物
    help        显示此帮助信息

示例:
    $0 all          # 构建所有平台
    $0 windows      # 仅构建 Windows
    $0 web clean    # 清理并重新构建前端

EOF
}

# 检查依赖
check_dependencies() {
    log_info "检查构建依赖..."

    # 检查 Rust
    if ! command -v rustc &> /dev/null; then
        log_error "Rust 未安装"
        exit 1
    fi

    # 检查 Node.js
    if ! command -v node &> /dev/null; then
        log_error "Node.js 未安装"
        exit 1
    fi

    # 检查 npm
    if ! command -v npm &> /dev/null; then
        log_error "npm 未安装"
        exit 1
    fi

    # 检查 Tauri CLI
    if ! command -v tauri &> /dev/null; then
        log_warn "Tauri CLI 未安装，正在安装..."
        npm install -g @tauri-apps/cli
    fi

    log_success "依赖检查完成"
}

# 安装 npm 依赖
install_deps() {
    log_info "安装项目依赖..."
    npm install
    log_success "依赖安装完成"
}

# 构建前端
build_frontend() {
    log_info "构建前端..."

    # 检查 go-magic/web 目录
    if [ -d "../go-magic/web" ]; then
        cd ../go-magic/web
        npm install
        npm run build
        cd - > /dev/null
        log_success "前端构建完成"
    else
        log_warn "go-magic/web 目录不存在，跳过前端构建"
        log_info "请确保 web-dist 目录存在"
    fi
}

# 清理构建产物
clean_build() {
    log_info "清理构建产物..."
    cd src-tauri
    cargo clean
    rm -rf target 2>/dev/null || true
    rm -rf "$BUNDLE_DIR" 2>/dev/null || true
    cd - > /dev/null
    log_success "清理完成"
}

# 构建 Windows 版本
build_windows() {
    log_info "构建 Windows 版本..."
    tauri build --target x86_64-pc-windows-msvc
    log_success "Windows 构建完成"
}

# 构建 macOS Intel 版本
build_macos() {
    log_info "构建 macOS 版本 (Intel)..."
    tauri build --target x86_64-apple-darwin
    log_success "macOS (Intel) 构建完成"
}

# 构建 macOS ARM 版本
build_macos_arm() {
    log_info "构建 macOS 版本 (Apple Silicon)..."
    tauri build --target aarch64-apple-darwin
    log_success "macOS (Apple Silicon) 构建完成"
}

# 构建 Linux x64 版本
build_linux() {
    log_info "构建 Linux 版本 (x64)..."
    tauri build --target x86_64-unknown-linux-gnu
    log_success "Linux (x64) 构建完成"
}

# 构建 Linux ARM64 版本
build_linux_arm() {
    log_info "构建 Linux 版本 (ARM64)..."
    tauri build --target aarch64-unknown-linux-gnu
    log_success "Linux (ARM64) 构建完成"
}

# 构建所有平台
build_all() {
    log_info "开始构建所有平台..."

    # 构建前端
    build_frontend

    # 检测当前平台并构建
    case "$(uname -s)" in
        Linux*)
            log_info "检测到 Linux 系统"
            build_linux
            ;;
        Darwin*)
            log_info "检测到 macOS 系统"
            if [ "$(uname -m)" = "arm64" ]; then
                build_macos_arm
            else
                build_macos
            fi
            ;;
        MINGW*|CYGWIN*|MSYS*)
            log_info "检测到 Windows 系统"
            build_windows
            ;;
        *)
            log_error "不支持的操作系统"
            exit 1
            ;;
    esac

    log_success "所有构建完成！"
    show_output_info
}

# 显示构建输出信息
show_output_info() {
    echo ""
    echo "=========================================="
    echo "构建输出位置:"
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

# 主函数
main() {
    # 切换到项目根目录
    cd "$(dirname "$0")"

    # 解析参数
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
            log_error "未知参数: $1"
            show_help
            exit 1
            ;;
    esac
}

# 执行主函数
main "$@"
