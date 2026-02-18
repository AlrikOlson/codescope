# CodeScope

A fast codebase indexer and search server. Works as an [MCP](https://modelcontextprotocol.io/) server for Claude Code (and other MCP clients) or as a standalone HTTP server with a web UI.

Built in Rust. Indexes 200K+ files in under 2 seconds. Understands module structure, import graphs, and file dependencies out of the box.

## Quick Start

```bash
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash
```

Or clone and run manually:

```bash
git clone https://github.com/AlrikOlson/codescope.git
cd codescope/server
./setup.sh
```

This installs the Rust toolchain (if needed), builds the server, and if Node.js is available, builds the web UI too. Everything goes into `~/.local/bin/`.

Then, in any project you want to index:

```bash
# MCP server (for Claude Code)
cd /path/to/your/project
codescope-init

# Web UI (standalone browser)
codescope-web /path/to/your/project
```

`codescope-init` creates a `.mcp.json` that Claude Code picks up automatically. `codescope-web` launches the browser UI at `http://localhost:8432`.

## MCP Tools

| Tool | What it does |
|------|-------------|
| `cs_find` | Combined filename + content search (start here) |
| `cs_grep` | Regex content search with context lines |
| `cs_read_file` | Read a file — full content or structural stubs only |
| `cs_read_files` | Batch read up to 50 files |
| `cs_read_context` | Budget-aware batch read — fits N files into a token budget |
| `cs_search` | Fuzzy filename and module search |
| `cs_list_modules` | List all detected modules/categories |
| `cs_get_module_files` | List files in a module |
| `cs_get_deps` | Module dependency graph |
| `cs_find_imports` | Import/include relationship tracing |

## Web UI

After running `setup.sh`, browse any project with:

```bash
codescope-web /path/to/project
```

Opens at `http://localhost:8432`. Set `PORT=9000` for a custom port.

### Features

- **File browser** — tree view with syntax-highlighted source viewer
- **Full-text search** — regex-powered grep with ranked results
- **Treemap visualization** — file sizes by module, zoomable
- **3D dependency graph** — interactive force-directed graph of module dependencies
- **Theme toggle** — dark, light, and system modes

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

# Treat these as noise/library directories (lower search priority, deprioritized in results)
noise_dirs = ["third_party"]
```

Built-in defaults for `skip_dirs`: `node_modules`, `target`, `dist`, `.git`, `build`, `__pycache__`, and others. Built-in defaults for `noise_dirs`: `ThirdParty`, `Plugins`, `External`, and similar vendor directories.

## Development

### Prerequisites

- Rust 1.75+ (for the server)
- Node.js 18+ (for the web UI)

### Running in Dev Mode

Start the backend and frontend separately for hot-reload:

```bash
# Terminal 1: Rust server (watches and rebuilds)
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
npx tsc --noEmit
```

## Architecture

```
server/src/
├── main.rs        — CLI parsing, HTTP server (Axum), MCP mode entry
├── mcp.rs         — MCP stdio server (JSON-RPC over stdin/stdout)
├── api.rs         — HTTP API handlers (/api/tree, /api/grep, etc.)
├── scan.rs        — File discovery, module detection, dependency + import scanning
├── stubs.rs       — Language-aware structural stub extraction (signatures without bodies)
├── fuzzy.rs       — FZF v2 fuzzy matching (Smith-Waterman with bitmask pre-filter)
├── budget.rs      — Token budget allocation (water-fill algorithm across files)
├── tokenizer.rs   — Token counting (bytes-estimate or tiktoken)
└── types.rs       — Shared types, state structs, scoring helpers

src/               — React 18 frontend (Vite + TypeScript)
├── App.tsx        — Main app shell, panels, keyboard shortcuts
└── ...
```

### Language Support

Stub extraction and import tracing support: Rust, TypeScript/JavaScript, Python, Go, C/C++, C#, Java, Kotlin, Swift, Ruby, PHP, Lua, Zig, TOML, YAML, JSON, XML, and more.

Dependency scanning supports: Cargo.toml, package.json, go.mod, C# `.Build.cs` / `.csproj`.

## License

MIT
