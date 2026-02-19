#!/usr/bin/env bash
set -euo pipefail

# CodeScope MCP Server — Setup Script
# Builds and installs codescope-server + codescope-init to ~/.local/bin
#
# Usage:
#   git clone https://github.com/AlrikOlson/codescope.git
#   cd codescope/server
#   ./setup.sh
#
# Or directly:
#   curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/main/server/setup.sh | bash

INSTALL_DIR="$HOME/.local/bin"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"

# Parse flags
WITH_SEMANTIC=0
for arg in "$@"; do
    case "$arg" in
        --with-semantic) WITH_SEMANTIC=1 ;;
    esac
done

info()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
err()   { printf '\033[1;31mERR\033[0m %s\n' "$*" >&2; }

# --- Detect if we're in the codescope repo or running from pipe/arbitrary dir ---
if ! grep -q 'name = "codescope-server"' "$SCRIPT_DIR/Cargo.toml" 2>/dev/null; then
    info "Running from pipe — cloning codescope repo..."
    TMPDIR="$(mktemp -d)"
    git clone --depth 1 https://github.com/AlrikOlson/codescope.git "$TMPDIR/codescope"
    SCRIPT_DIR="$TMPDIR/codescope/server"
    CLEANUP_TMP=1
else
    CLEANUP_TMP=0
fi

cleanup() {
    if [ "$CLEANUP_TMP" = "1" ] && [ -n "${TMPDIR:-}" ]; then
        rm -rf "$TMPDIR"
    fi
}
trap cleanup EXIT

# --- Check for Rust toolchain ---
if command -v cargo >/dev/null 2>&1; then
    info "Rust toolchain found: $(rustc --version)"
else
    info "Rust not found — installing via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    ok "Rust installed: $(rustc --version)"
fi

# --- Build server ---
cd "$SCRIPT_DIR"
if [ "$WITH_SEMANTIC" = "1" ]; then
    info "Building codescope-server with semantic search (release mode)..."
    cargo build --release --features semantic
else
    info "Building codescope-server (release mode)..."
    cargo build --release
fi

# --- Install binaries ---
mkdir -p "$INSTALL_DIR"

cp "$SCRIPT_DIR/target/release/codescope-server" "$INSTALL_DIR/codescope-server"
ok "Installed codescope-server -> $INSTALL_DIR/codescope-server"

# Install helper scripts
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
for script in codescope-init codescope-web; do
    if [ -f "$SCRIPT_DIR/$script" ]; then
        cp "$SCRIPT_DIR/$script" "$INSTALL_DIR/$script"
        chmod +x "$INSTALL_DIR/$script"
        ok "Installed $script -> $INSTALL_DIR/$script"
    fi
done

# --- Build web UI ---
DIST_INSTALL="$HOME/.local/share/codescope/dist"
if command -v npm >/dev/null 2>&1; then
    info "Building web UI..."
    cd "$REPO_ROOT"
    npm install --no-audit --no-fund 2>&1 | tail -1
    npm run build 2>&1 | tail -1
    if [ -d "$REPO_ROOT/dist" ]; then
        mkdir -p "$DIST_INSTALL"
        rm -rf "$DIST_INSTALL"
        cp -r "$REPO_ROOT/dist" "$DIST_INSTALL"
        ok "Installed web UI -> $DIST_INSTALL"
    else
        err "Frontend build produced no dist/ directory — web UI not installed"
    fi
else
    info "npm not found — skipping web UI build (MCP server still works)"
    info "Install Node.js and re-run setup.sh to enable the web UI"
fi

# --- Ensure ~/.local/bin is in PATH ---
add_to_path() {
    local rc_file="$1"
    if [ -f "$rc_file" ] && grep -q '\.local/bin' "$rc_file" 2>/dev/null; then
        return 0
    fi
    if [ -f "$rc_file" ] || [ "$rc_file" = "$HOME/.bashrc" ]; then
        printf '\n# CodeScope\nexport PATH="$HOME/.local/bin:$PATH"\n' >> "$rc_file"
        info "Added ~/.local/bin to PATH in $(basename "$rc_file")"
    fi
}

if ! echo "$PATH" | tr ':' '\n' | grep -q "$HOME/.local/bin"; then
    # Detect which shell rc files to update
    if [ -f "$HOME/.zshrc" ]; then
        add_to_path "$HOME/.zshrc"
    fi
    add_to_path "$HOME/.bashrc"
    export PATH="$INSTALL_DIR:$PATH"
fi

# --- Done ---
echo ""
ok "Setup complete!"
echo ""
echo "  MCP server (for Claude Code):"
echo "    cd /path/to/your/project"
echo "    codescope-init"
echo ""
if [ "$WITH_SEMANTIC" = "1" ]; then
    ok "Semantic search enabled — ML model downloads on first use (~90MB)"
    echo ""
else
    echo "  Tip: re-run with --with-semantic to enable ML-powered semantic search"
    echo ""
fi
if [ -d "$DIST_INSTALL" ]; then
    echo "  Web UI:"
    echo "    codescope-web /path/to/your/project"
    echo ""
fi
if ! command -v codescope-server >/dev/null 2>&1; then
    echo "  NOTE: Restart your shell (or run 'source ~/.bashrc') to pick up the new PATH."
    echo ""
fi
