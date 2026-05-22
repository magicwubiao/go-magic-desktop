#!/bin/bash
#==============================================================================
# Go Magic Desktop - 进程分离模式构建脚本
#
# 特点:
# - 自动构建 go-magic 后端
# - 打包前端资源
# - 生成安装包
#==============================================================================

set -e

# 颜色
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'

log() { echo -e "${BLUE}[INFO]${NC} $*"; }
succ() { echo -e "${GREEN}[OK]${NC} $*"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
err() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

# 检测操作系统
detect_os() {
    case "$(uname -s)" in
        Darwin*)  echo "macos" ;;
        Linux*)   echo "linux" ;;
        MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
        *)        echo "unknown" ;;
    esac
}

# 获取后端二进制扩展名
get_backend_ext() {
    case "$(detect_os)" in
        windows) echo ".exe" ;;
        *)       echo "" ;;
    esac
}

# 目录
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"
TAURI_DIR="$PROJECT_ROOT/src-tauri"
GOMAGIC_DIR="$PROJECT_ROOT/../go-magic"
RESOURCES_DIR="$TAURI_DIR/resources"

#==============================================================================
# 步骤 1: 检查环境
#==============================================================================
check_environment() {
    log "检查构建环境..."
    log "操作系统: $(detect_os)"

    command -v rustc >/dev/null || err "Rust 未安装"
    command -v node >/dev/null || err "Node.js 未安装"
    command -v go >/dev/null || err "Go 未安装"

    # 检查 Tauri CLI
    if ! command -v tauri >/dev/null; then
        log "安装 Tauri CLI..."
        npm install -g @tauri-apps/cli
    fi

    succ "环境检查完成"
}

#==============================================================================
# 步骤 2: 构建前端
#==============================================================================
build_frontend() {
    log "构建前端..."

    # 确定前端源码位置和输出位置
    local src_dir=""
    local output_dir=""

    if [ -d "$GOMAGIC_DIR/web" ]; then
        src_dir="$GOMAGIC_DIR/web"
        output_dir="$GOMAGIC_DIR/web/dist"
    elif [ -d "$GOMAGIC_DIR/internal/server" ]; then
        src_dir="$GOMAGIC_DIR/internal/server"
        output_dir="$GOMAGIC_DIR/internal/server/dist"
    else
        warn "go-magic 源码不存在，跳过前端构建"
        if [ ! -d "$PROJECT_ROOT/web-dist" ]; then
            err "web-dist 目录也不存在，无法继续"
        fi
        warn "使用现有的 web-dist"
        return 0
    fi

    log "前端源码: $src_dir"
    log "输出目录: $output_dir"

    cd "$src_dir"
    npm install
    npm run build

    # 复制到 web-dist
    if [ -d "$output_dir" ]; then
        rm -rf "$PROJECT_ROOT/web-dist"
        cp -r "$output_dir" "$PROJECT_ROOT/web-dist"
        succ "前端构建完成 -> $PROJECT_ROOT/web-dist"
    else
        err "前端构建失败：输出目录不存在"
    fi

    cd "$PROJECT_ROOT"
}

#==============================================================================
# 步骤 3: 构建后端
#==============================================================================
build_backend() {
    log "构建 go-magic 后端..."

    if [ ! -d "$GOMAGIC_DIR/cmd/magic" ]; then
        err "go-magic 源码目录不存在: $GOMAGIC_DIR/cmd/magic"
    fi

    # 确保资源目录存在
    mkdir -p "$RESOURCES_DIR"

    local ext=$(get_backend_ext)
    local output_file="$RESOURCES_DIR/go-magic$ext"

    cd "$GOMAGIC_DIR"

    log "编译 go-magic 到: $output_file"

    # 静态编译
    CGO_ENABLED=0 go build -ldflags="-s -w" \
        -o "$output_file" \
        ./cmd/magic

    if [ -f "$output_file" ]; then
        succ "后端构建完成"
        ls -lh "$output_file"
    else
        err "后端构建失败"
    fi

    cd "$PROJECT_ROOT"
}

#==============================================================================
# 步骤 4: 打包 Tauri 应用
#==============================================================================
build_tauri() {
    log "打包 Tauri 应用..."

    # 确保 web-dist 存在
    if [ ! -d "$PROJECT_ROOT/web-dist" ]; then
        err "web-dist 目录不存在，请先构建前端"
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

    log "构建目标: ${target:-default}"

    if [ -n "$target" ]; then
        tauri build $target
    else
        tauri build
    fi

    cd "$PROJECT_ROOT"
    succ "Tauri 应用打包完成"
}

#==============================================================================
# 步骤 5: 显示结果
#==============================================================================
show_result() {
    log "构建输出:"

    local bundle_dir="$TAURI_DIR/target/release/bundle"

    echo ""
    echo "============================================"
    echo " 安装包位置"
    echo "============================================"

    find "$bundle_dir" -type f \( -name "*.exe" -o -name "*.msi" -o -name "*.dmg" -o -name "*.AppImage" -o -name "*.deb" \) 2>/dev/null | while read -r f; do
        echo "  $f"
        ls -lh "$f" 2>/dev/null | awk '{print "    Size: " $5}'
    done

    echo ""
    echo "============================================"
    echo " 资源文件"
    echo "============================================"
    ls -lh "$RESOURCES_DIR/" 2>/dev/null || true

    echo ""
}

#==============================================================================
# 主函数
#==============================================================================
main() {
    log "Go Magic Desktop 构建脚本"
    log "模式: 进程分离 (Tauri + go-magic)"
    echo ""

    # 解析参数
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
            log "清理..."
            rm -rf "$PROJECT_ROOT/web-dist"
            rm -rf "$RESOURCES_DIR/go-magic"*
            rm -rf "$TAURI_DIR/target"
            succ "清理完成"
            ;;
        help|--help|-h)
            echo "用法: $0 [选项]"
            echo ""
            echo "选项:"
            echo "  all       完整构建 (默认)"
            echo "  frontend  仅构建前端"
            echo "  backend   仅构建后端"
            echo "  tauri     仅打包 Tauri"
            echo "  clean     清理构建产物"
            echo "  help      显示帮助"
            ;;
        *)
            err "未知参数: $1"
            ;;
    esac
}

main "$@"
