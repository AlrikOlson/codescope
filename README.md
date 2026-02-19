# CodeScope

[![CI](https://github.com/AlrikOlson/codescope/actions/workflows/integration.yml/badge.svg)](https://github.com/AlrikOlson/codescope/actions/workflows/integration.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A fast codebase indexer and search server. Works as an [MCP](https://modelcontextprotocol.io/) server for Claude Code (and other MCP clients) or as a standalone HTTP server with a rich web UI.

Indexes 200K+ files in under 2 seconds. Understands module structure, import graphs, and file dependencies across 18+ languages out of the box.

## Quick Start

### 1. Install

```bash
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash
```

Downloads a pre-built binary (~5MB). Takes about 10 seconds. No compilation needed.

### 2. Set Up Your Project

```bash
cd /path/to/your/project
codescope-server init
```

This generates config files for your project. Open Claude Code in that directory and CodeScope tools are available immediately.

### Or do both in one command

```bash
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash -s -- /path/to/project
```

## Why CodeScope?

- **Structural awareness** -- not just text search. CodeScope extracts function/class signatures, traces import graphs, and detects module boundaries across 18+ languages.
- **Token-budget reads** -- feed an LLM exactly what fits in its context window. The `cs_read_context` tool uses a water-fill algorithm to allocate tokens across files by importance.
- **Impact analysis** -- answer "what breaks if I change this file?" by tracing the full import dependency chain, even across repository boundaries.
- **One command to start** -- `codescope-server init` auto-detects your project type, generates config, and you're ready to go.

## MCP Tools

| Tool | What it does |
|------|-------------|
| **Search** | |
| `cs_find` | Combined filename + content search (start here) |
| `cs_grep` | Regex content search with context lines |
| `cs_search` | Fuzzy filename and module search |
| `cs_semantic_search` | Search by intent using ML embeddings (requires `--with-semantic`) |
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

## Semantic Search (Optional)

ML-powered search that finds code by meaning, not just keywords. Requires compiling from source (~5 minutes, one-time):

```bash
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash -s -- --with-semantic
```

Uses a BERT model (`all-MiniLM-L6-v2`, ~90MB, downloaded on first use) for vector similarity search. When enabled, `codescope-server init` automatically configures your project to use it.

## Web UI

After installing, browse any project with:

```bash
codescope-web /path/to/project
```

Opens at `http://localhost:8432`. Set `PORT=9000` for a custom port.

Features: file browser with syntax highlighting, regex search, treemap visualization, 3D dependency graph, dark/light themes.

| Shortcut | Action |
|----------|--------|
| `Ctrl+K` | Focus search |
| `Ctrl+B` | Toggle sidebar |
| `Ctrl+1` through `Ctrl+5` | Switch panels |

The web UI requires Node.js at install time. If you installed without Node.js, re-run `setup.sh` after installing it.

## Multi-Repo Support

Index multiple repositories simultaneously:

```bash
# Named repos via CLI
codescope-server --mcp --repo engine=/path/to/engine --repo game=/path/to/game

# Via config file
codescope-server --mcp --config ~/.codescope/repos.toml

# Single repo (default)
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

All tools gain an optional `repo` parameter. With a single repo it works automatically. With multiple repos, search results are tagged by repo name. You can also add repos at runtime with `cs_add_repo`.

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

Built-in defaults for `skip_dirs`: `node_modules`, `target`, `dist`, `.git`, `build`, `__pycache__`, `vendor`, and others.

## CLI Subcommands

```bash
# Auto-detect project type, generate .codescope.toml + .mcp.json
codescope-server init [/path/to/project]

# Add to global config instead of per-project
codescope-server init --global

# Diagnostics -- check config, test scan, validate setup
codescope-server doctor [/path/to/project]
```

## Troubleshooting

### "codescope-server: command not found"

Restart your terminal, or run:

```bash
source ~/.bashrc    # or: source ~/.zshrc
```

### Semantic search not working

The standard install does not include semantic search. Reinstall with:

```bash
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash -s -- --with-semantic
```

### Install fails behind a corporate proxy

Use `--from-source` with the Rust toolchain already installed:

```bash
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash -s -- --from-source
```

### WSL (Windows Subsystem for Linux)

Works the same as regular Linux. No special steps needed.

### Claude Code doesn't see the tools

Make sure you ran `codescope-server init` in your project directory, then restart Claude Code. Check that `.mcp.json` exists in your project root.

---

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

# Server with semantic search
cargo build --release --features semantic

# Web UI
npm install
npm run build

# Both (via setup script)
cd server && ./setup.sh --from-source

# Both with semantic search
cd server && ./setup.sh --with-semantic
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
├── types.rs       -- Shared types: RepoState, ServerState, IDF index, scoring helpers
├── init.rs        -- CLI subcommands: init, doctor
└── semantic.rs    -- Semantic code search via BERT embeddings (feature-gated, optional)

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
