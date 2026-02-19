# CodeScope

[![CI](https://github.com/AlrikOlson/codescope/actions/workflows/ci.yml/badge.svg)](https://github.com/AlrikOlson/codescope/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Fast codebase indexer and search server. Works as an [MCP](https://modelcontextprotocol.io/) server for Claude Code or as a standalone HTTP server with a rich web UI.

Indexes 200K+ files in under 2 seconds. Understands module structure, import graphs, and file dependencies across 18+ languages.

## Quick Start

**Install:**

```bash
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash
```

Downloads a pre-built binary (~5MB). No compilation needed.

**Set up your project:**

```bash
cd /path/to/your/project
codescope-server init
```

Generates `.codescope.toml` and `.mcp.json`. Open Claude Code in that directory and CodeScope tools are available immediately.

**Or both in one command:**

```bash
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash -s -- /path/to/project
```

## Why CodeScope?

- **Structural awareness** — not just text search. Extracts function/class signatures, traces import graphs, and detects module boundaries across 18+ languages.
- **Token-budget reads** — `cs_read_context` uses a water-fill algorithm to allocate tokens across files by importance, fitting exactly what an LLM's context window can hold.
- **Impact analysis** — "what breaks if I change this file?" Traces the full import dependency chain with configurable depth, even across repository boundaries.
- **Semantic search** — find code by meaning, not just keywords. Uses BERT embeddings (all-MiniLM-L6-v2) for intent-based matching.

## MCP Tools

| Tool | Description |
|------|-------------|
| **Search** | |
| `cs_find` | Combined filename + content search — start here |
| `cs_grep` | Regex content search with context lines, extension/category filters |
| `cs_search` | Fuzzy filename and module search (CamelCase-aware) |
| `cs_semantic_search` | Search by intent using ML embeddings (requires `--semantic` flag) |
| **Read** | |
| `cs_read_file` | Read a file — full content or structural stubs (signatures without bodies) |
| `cs_read_files` | Batch read up to 50 files |
| `cs_read_context` | Budget-aware batch read — fits N files into a token budget with importance-weighted compression |
| **Navigate** | |
| `cs_list_modules` | List all detected modules/categories with file counts |
| `cs_get_module_files` | List files in a specific module |
| `cs_get_deps` | Module dependency graph (public/private) |
| `cs_find_imports` | Trace import/include relationships in both directions |
| **Analyze** | |
| `cs_impact` | Full dependency chain — what files are affected by a change, with depth control |
| **Server** | |
| `cs_status` | Indexed repos, file counts, language breakdown, scan time |
| `cs_rescan` | Re-index repos without restarting |
| `cs_add_repo` | Add a new repository at runtime |

## Semantic Search

All pre-built binaries include semantic search. Enable it at startup:

```bash
codescope-server --mcp --root /path/to/project --semantic
```

Uses the [all-MiniLM-L6-v2](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2) model (~90MB, downloaded to `~/.cache/codescope/models/` on first use). Generates 384-dimensional embeddings with mean pooling and L2 normalization, then ranks results by cosine similarity.

When `--semantic` is passed, `codescope-server init` automatically configures your project to use it.

## Web UI

```bash
codescope-web /path/to/project
```

Opens at `http://localhost:8432`. Set `PORT=9000` for a custom port.

**Panels:**

| Shortcut | Panel |
|----------|-------|
| `Ctrl+K` / `Ctrl+1` | Search — full-text with fuzzy matching |
| `Ctrl+B` / `Ctrl+2` | Tree — file/module browser with breadcrumbs |
| `Ctrl+3` | Context — selected files inspector |
| `Ctrl+4` | Stats — codebase metrics dashboard |
| `Ctrl+5` | Graph — interactive dependency visualization |

**View modes:** file list with preview, treemap visualization (Three.js), dependency graph, stats dashboard. Supports dark/light/system themes, virtual scrolling for large codebases, and responsive layouts.

## Multi-Repo Support

```bash
# Named repos via CLI
codescope-server --mcp --repo engine=/path/to/engine --repo game=/path/to/game

# Via config file
codescope-server --mcp --config ~/.codescope/repos.toml

# Single repo (default)
codescope-server --mcp --root /path/to/project
```

Config file format:

```toml
[repos.backend]
root = "/home/user/my-api"
scan_dirs = ["src"]

[repos.frontend]
root = "/home/user/my-app"
```

All tools gain an optional `repo` parameter. With a single repo it's automatic. With multiple repos, search results are tagged by repo name. Add repos at runtime with `cs_add_repo`.

## Configuration

Drop a `.codescope.toml` in your project root:

```toml
# Only scan these directories (default: scan everything)
scan_dirs = ["src", "lib"]

# Skip these directories (merged with built-in defaults)
skip_dirs = ["vendor", "generated"]

# Only index these extensions (default: common source extensions)
extensions = [".rs", ".ts", ".go", ".py"]

# Lower search priority for these directories
noise_dirs = ["third_party"]
```

Built-in `skip_dirs`: `node_modules`, `target`, `dist`, `.git`, `build`, `__pycache__`, `vendor`, and others.

## Release Channels

```bash
# Stable (default) — tagged releases
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash

# Edge — latest master commit
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash -s -- --edge

# Dev — latest dev branch commit
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash -s -- --dev
```

## CLI Reference

```
codescope-server [OPTIONS] [SUBCOMMAND]

Subcommands:
  init [PATH]              Auto-detect project, generate config files
  init --global            Add to global config (~/.codescope/repos.toml)
  doctor [PATH]            Diagnose setup issues

Options:
  --root <PATH>            Project root (default: current directory)
  --repo <NAME=PATH>       Named repository (repeatable)
  --config <PATH>          Load repos from a TOML config file
  --mcp                    Run as MCP stdio server
  --semantic               Enable semantic code search
  --dist <PATH>            Path to web UI dist directory
  --tokenizer <NAME>       Token counter: bytes-estimate (default) or tiktoken
  --help                   Show help
  --version                Show version

Environment:
  PORT                     HTTP server port (default: auto-scan 8432-8441)
```

## Troubleshooting

**"codescope-server: command not found"** — Restart your terminal or `source ~/.bashrc` (or `~/.zshrc`).

**Semantic search not responding** — Make sure you passed `--semantic` when starting the server. The model downloads on first use (~90MB).

**Install fails behind a proxy** — Build from source: `bash setup.sh --from-source` (requires Rust toolchain).

**Claude Code doesn't see the tools** — Run `codescope-server init` in your project directory, then restart Claude Code. Verify `.mcp.json` exists in your project root.

**WSL** — Works the same as regular Linux. No special steps.

---

## Development

### Prerequisites

- Rust 1.87+ (candle-core requires `unsigned_is_multiple_of`, stabilized in 1.87)
- Node.js 18+ (web UI, optional for server-only development)

### Dev Mode

```bash
# Terminal 1: Rust server
cd server && cargo run -- --root /path/to/project

# Terminal 2: Vite dev server (proxies API to :8432)
npm run dev
```

### Building from Source

```bash
# Server with semantic search
cargo build --release --manifest-path server/Cargo.toml --features semantic

# Web UI
npm ci && npm run build

# Both via setup script
cd server && ./setup.sh --from-source
```

Binary: `server/target/release/codescope-server`. Web UI: `dist/`.

### Testing

```bash
# Integration tests
bash tests/integration.sh

# Lint
cargo fmt --manifest-path server/Cargo.toml -- --check
cargo clippy --manifest-path server/Cargo.toml -- -D warnings
npx tsc --noEmit
```

### CI Pipeline

Single workflow (`ci.yml`):

```
PR:      lint ─┐ (parallel)
         test ─┘

master:  lint ─┬─→ version (AI) ─→ build (4 platforms) ─→ stable-release
         test ─┘                                         ─→ channel-release (edge)

dev:     lint ─┬─→ build (4 platforms) ─→ channel-release (dev)
         test ─┘
```

Version analysis uses the [Claude Agent SDK](https://docs.anthropic.com/en/docs/agents-and-tools/claude-code/sdk) with CodeScope's own MCP tools to analyze changes and determine semantic version bumps, commit messages, and release notes.

## Architecture

```
server/src/
├── main.rs        CLI parsing, HTTP server (Axum), MCP mode entry
├── mcp.rs         MCP stdio server (JSON-RPC), 15 tools
├── api.rs         HTTP API handlers (/api/tree, /api/grep, etc.)
├── scan.rs        File discovery, module detection, dependency + import scanning
├── stubs.rs       Language-aware stub extraction (signatures without bodies)
├── fuzzy.rs       FZF v2 fuzzy matching (Smith-Waterman with bitmask pre-filter)
├── budget.rs      Token budget allocation (water-fill algorithm across files)
├── tokenizer.rs   Token counting (bytes-estimate or tiktoken)
├── types.rs       Shared types: RepoState, ServerState, IDF index, scoring
├── init.rs        CLI subcommands: init, doctor
└── semantic.rs    Semantic search via all-MiniLM-L6-v2 BERT embeddings

src/               React 18 frontend (Vite + TypeScript)
├── App.tsx        Main app shell, panels, keyboard shortcuts
└── ...            TreeSidebar, FileList, SearchSidebar, CodebaseMap,
                   DependencyGraph, StatsDashboard, ContextPanel, ActivityBar
```

### Language Support

**Stub extraction** (function/class signatures):

Brace-based: C, C++, C#, Java, Kotlin, Scala, Rust, Go, JavaScript, TypeScript, Swift, D, PowerShell, HLSL/GLSL/WGSL shaders

Indent-based: Python, Ruby

Config: JSON, YAML, TOML, XML, INI

**Import tracing:**

C/C++ (`#include`), Python (`import`/`from`), JavaScript/TypeScript (`import`/`require`), Rust (module system), Go (package imports), C# (`using`), PowerShell (`Import-Module`)

**Package manager detection:** Cargo.toml, package.json, go.mod, .csproj

## License

MIT
