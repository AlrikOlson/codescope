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

info()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
err()   { printf '\033[1;31mERR\033[0m %s\n' "$*" >&2; }

# --- Detect if we're running from a pipe (curl | bash) ---
if [ ! -f "$SCRIPT_DIR/Cargo.toml" ]; then
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

# --- Build ---
info "Building codescope-server (release mode)..."
cd "$SCRIPT_DIR"
cargo build --release

# --- Install ---
mkdir -p "$INSTALL_DIR"

cp "$SCRIPT_DIR/target/release/codescope-server" "$INSTALL_DIR/codescope-server"
ok "Installed codescope-server -> $INSTALL_DIR/codescope-server"

# Install codescope-init helper if it exists alongside setup.sh
if [ -f "$SCRIPT_DIR/codescope-init" ]; then
    cp "$SCRIPT_DIR/codescope-init" "$INSTALL_DIR/codescope-init"
    chmod +x "$INSTALL_DIR/codescope-init"
    ok "Installed codescope-init -> $INSTALL_DIR/codescope-init"
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
echo "  To add CodeScope to any project:"
echo ""
echo "    cd /path/to/your/project"
echo "    codescope-init"
echo ""
echo "  Then open Claude Code in that directory — cs_find, cs_grep, etc. are available."
echo ""
if ! command -v codescope-server >/dev/null 2>&1; then
    echo "  NOTE: Restart your shell (or run 'source ~/.bashrc') to pick up the new PATH."
    echo ""
fi
