# CodeScope

[![CI](https://github.com/AlrikOlson/codescope/actions/workflows/integration.yml/badge.svg)](https://github.com/AlrikOlson/codescope/actions/workflows/integration.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A fast codebase indexer and search server. Works as an [MCP](https://modelcontextprotocol.io/) server for Claude Code (and other MCP clients) or as a standalone HTTP server with a rich web UI.

Built in Rust. Indexes 200K+ files in under 2 seconds. Understands module structure, import graphs, and file dependencies out of the box.

## Why CodeScope?

- **Structural awareness** -- not just text search. CodeScope extracts function/class signatures, traces import graphs, and detects module boundaries across 18+ languages.
- **Token-budget reads** -- feed an LLM exactly what fits in its context window. The `cs_read_context` tool uses a water-fill algorithm to allocate tokens across files by importance.
- **Impact analysis** -- answer "what breaks if I change this file?" by tracing the full import dependency chain, even across repository boundaries.
- **One command to start** -- `codescope-server init` auto-detects your project type, generates config, and you're ready to go.

## Quick Start

```bash
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash
```

Or clone and build manually:

```bash
git clone https://github.com/AlrikOlson/codescope.git
cd codescope/server
./setup.sh
```

This installs the Rust toolchain (if needed), builds the server, and if Node.js is available, builds the web UI too. Everything goes into `~/.local/bin/`.

Then, in any project you want to index:

```bash
# Auto-detect project type and generate .mcp.json + .codescope.toml
codescope-server init

# Web UI (standalone browser)
codescope-web /path/to/your/project
```

`codescope-server init` detects your project type and generates smart defaults. Claude Code picks up the `.mcp.json` automatically. `codescope-web` launches the browser UI at `http://localhost:8432`.

## Multi-Repo Support

Index multiple repositories simultaneously:

```bash
# Named repos via CLI
codescope-server --mcp --repo engine=/path/to/engine --repo game=/path/to/game

# Via config file
codescope-server --mcp --config ~/.codescope/repos.toml

# Single repo (unchanged, backwards compatible)
codescope-server --mcp --root /path/to/project
```

Config file format (`repos.toml`):

```toml
[repos.backend]
root = "/home/user/my-api"
scan_dirs = ["src"]

[repos.frontend]
root = "/home/user/my-app"
```

All tools gain an optional `repo` parameter. When omitted with a single repo, it works exactly as before. With multiple repos, search tools automatically search across all repos with results tagged by repo name.

You can also add repos dynamically at runtime using the `cs_add_repo` tool.

## MCP Tools

| Tool | What it does |
|------|-------------|
| **Search** | |
| `cs_find` | Combined filename + content search (start here) |
| `cs_grep` | Regex content search with context lines |
| `cs_search` | Fuzzy filename and module search |
| **Read** | |
| `cs_read_file` | Read a file -- full content or structural stubs only |
| `cs_read_files` | Batch read up to 50 files |
| `cs_read_context` | Budget-aware batch read -- fits N files into a token budget |
| **Navigate** | |
| `cs_list_modules` | List all detected modules/categories |
| `cs_get_module_files` | List files in a module |
| `cs_get_deps` | Module dependency graph |
| `cs_find_imports` | Import/include relationship tracing |
| **Analyze** | |
| `cs_impact` | Impact analysis -- what breaks if I change this file? |
| **Server** | |
| `cs_status` | Show indexed repos, file counts, languages, scan time |
| `cs_rescan` | Re-index repos without restarting |
| `cs_add_repo` | Dynamically add a repo at runtime |

## Impact Analysis

The `cs_impact` tool traces the import graph to answer "what breaks if I change this file?":

```
Impact analysis for src/types.rs

Depth 1 (direct dependents): 6 files
  src/api.rs
  src/budget.rs
  src/fuzzy.rs
  src/main.rs
  src/mcp.rs
  src/scan.rs

Total: 6 files affected across 1 depth level
```

Works across repo boundaries -- if repo B imports from repo A, `cs_impact` traces the full cross-repo dependency chain.

## CLI Subcommands

```bash
# Auto-detect project type, generate .codescope.toml + .mcp.json
codescope-server init [/path/to/project]

# Add to global config instead of per-project
codescope-server init --global

# Diagnostics -- check config, test scan, validate setup
codescope-server doctor [/path/to/project]
```

## Web UI

After running `setup.sh`, browse any project with:

```bash
codescope-web /path/to/project
```

Opens at `http://localhost:8432`. Set `PORT=9000` for a custom port.

### Features

- **File browser** -- tree view with syntax-highlighted source viewer
- **Full-text search** -- regex-powered grep with ranked results
- **Treemap visualization** -- file sizes by module, zoomable
- **3D dependency graph** -- interactive force-directed graph of module dependencies
- **Theme toggle** -- dark, light, and system modes

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+K` | Focus search |
| `Ctrl+B` | Toggle sidebar |
| `Ctrl+1` through `Ctrl+5` | Switch panels (Files, Search, Modules, Deps, Treemap) |

Panels are drag-to-resize.

The web UI requires Node.js at install time (for building the React frontend). If you installed without Node.js, re-run `setup.sh` after installing it.

## Configuration

Drop a `.codescope.toml` in your project root to customize scanning:

```toml
# Only scan these directories (default: scan everything)
scan_dirs = ["src", "lib"]

# Skip these directories (merged with built-in defaults)
skip_dirs = ["vendor", "generated"]

# Only index these extensions (default: common source extensions)
extensions = [".rs", ".ts", ".go", ".py"]

# Treat these as noise/library directories (lower search priority)
noise_dirs = ["third_party"]
```

Built-in defaults for `skip_dirs`: `node_modules`, `target`, `dist`, `.git`, `build`, `__pycache__`, `vendor`, and others. Built-in defaults for `noise_dirs`: `src`, `lib`, `Source`, `Include`, and similar common directory names that add noise to category paths.

## Development

### Prerequisites

- Rust 1.75+ (for the server)
- Node.js 18+ (for the web UI, optional)

### Running in Dev Mode

Start the backend and frontend separately for hot-reload:

```bash
# Terminal 1: Rust server
cd server
cargo run -- --root /path/to/project

# Terminal 2: Vite dev server (proxies API to :8432)
npm run dev
```

### Building from Source

```bash
# Server only
cd server
cargo build --release

# Server with semantic search (experimental)
cargo build --release --features semantic

# Web UI
npm install
npm run build

# Both (via setup script)
cd server && ./setup.sh
```

Binary lands at `server/target/release/codescope-server`. Web UI builds to `dist/`.

### Running Tests

```bash
# Integration tests (requires built server binary)
bash tests/integration.sh

# Lint
cargo clippy --manifest-path server/Cargo.toml -- -D warnings
cargo fmt --manifest-path server/Cargo.toml -- --check
npx tsc --noEmit
```

## Architecture

```
server/src/
├── main.rs        -- CLI parsing, HTTP server (Axum), MCP mode entry
├── mcp.rs         -- MCP stdio server (JSON-RPC), 14 tools
├── api.rs         -- HTTP API handlers (/api/tree, /api/grep, etc.)
├── scan.rs        -- File discovery, module detection, dependency + import scanning
├── stubs.rs       -- Language-aware structural stub extraction (signatures without bodies)
├── fuzzy.rs       -- FZF v2 fuzzy matching (Smith-Waterman with bitmask pre-filter)
├── budget.rs      -- Token budget allocation (water-fill algorithm across files)
├── tokenizer.rs   -- Token counting (bytes-estimate or tiktoken)
├── types.rs       -- Shared types: RepoState, ServerState, scoring helpers
├── init.rs        -- CLI subcommands: init, doctor
└── semantic.rs    -- Semantic code search (feature-gated, optional)

src/               -- React 18 frontend (Vite + TypeScript)
├── App.tsx        -- Main app shell, panels, keyboard shortcuts
└── ...
```

### Language Support

Stub extraction and import tracing: Rust, TypeScript/JavaScript, Python, Go, C/C++, C#, Java, Kotlin, Swift, Ruby, PHP, Lua, Zig, PowerShell, and more.

Dependency scanning: Cargo.toml, package.json, go.mod, .csproj.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## License

MIT
