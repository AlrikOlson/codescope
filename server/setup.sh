#!/usr/bin/env bash
set -euo pipefail

# CodeScope — Install Script
# Downloads a pre-built binary (~5MB). No compilation needed.
#
# Usage:
#   curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash
#   curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash -s -- /path/to/project
#   curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash -s -- --edge

if [[ "$(uname -s)" == MINGW* || "$(uname -s)" == MSYS* || "$(uname -s)" == CYGWIN* ]]; then
    INSTALL_DIR="${LOCALAPPDATA:-$APPDATA}/codescope/bin"
else
    INSTALL_DIR="$HOME/.local/bin"
fi
REPO="AlrikOlson/codescope"
BRANCH="master"

# --- Output helpers ---
info()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m OK\033[0m %s\n' "$*"; }
err()   { printf '\033[1;31mERR\033[0m %s\n' "$*" >&2; }

usage() {
    cat <<'EOF'
CodeScope — Install Script

Usage:
  bash setup.sh [options] [/path/to/project]

Options:
  --edge            Install bleeding-edge build from latest master commit
  --dev             Install development build from latest dev branch commit
  --from-source     Force compilation from source instead of downloading a binary
  --help, -h        Show this help

All pre-built binaries include semantic search. Enable it at runtime
with the --semantic flag when starting codescope-server.

Examples:
  # Standard install (downloads pre-built binary, ~10 seconds)
  bash setup.sh

  # Install and set up a project in one step
  bash setup.sh /path/to/my/project

  # Install bleeding-edge build (latest master commit)
  bash setup.sh --edge

  # Install development build (latest dev branch commit)
  bash setup.sh --dev
EOF
}

# --- Parse flags ---
FROM_SOURCE=0
EDGE=0
DEV=0
PROJECT_PATH=""
for arg in "$@"; do
    case "$arg" in
        --with-semantic) ;; # accepted for backwards compatibility, now a no-op
        --from-source)   FROM_SOURCE=1 ;;
        --edge)          EDGE=1 ;;
        --dev)           DEV=1 ;;
        --help|-h)       usage; exit 0 ;;
        --)              ;; # ignore -- separator from curl pipe
        -*)              err "Unknown flag: $arg"; usage; exit 1 ;;
        *)               PROJECT_PATH="$arg" ;;
    esac
done

# --- Platform detection ---
detect_platform() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os" in
        Linux)                os="linux" ;;
        Darwin)               os="macos" ;;
        MINGW*|MSYS*|CYGWIN*) os="windows" ;;
        *)
            err "Unsupported OS: $os (CodeScope supports Linux, macOS, and Windows)"
            return 1
            ;;
    esac
    case "$arch" in
        x86_64|amd64)   arch="x86_64" ;;
        aarch64|arm64)  arch="aarch64" ;;
        *)
            err "Unsupported architecture: $arch"
            return 1
            ;;
    esac
    echo "${os}-${arch}"
}

# --- Download pre-built binary ---
download_binary() {
    local platform="$1"
    local api_url channel
    if [ "$DEV" = "1" ]; then
        api_url="https://api.github.com/repos/$REPO/releases/tags/dev"
        channel="dev"
    elif [ "$EDGE" = "1" ]; then
        api_url="https://api.github.com/repos/$REPO/releases/tags/edge"
        channel="edge"
    else
        api_url="https://api.github.com/repos/$REPO/releases/latest"
        channel="stable"
    fi
    info "Checking for latest $channel release..."
    local release_json
    if ! release_json="$(curl -fsSL --connect-timeout 10 --max-time 30 "$api_url" 2>/dev/null)"; then
        err "Could not reach GitHub. Check your internet connection."
        return 1
    fi

    local tag
    tag="$(echo "$release_json" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')"
    if [ -z "$tag" ]; then
        err "Could not find a release to download"
        return 1
    fi

    # Determine archive format and binary name based on platform
    local os_part="${platform%%-*}"
    local archive binary_name
    if [ "$os_part" = "windows" ]; then
        archive="codescope-server-${platform}.zip"
        binary_name="codescope-server.exe"
    else
        archive="codescope-server-${platform}.tar.gz"
        binary_name="codescope-server"
    fi
    local url="https://github.com/$REPO/releases/download/$tag/$archive"

    info "Downloading CodeScope $tag ($platform)..."
    local tmpdir
    tmpdir="$(mktemp -d)"

    if ! curl -fsSL --connect-timeout 10 --max-time 120 -o "$tmpdir/$archive" "$url"; then
        rm -rf "$tmpdir"
        err "Download failed. Your platform ($platform) may not have a pre-built binary."
        return 1
    fi

    # Extract
    mkdir -p "$tmpdir/extracted"
    if [ "$os_part" = "windows" ]; then
        if ! unzip -q "$tmpdir/$archive" -d "$tmpdir/extracted" 2>/dev/null; then
            rm -rf "$tmpdir"
            err "Failed to extract downloaded archive"
            return 1
        fi
    else
        if ! tar xzf "$tmpdir/$archive" -C "$tmpdir/extracted" 2>/dev/null; then
            rm -rf "$tmpdir"
            err "Failed to extract downloaded archive"
            return 1
        fi
    fi

    # Install binary
    mkdir -p "$INSTALL_DIR"
    local binary
    binary="$(find "$tmpdir/extracted" -name "$binary_name" -type f | head -1)"
    if [ -z "$binary" ]; then
        rm -rf "$tmpdir"
        err "Archive did not contain $binary_name"
        return 1
    fi
    # Remove old binary first — avoids "Text file busy" if it's running (e.g., as MCP server)
    rm -f "$INSTALL_DIR/$binary_name"
    cp "$binary" "$INSTALL_DIR/$binary_name"
    if [ "$os_part" != "windows" ]; then
        chmod +x "$INSTALL_DIR/$binary_name"
    fi

    # macOS: clear Gatekeeper quarantine
    if [ "$(uname -s)" = "Darwin" ]; then
        xattr -d com.apple.quarantine "$INSTALL_DIR/$binary_name" 2>/dev/null || true
    fi

    ok "Installed $binary_name -> $INSTALL_DIR/"

    # Install helper scripts from tarball if present
    for script in codescope-init codescope-web; do
        local found
        found="$(find "$tmpdir/extracted" -name "$script" -type f | head -1)"
        if [ -n "$found" ]; then
            cp "$found" "$INSTALL_DIR/$script"
            chmod +x "$INSTALL_DIR/$script"
            if [ "$(uname -s)" = "Darwin" ]; then
                xattr -d com.apple.quarantine "$INSTALL_DIR/$script" 2>/dev/null || true
            fi
        fi
    done

    rm -rf "$tmpdir"
    return 0
}

# --- Download helper scripts separately (fallback) ---
install_helper_scripts() {
    for script in codescope-init codescope-web; do
        if [ ! -f "$INSTALL_DIR/$script" ]; then
            local url="https://raw.githubusercontent.com/$REPO/$BRANCH/server/$script"
            if curl -fsSL --connect-timeout 10 --max-time 30 -o "$INSTALL_DIR/$script" "$url" 2>/dev/null; then
                chmod +x "$INSTALL_DIR/$script"
            else
                err "Could not download $script (non-critical)"
            fi
        fi
    done
}

# --- Compile from source (fallback) ---
install_from_source() {
    local script_dir="$1"

    # If we don't have the source, clone it
    if [ -z "$script_dir" ] || ! grep -q 'name = "codescope-server"' "$script_dir/Cargo.toml" 2>/dev/null; then
        info "Downloading source code..."
        local tmpdir
        tmpdir="$(mktemp -d)"
        SOURCE_CLEANUP="$tmpdir"
        git clone --depth 1 "https://github.com/$REPO.git" "$tmpdir/codescope"
        script_dir="$tmpdir/codescope/server"
    fi

    # Check for Rust toolchain
    if command -v cargo >/dev/null 2>&1; then
        info "Rust toolchain found: $(rustc --version)"
    else
        info "Installing Rust (needed for compilation)..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
        ok "Rust installed: $(rustc --version)"
    fi

    # Build
    cd "$script_dir"
    info "Compiling from source (this takes a few minutes)..."
    cargo build --release --features semantic

    # Install binary
    mkdir -p "$INSTALL_DIR"
    rm -f "$INSTALL_DIR/codescope-server"
    cp "$script_dir/target/release/codescope-server" "$INSTALL_DIR/codescope-server"
    ok "Installed codescope-server -> $INSTALL_DIR/"

    # Install helper scripts from source tree
    local repo_root
    repo_root="$(cd "$script_dir/.." && pwd)"
    for script in codescope-init codescope-web; do
        if [ -f "$script_dir/$script" ]; then
            cp "$script_dir/$script" "$INSTALL_DIR/$script"
            chmod +x "$INSTALL_DIR/$script"
        fi
    done

    # Build web UI if npm is available
    if command -v npm >/dev/null 2>&1; then
        info "Building web UI..."
        cd "$repo_root"
        npm install --no-audit --no-fund 2>&1 | tail -1
        npm run build 2>&1 | tail -1
        local dist_install
        if [[ "$(uname -s)" == MINGW* || "$(uname -s)" == MSYS* || "$(uname -s)" == CYGWIN* ]]; then
            dist_install="${LOCALAPPDATA:-$APPDATA}/codescope/dist"
        else
            dist_install="$HOME/.local/share/codescope/dist"
        fi
        if [ -d "$repo_root/dist" ]; then
            mkdir -p "$dist_install"
            rm -rf "$dist_install"
            cp -r "$repo_root/dist" "$dist_install"
            ok "Installed web UI -> $dist_install"
        fi
    fi

    # Cleanup temp source
    if [ -n "${SOURCE_CLEANUP:-}" ]; then
        rm -rf "$SOURCE_CLEANUP"
    fi
}

# --- PATH setup ---
ensure_path() {
    # Windows Git Bash / MSYS2: use setx to persist PATH
    if [[ "$(uname -s)" == MINGW* || "$(uname -s)" == MSYS* ]]; then
        if ! echo "$PATH" | tr ':' '\n' | grep -qi "codescope"; then
            local winpath
            winpath="$(cygpath -w "$INSTALL_DIR")"
            setx PATH "%PATH%;$winpath" 2>/dev/null || true
            export PATH="$INSTALL_DIR:$PATH"
            info "Added $winpath to Windows PATH (takes effect in new terminals)"
        fi
        return 0
    fi

    if echo "$PATH" | tr ':' '\n' | grep -q "$HOME/.local/bin"; then
        return 0
    fi

    add_to_rc() {
        local rc_file="$1"
        if [ -f "$rc_file" ] && grep -q '\.local/bin' "$rc_file" 2>/dev/null; then
            return 0
        fi
        if [ -f "$rc_file" ] || [ "$rc_file" = "$HOME/.bashrc" ]; then
            printf '\n# CodeScope\nexport PATH="$HOME/.local/bin:$PATH"\n' >> "$rc_file"
        fi
    }

    if [ -f "$HOME/.zshrc" ]; then
        add_to_rc "$HOME/.zshrc"
    fi
    add_to_rc "$HOME/.bashrc"

    # Fish shell support
    local fish_config="$HOME/.config/fish/config.fish"
    if [ -f "$fish_config" ]; then
        if ! grep -q '\.local/bin' "$fish_config" 2>/dev/null; then
            printf '\n# CodeScope\nfish_add_path -g "$HOME/.local/bin"\n' >> "$fish_config"
        fi
    fi

    export PATH="$INSTALL_DIR:$PATH"
}

# ============================================================
# Main
# ============================================================

SOURCE_CLEANUP=""
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd 2>/dev/null || echo "")"

echo ""
info "Installing CodeScope..."
echo ""

# Decide install method
if [ "$FROM_SOURCE" = "1" ]; then
    install_from_source "$SCRIPT_DIR"
else
    platform="$(detect_platform 2>/dev/null)" || platform=""
    if [ -n "$platform" ]; then
        if ! download_binary "$platform"; then
            info "Binary download unavailable — compiling from source instead..."
            install_from_source "$SCRIPT_DIR"
        fi
    else
        info "Could not detect platform — compiling from source instead..."
        install_from_source "$SCRIPT_DIR"
    fi
fi

# Make sure helper scripts are installed
install_helper_scripts

# PATH setup
ensure_path

# Auto-init project if path given
if [ -n "$PROJECT_PATH" ]; then
    echo ""
    info "Setting up CodeScope in $PROJECT_PATH..."
    "$INSTALL_DIR/codescope-server" init "$PROJECT_PATH"
fi

# --- Done ---
echo ""
ok "CodeScope installed!"
echo ""
echo "  Next steps:"
echo ""
echo "    1. Set up a project:"
echo "       cd /path/to/your/project"
echo "       codescope-server init"
echo ""
echo "    2. Open Claude Code in that directory — CodeScope is ready to use."
echo ""
if [ "$DEV" = "1" ]; then
    ok "Dev channel — built from latest dev branch commit"
    echo ""
elif [ "$EDGE" = "1" ]; then
    ok "Edge channel — built from latest master commit"
    echo ""
fi
echo "  Semantic search included. Enable with: codescope-server --mcp --semantic --root ."
echo ""
if [ "$EDGE" = "0" ] && [ "$DEV" = "0" ]; then
    echo "  Optional: re-run with --edge or --dev for pre-release builds"
    echo ""
fi
if ! command -v codescope-server >/dev/null 2>&1; then
    echo "  NOTE: Restart your terminal to pick up the new PATH."
    echo ""
fi
