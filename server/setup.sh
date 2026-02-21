#!/usr/bin/env bash
set -euo pipefail

# CodeScope — Cross-Platform Install Script
# Works from: Linux, macOS, Git Bash, WSL
#
# Usage:
#   curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash
#   bash setup.sh --cuda
#   Windows: irm .../setup.ps1 | iex  (or: wsl bash -c "curl ... | bash")

REPO="AlrikOlson/codescope"
BRANCH="master"

# --- Detect environment ---
# Three-method WSL detection: /proc/version, env var, binfmt_misc interop files
IS_WSL=false
if grep -qi microsoft /proc/version 2>/dev/null; then
    IS_WSL=true
elif [ -n "${WSL_DISTRO_NAME:-}" ]; then
    IS_WSL=true
elif [ -f /proc/sys/fs/binfmt_misc/WSLInterop ] || [ -f /proc/sys/fs/binfmt_misc/WSLInterop-late ]; then
    IS_WSL=true
fi

# Git Bash / MSYS2 / Cygwin detection
IS_GITBASH=false
if [[ "$(uname -s)" == MINGW* || "$(uname -s)" == MSYS* || "$(uname -s)" == CYGWIN* ]]; then
    IS_GITBASH=true
fi

IS_WINDOWS=false
if [ "$IS_WSL" = "true" ] || [ "$IS_GITBASH" = "true" ]; then
    IS_WINDOWS=true
fi

# --- Resolve Windows install directory ---
resolve_win_install_dir() {
    local localappdata=""

    # Method 1: cmd.exe to get LOCALAPPDATA (works in WSL with interop enabled)
    localappdata="$(cmd.exe /c "echo %LOCALAPPDATA%" 2>/dev/null | tr -d '\r\n' || true)"
    # Check it actually expanded (not the literal %LOCALAPPDATA%)
    if [ -z "$localappdata" ] || [[ "$localappdata" == *"%"* ]]; then
        localappdata=""
    fi

    # Method 2: powershell.exe fallback
    if [ -z "$localappdata" ]; then
        localappdata="$(powershell.exe -NoProfile -Command '$env:LOCALAPPDATA' 2>/dev/null | tr -d '\r\n' || true)"
    fi

    # Method 3: wslvar (requires wslu, present on Ubuntu WSL by default)
    if [ -z "$localappdata" ] && command -v wslvar &>/dev/null; then
        localappdata="$(wslvar LOCALAPPDATA 2>/dev/null | tr -d '\r\n' || true)"
    fi

    # Method 4: construct from username + standard path
    if [ -z "$localappdata" ]; then
        local win_user=""
        win_user="$(cmd.exe /c "echo %USERNAME%" 2>/dev/null | tr -d '\r\n' || true)"
        if [ -z "$win_user" ] || [[ "$win_user" == *"%"* ]]; then
            # Last resort: parse /mnt/*/Users/ for a single user
            local mount_root
            mount_root="$(findmnt -n -o TARGET /dev/sdc 2>/dev/null || echo "/mnt/c")"
            [ -d "$mount_root" ] || mount_root="/mnt/c"
            local candidates
            candidates="$(ls "${mount_root}/Users/" 2>/dev/null | grep -vE '^(Public|Default|Default User|All Users|desktop\.ini)$' || true)"
            if [ "$(echo "$candidates" | wc -l)" -eq 1 ] && [ -n "$candidates" ]; then
                win_user="$candidates"
            fi
        fi
        if [ -n "$win_user" ]; then
            localappdata="C:\\Users\\${win_user}\\AppData\\Local"
        fi
    fi

    if [ -z "$localappdata" ]; then
        return 1
    fi

    # Convert Windows path to WSL/Unix path
    local linux_path
    linux_path="$(wslpath -u "$localappdata" 2>/dev/null || true)"
    if [ -z "$linux_path" ]; then
        # Manual conversion: C:\Users\foo -> /mnt/c/Users/foo
        linux_path="$(echo "$localappdata" | sed 's|\\|/|g' | sed 's|^\([A-Za-z]\):|/mnt/\1|' | sed 's|^/mnt/\(.\)|/mnt/\1|')"
        # Lowercase the drive letter (tr is portable, sed \L is GNU-only)
        local drive_letter
        drive_letter="$(echo "$linux_path" | cut -c6 | tr 'A-Z' 'a-z')"
        linux_path="/mnt/${drive_letter}${linux_path:6}"
    fi

    echo "${linux_path}/codescope/bin"
}

# Install directory
if [ "$IS_WSL" = "true" ]; then
    INSTALL_DIR="$(resolve_win_install_dir 2>/dev/null || true)"
    if [ -z "$INSTALL_DIR" ]; then
        # Cannot resolve Windows paths (interop disabled?) — fall back to WSL-local
        INSTALL_DIR="$HOME/.local/bin"
        IS_WSL=false  # treat as native Linux from here on
    fi
elif [ "$IS_GITBASH" = "true" ]; then
    INSTALL_DIR="${LOCALAPPDATA:-$APPDATA}/codescope/bin"
else
    INSTALL_DIR="$HOME/.local/bin"
fi

# --- Output helpers ---
# Respect NO_COLOR (https://no-color.org/) and detect TTY
if [ -n "${NO_COLOR:-}" ] || [ ! -t 1 ]; then
    info()  { printf '==> %s\n' "$*"; }
    ok()    { printf ' OK %s\n' "$*"; }
    warn()  { printf '  ! %s\n' "$*"; }
    err()   { printf 'ERR %s\n' "$*" >&2; }
else
    info()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
    ok()    { printf '\033[1;32m  ✓\033[0m %s\n' "$*"; }
    warn()  { printf '\033[1;33m  !\033[0m %s\n' "$*"; }
    err()   { printf '\033[1;31m  ✗\033[0m %s\n' "$*" >&2; }
fi

usage() {
    cat <<'EOF'
CodeScope — Install Script

Usage:
  bash setup.sh [options] [/path/to/project]

Options:
  --edge            Install bleeding-edge build from latest master commit
  --dev             Install development build from latest dev branch commit
  --from-source     Build from source (auto-detects CUDA for GPU acceleration)
  --cuda            Build from source with CUDA required (errors if CUDA not found)
  --help, -h        Show this help

All pre-built binaries include semantic search (CPU), enabled by default.
Use --from-source to build locally with CUDA GPU acceleration (auto-detected).
Use --cuda to force CUDA (implies --from-source, errors if CUDA not found).

Examples:
  # Standard install (downloads pre-built binary, ~10 seconds)
  bash setup.sh

  # Install and set up a project in one step
  bash setup.sh /path/to/my/project

  # Build from source with CUDA GPU acceleration
  bash setup.sh --cuda

  # Install bleeding-edge build (latest master commit)
  bash setup.sh --edge
EOF
}

# --- Parse flags ---
FROM_SOURCE=0
FORCE_CUDA=0
EDGE=0
DEV=0
PROJECT_PATH=""
for arg in "$@"; do
    case "$arg" in
        --with-semantic) ;; # accepted for backwards compatibility, now a no-op
        --from-source)   FROM_SOURCE=1 ;;
        --cuda)          FORCE_CUDA=1; FROM_SOURCE=1 ;;
        --edge)          EDGE=1 ;;
        --dev)           DEV=1 ;;
        --help|-h)       usage; exit 0 ;;
        --)              ;; # ignore -- separator from curl pipe
        -*)              err "Unknown flag: $arg"; usage; exit 1 ;;
        *)               PROJECT_PATH="$arg" ;;
    esac
done

# --- CUDA detection helper ---
has_cuda() {
    command -v nvcc &>/dev/null \
        || [ -d /usr/local/cuda ] \
        || [ -f /usr/lib/x86_64-linux-gnu/libcuda.so ] \
        || command -v nvidia-smi &>/dev/null
}

# --- Platform detection ---
detect_platform() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"

    # WSL: uname says Linux, but we're installing a Windows binary
    if [ "$IS_WSL" = "true" ]; then
        os="windows"
    elif [ "$IS_GITBASH" = "true" ]; then
        os="windows"
    else
        case "$os" in
            Linux)  os="linux" ;;
            Darwin) os="macos" ;;
            *)
                err "Unsupported OS: $os (CodeScope supports Linux, macOS, and Windows)"
                return 1
                ;;
        esac
    fi

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

# --- Extract archive (with unzip fallback for .zip on minimal WSL distros) ---
extract_archive() {
    local archive="$1" destdir="$2" os_part="$3"

    mkdir -p "$destdir"
    if [ "$os_part" = "windows" ]; then
        # .zip extraction — unzip may not be installed on minimal WSL distros
        if command -v unzip &>/dev/null; then
            unzip -oq "$archive" -d "$destdir"
        elif command -v python3 &>/dev/null; then
            python3 -c "import zipfile; zipfile.ZipFile('$archive').extractall('$destdir')"
        elif command -v busybox &>/dev/null && busybox unzip --help &>/dev/null 2>&1; then
            busybox unzip "$archive" -d "$destdir"
        else
            err "No zip extraction tool found. Install unzip:"
            err "  sudo apt install unzip"
            return 1
        fi
    else
        if ! tar xzf "$archive" -C "$destdir" 2>/dev/null; then
            err "Failed to extract downloaded archive"
            return 1
        fi
    fi
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
        archive="codescope-${platform}.zip"
        binary_name="codescope.exe"
    else
        archive="codescope-${platform}.tar.gz"
        binary_name="codescope"
    fi
    info "Downloading CodeScope $tag ($platform)..."
    local tmpdir
    tmpdir="$(mktemp -d)"

    local url="https://github.com/$REPO/releases/download/$tag/$archive"
    local curl_progress="-s"
    if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then curl_progress="--progress-bar"; fi
    if ! curl -fL --connect-timeout 10 --max-time 120 $curl_progress -o "$tmpdir/$archive" "$url"; then
        rm -rf "$tmpdir"
        err "Download failed. Your platform ($platform) may not have a pre-built binary."
        return 1
    fi

    # Extract
    if ! extract_archive "$tmpdir/$archive" "$tmpdir/extracted" "$os_part"; then
        rm -rf "$tmpdir"
        return 1
    fi

    # Install binary
    if ! mkdir -p "$INSTALL_DIR" 2>/dev/null; then
        rm -rf "$tmpdir"
        err "Cannot create directory: $INSTALL_DIR"
        err "Check that you have write access to this location."
        return 1
    fi

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

    # Verify binary wasn't quarantined (Windows Defender can empty the file)
    if [ ! -s "$INSTALL_DIR/$binary_name" ]; then
        rm -rf "$tmpdir"
        err "Binary appears empty — Windows Defender may have quarantined it."
        err "Check Windows Security > Virus & threat protection > Protection history"
        return 1
    fi

    # macOS: clear Gatekeeper quarantine
    if [ "$(uname -s)" = "Darwin" ]; then
        xattr -d com.apple.quarantine "$INSTALL_DIR/$binary_name" 2>/dev/null || true
    fi

    ok "Installed $binary_name -> $INSTALL_DIR/"

    rm -rf "$tmpdir"
    return 0
}

# --- Compile from source (fallback) ---
install_from_source() {
    local script_dir="$1"

    # If we don't have the source, clone it
    if [ -z "$script_dir" ] || ! grep -q 'name = "codescope"' "$script_dir/Cargo.toml" 2>/dev/null; then
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

    # Build — auto-detect CUDA for GPU acceleration
    cd "$script_dir"
    FEATURES="semantic"
    local cuda_found=false
    if command -v nvcc &>/dev/null || [ -f /usr/local/cuda/bin/nvcc ]; then
        cuda_found=true
    fi

    if [ "$FORCE_CUDA" = "1" ] && [ "$cuda_found" = "false" ]; then
        err "CUDA not found. Install the CUDA toolkit (nvcc must be in PATH or /usr/local/cuda/bin/)."
        err "  Ubuntu/Debian: sudo apt install nvidia-cuda-toolkit"
        err "  Download:      https://developer.nvidia.com/cuda-downloads"
        return 1
    fi

    if [ "$cuda_found" = "true" ]; then
        export PATH="/usr/local/cuda/bin:$PATH"
        FEATURES="semantic,cuda"
        local cuda_ver cuda_path
        cuda_ver="$(nvcc --version 2>/dev/null | grep -oP 'release \K[0-9]+\.[0-9]+' || echo "?")"
        cuda_path="$(command -v nvcc 2>/dev/null || echo "/usr/local/cuda/bin/nvcc")"
        info "CUDA $cuda_ver detected ($cuda_path) — building with GPU acceleration"
    fi
    info "Compiling from source (this takes a few minutes)..."
    cargo build --release --features "$FEATURES"

    # Install binary
    # Note: from-source in WSL builds a Linux binary — install to WSL, not Windows
    local src_install_dir="$INSTALL_DIR"
    if [ "$IS_WSL" = "true" ]; then
        src_install_dir="$HOME/.local/bin"
        warn "Building from source in WSL produces a Linux binary (WSL-only)."
        info "Installing to $src_install_dir (use from WSL terminal, not PowerShell)"
    fi
    mkdir -p "$src_install_dir"
    rm -f "$src_install_dir/codescope"
    cp "$script_dir/target/release/codescope" "$src_install_dir/codescope"
    ok "Installed codescope -> $src_install_dir/"

    local repo_root
    repo_root="$(cd "$script_dir/.." && pwd)"

    # Build web UI if npm is available
    if command -v npm >/dev/null 2>&1; then
        info "Building web UI..."
        cd "$repo_root"
        npm install --no-audit --no-fund 2>&1 | tail -1
        npm run build 2>&1 | tail -1
        local dist_install
        if [ "$IS_WINDOWS" = "true" ]; then
            dist_install="$(dirname "$INSTALL_DIR")/dist"
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

# --- PATH check (never modifies shell config) ---
check_path() {
    # Add to current session so post-install steps work
    export PATH="$INSTALL_DIR:$PATH"

    # Check if install dir is on the user's persistent PATH
    local on_path=false
    if [ "$IS_WINDOWS" = "true" ]; then
        if echo "$PATH" | tr ':' '\n' | grep -qi "codescope"; then on_path=true; fi
    else
        if echo "$PATH" | tr ':' '\n' | grep -q "$INSTALL_DIR"; then on_path=true; fi
    fi

    if [ "$on_path" = "false" ]; then
        PATH_HINT="$INSTALL_DIR"
    else
        PATH_HINT=""
    fi
}

# ============================================================
# Main
# ============================================================

SOURCE_CLEANUP=""
SCRIPT_DIR="$(cd "$(dirname -- "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd 2>/dev/null || echo "")"

echo ""
if [ "$IS_WSL" = "true" ] && [ "$FROM_SOURCE" = "0" ]; then
    info "Installing CodeScope for Windows (detected WSL)..."
elif [ "$IS_GITBASH" = "true" ]; then
    info "Installing CodeScope for Windows..."
else
    info "Installing CodeScope..."
fi
echo ""

# Decide install method
USED_BINARY=0
if [ "$FROM_SOURCE" = "1" ]; then
    install_from_source "$SCRIPT_DIR"
else
    platform="$(detect_platform 2>/dev/null)" || platform=""
    if [ -n "$platform" ]; then
        if download_binary "$platform"; then
            USED_BINARY=1
        else
            info "Binary download unavailable — compiling from source instead..."
            install_from_source "$SCRIPT_DIR"
        fi
    else
        info "Could not detect platform — compiling from source instead..."
        install_from_source "$SCRIPT_DIR"
    fi
fi

# PATH check
check_path

# Auto-init project if path given
if [ -n "$PROJECT_PATH" ]; then
    echo ""
    info "Setting up CodeScope in $PROJECT_PATH..."
    "$INSTALL_DIR/codescope" init "$PROJECT_PATH"
fi

# --- Done ---
echo ""
ok "CodeScope installed!"

# Show version if detectable
if command -v codescope >/dev/null 2>&1; then
    echo "  $(codescope --version 2>/dev/null || true)"
fi

# GPU hint — if binary install and CUDA is available, suggest rebuilding
if [ "$USED_BINARY" = "1" ] && has_cuda; then
    echo ""
    warn "GPU detected — for CUDA-accelerated semantic search, rebuild from source:"
    echo "    bash setup.sh --cuda"
fi

echo ""
echo "  Get started:"
echo "    codescope init          Set up your project"
echo "    codescope doctor        Verify everything works"
echo ""

if [ -n "$PATH_HINT" ]; then
    warn "Add to your PATH to use codescope from anywhere:"
    if [ "$IS_WINDOWS" = "true" ]; then
        winpath="$(cygpath -w "$PATH_HINT" 2>/dev/null || wslpath -w "$PATH_HINT" 2>/dev/null || true)"
        if [ -z "$winpath" ]; then
            # Manual: /mnt/c/Users/foo -> C:\Users\foo
            dl="$(echo "$PATH_HINT" | cut -c6 | tr 'a-z' 'A-Z')"
            winpath="${dl}:\\$(echo "${PATH_HINT:7}" | sed 's|/|\\|g')"
        fi
        echo "    [Environment]::SetEnvironmentVariable('Path', \$env:Path + ';$winpath', 'User')"
    else
        echo "    export PATH=\"$PATH_HINT:\$PATH\""
        echo ""
        echo "    Add that line to your shell config (~/.bashrc, ~/.zshrc, etc.)"
    fi
    echo ""
fi
