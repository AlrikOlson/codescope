# CodeScope

[![CI](https://github.com/AlrikOlson/codescope/actions/workflows/ci.yml/badge.svg)](https://github.com/AlrikOlson/codescope/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

CodeScope indexes your codebase and exposes it over MCP. Scans 200K files in ~2s, extracts function/class signatures across 18 languages, builds import dependency graphs, and uses a water-fill algorithm to pack relevant context into token budgets.

Works as an MCP server for [Claude Code](https://docs.anthropic.com/en/docs/agents-and-tools/claude-code) or as a standalone HTTP server with a web UI.

## Install

```bash
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash
```

Pre-built binary, ~5MB.

## Setup

```bash
cd /path/to/your/project
codescope-server init
```

Generates `.codescope.toml` and `.mcp.json`. Restart Claude Code to pick up the tools.

## How It Works

**Scanning.** Parallel directory walk via the `ignore` crate, respects `.gitignore`. Detects modules by directory heuristics (e.g., a directory with its own `Cargo.toml` or `package.json` is a module boundary). Files are scored by IDF-weighted path terms for search ranking.

**Stub extraction.** Strips function and class bodies, keeps signatures. Uses brace-depth tracking for C-family languages and indentation tracking for Python/Ruby. This lets agents read the structure of a file without burning tokens on implementation details. Quality varies by language — works best for Rust, TypeScript, and Python; C++ templates can trip up the brace tracker.

**Fuzzy matching.** FZF v2 algorithm: 64-bit bitmask pre-filter rejects non-matching candidates in O(1), then Smith-Waterman DP scores matches with bonuses for CamelCase boundaries, path delimiters, and consecutive characters.

**Budget allocation.** `cs_read_context` ranks files by importance (search relevance, dependency centrality), then fills a token budget using a water-fill strategy. Files demote through tiers — full content, stubs, pruned stubs, manifest-only — until everything fits. Files the agent already read in the current session get deprioritized automatically.

## MCP Tools

19 tools, grouped by function:

| Tool | Description |
|------|-------------|
| `cs_find` | Combined filename + content search (start here) |
| `cs_grep` | Regex content search with context lines and file filters |
| `cs_semantic_search` | Search by intent using BERT embeddings (requires `--semantic`) |
| `cs_read_file` | Read a file: full content or structural stubs (signatures only) |
| `cs_read_files` | Batch read up to 50 files |
| `cs_read_context` | Budget-aware batch read with importance-weighted compression |
| `cs_list_modules` | List detected modules with file counts |
| `cs_get_module_files` | List files in a specific module |
| `cs_get_deps` | Module dependency graph (public/private) |
| `cs_find_imports` | Trace import relationships in both directions |
| `cs_impact` | Transitive dependency chain — what breaks if a file changes |
| `cs_blame` | Git blame for a file or line range |
| `cs_file_history` | Recent commits that touched a file, with co-changed files |
| `cs_changed_since` | Files changed since a commit, branch, or tag |
| `cs_hot_files` | Most frequently changed files (churn ranking) |
| `cs_session_info` | Files read in the current MCP session |
| `cs_status` | Indexed repos, file counts, language stats, scan time |
| `cs_rescan` | Re-index without restarting |
| `cs_add_repo` | Add a repository at runtime |

## Semantic Search

Enable at startup:

```bash
codescope-server --mcp --root /path/to/project --semantic
```

Uses [all-MiniLM-L6-v2](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2) (~90MB, downloaded to `~/.cache/codescope/models/` on first use). Generates 384-dimensional embeddings with mean pooling and L2 normalization, ranks by cosine similarity. Adds a few seconds to startup for indexing.

When `--semantic` is passed, `codescope-server init` configures the project to use it automatically.

## Web UI

```bash
codescope-web /path/to/project
```

Opens at `http://localhost:8432`. Set `PORT=9000` for a custom port.

| Shortcut | Panel |
|----------|-------|
| `Ctrl+K` / `Ctrl+1` | Search (full-text, fuzzy) |
| `Ctrl+B` / `Ctrl+2` | File/module tree browser |
| `Ctrl+3` | Context inspector |
| `Ctrl+4` | Codebase stats |
| `Ctrl+5` | Dependency graph |

Also includes a treemap visualization (Three.js) and dark/light theme support.

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

`codescope-server: command not found` — Restart your terminal or `source ~/.bashrc` / `~/.zshrc`.

Semantic search not responding — Make sure you passed `--semantic` when starting the server. The model downloads on first use (~90MB).

Install fails behind a proxy — Build from source: `bash setup.sh --from-source` (requires Rust toolchain).

Claude Code doesn't see the tools — Run `codescope-server init` in your project directory, then restart Claude Code. Verify `.mcp.json` exists in your project root.

WSL — Works the same as regular Linux. No special steps.

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
├── mcp.rs         MCP stdio server (JSON-RPC), 19 tools
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

Stub extraction (function/class signatures):

Brace-based: C, C++, C#, Java, Kotlin, Scala, Rust, Go, JavaScript, TypeScript, Swift, D, PowerShell, HLSL/GLSL/WGSL shaders

Indent-based: Python, Ruby

Config: JSON, YAML, TOML, XML, INI

Import tracing:

C/C++ (`#include`), Python (`import`/`from`), JavaScript/TypeScript (`import`/`require`), Rust (module system), Go (package imports), C# (`using`), PowerShell (`Import-Module`)

Package manager detection: Cargo.toml, package.json, go.mod, .csproj

## License

MIT
