# Changelog

All notable changes to CodeScope will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [0.5.0] - 2026-02-20

### Added
- Windows support: `server/src/main.rs` gains `home_dir`/`config_dir`/`data_dir` helpers resolving to `%APPDATA%`/`%LOCALAPPDATA%` on Windows
- `server/setup.sh` installs to `%LOCALAPPDATA%/codescope/bin` on Windows, downloads `.zip` archives, and updates PATH via `setx`
- `server/codescope-web` uses Windows-aware dist dir and `explorer.exe` for browser open
- CI builds and publishes `windows-x86_64` and `windows-aarch64` binaries (MSVC targets)
- AI-powered releases now auto-update `CHANGELOG.md` with the generated entry

### Fixed
- Replace `grep -q` with `grep >/dev/null` in CI archive verify to avoid SIGPIPE errors
- Free build intermediates before packaging to prevent disk exhaustion and tar write errors on CI runners
- Add `shell: bash` to CI steps requiring bash behavior on Windows runners

## [Unreleased]

### Fixed

- Free disk space before packaging to prevent tar write errors during CI releases
- Add `shell: bash` to CI build steps for Windows runners

## [0.4.0] - 2026-02-18

### Features

- **CMake dependency scanning** — parse `CMakeLists.txt` for project dependencies
- **C# dependency scanning** — parse `.Build.cs` and `.csproj` files
- **Unified selection model** — module toggle, dependency selection, and context builder share consistent selection logic across all views
- **Graph performance** — instanced mesh rendering with LOD geometry and spatial-hash simulation for 1000+ node graphs
- **Version sync** — all version files (`Cargo.toml`, `package.json`, `package-lock.json`) updated atomically during release
- **Cross-platform Windows support** — setup script, Rust backend, and CI pipeline work on Windows (Git Bash / MSYS2)
- **Auto-changelog** — AI-powered releases automatically update CHANGELOG.md

## [0.3.2] - 2026-02-18

### Changed

- Documentation restructure and Cargo.lock version sync

## [0.3.1] - 2026-02-18

### Fixed

- Restore treemap view that was broken in a previous release
- Fix layout space distribution in squarified treemap algorithm

## [0.3.0] - 2026-02-18

Initial public release.

### Features

- **MCP server** with 19 tools for Claude Code integration: search, read, navigate, analyze
- **Semantic search** via all-MiniLM-L6-v2 BERT embeddings with non-blocking indexing and CUDA GPU acceleration
- **Web UI** with file browser, full-text search, treemap visualization, and 3D dependency graph
- **Multi-repo support** — index and search across multiple repositories simultaneously
- **Impact analysis** — trace import dependency chains to answer "what breaks if I change this?"
- **Token budget allocation** — water-fill algorithm to fit file contents into LLM context windows
- **Structural stubs** — extract function/class signatures without bodies for compact code summaries
- **FZF v2 fuzzy matching** — Smith-Waterman scoring with bitmask pre-filtering
- **Cross-repo import resolution** — trace imports that cross repository boundaries
- **18+ language support** — stub extraction and import tracing for Rust, TypeScript, Python, Go, C/C++, C#, Java, Kotlin, Swift, Ruby, PHP, Lua, Zig, PowerShell, and more
- **Dependency scanning** — parse Cargo.toml, package.json, go.mod, CMakeLists.txt, .Build.cs
- **CLI subcommands** — `init` for auto-setup, `doctor` for diagnostics
- **Configuration** — `.codescope.toml` for per-project scan customization
