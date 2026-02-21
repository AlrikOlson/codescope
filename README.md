# CodeScope

[![CI](https://github.com/AlrikOlson/codescope/actions/workflows/ci.yml/badge.svg)](https://github.com/AlrikOlson/codescope/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

An MCP server that gives AI coding agents structural understanding of your codebase. Instead of agents fumbling through files with `Read` and `Grep`, CodeScope gives them the tools to actually navigate code — searching across filenames, content, and semantics simultaneously, reading function signatures without burning tokens on implementation details, tracing import graphs to understand blast radius before touching shared code, and packing exactly the right context into a token budget.

Scans 200K files in ~2 seconds. 20+ languages. Also ships with a standalone web UI for visual codebase exploration.

## Install

```bash
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash
```

Detects your platform (Linux, macOS, Windows via WSL/Git Bash), downloads a ~5MB binary to `~/.local/bin/` (Linux/macOS) or `%LOCALAPPDATA%\codescope\bin` (Windows). Semantic search included (CPU). Never modifies your PATH — tells you what to add.

On Windows, use `curl.exe` (not `curl`, which is a PowerShell alias):

```powershell
curl.exe -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash
```

Or grab a binary from [Releases](https://github.com/AlrikOlson/codescope/releases) and add it to your PATH.

<details>
<summary>Other install channels</summary>

```bash
# Edge — latest master commit (may break)
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash -s -- --edge

# Dev — latest dev branch commit
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash -s -- --dev

# Build from source (requires Rust 1.87+)
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash -s -- --from-source

# Build from source with CUDA GPU acceleration (auto-detected)
curl -sSL https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.sh | bash -s -- --cuda

# Windows (PowerShell — delegates to bash via WSL or Git Bash)
irm https://raw.githubusercontent.com/AlrikOlson/codescope/master/server/setup.ps1 | iex
```
</details>

## Quick Start

```bash
cd /path/to/your/project
codescope init
```

This does three things:

1. **Detects your project type** — Rust, Node.js, Go, Python, C/C++, .NET, Unreal Engine, pnpm/uv workspaces. Figures out which directories to scan and which to skip.
2. **Generates `.codescope.toml`** — Project-specific config with scan dirs, extensions, and skip dirs tuned to your ecosystem.
3. **Generates `.mcp.json`** — Tells Claude Code to start CodeScope as an MCP server when you open this project.

Restart Claude Code. Your agent now has 9 code navigation tools (`cs_search`, `cs_read`, `cs_imports`, etc.) instead of relying on raw file reads and grep.

Run `codescope doctor` to verify everything is wired up correctly.

### Global Repos

```bash
codescope init --global
```

Adds the project to `~/.codescope/repos.toml` so CodeScope loads it automatically in MCP mode, even without a local `.mcp.json`. Useful for repos you always want indexed.

## What the Agent Gets

Without CodeScope, an AI agent exploring a codebase has `Read`, `Grep`, and `Glob`. It reads entire files hoping to find what it needs, greps with patterns it guesses, and burns tokens on implementation details it doesn't care about.

With CodeScope, the agent gets 9 purpose-built tools that understand code structure:

| Tool | What the agent can do with it |
|------|-------------------------------|
| `cs_search` | Find code by concept, not just string matching. Searches filenames, content, and semantic meaning simultaneously. The agent's first move in any exploration. |
| `cs_grep` | Regex search with context lines, scoped by path or file extension. For when the agent knows what pattern it's looking for. |
| `cs_read` | Read files intelligently: full content when needed, structural stubs (just signatures) to understand a file's shape without reading every line, or budget-aware batch reads across many files at once. |
| `cs_modules` | Understand project structure — what modules exist, what files belong to each, how modules depend on each other. |
| `cs_imports` | Before modifying shared code, the agent traces what imports a file and what it imports. `transitive: true` shows the full blast radius — every file that would be affected by a change. |
| `cs_git` | Git-aware exploration: blame, file history, recently changed files, and churn ranking to identify hotspots. |
| `cs_status` | The agent's orientation tool — what repos are indexed, file counts, language breakdown, whether semantic search is ready. |
| `cs_rescan` | Re-index after the agent or user makes external changes, without restarting. |
| `cs_add_repo` | Dynamically add another repository mid-session. |

### How the Agent Uses These

A typical agent exploration looks like this:

1. **Search** — `cs_search("authentication middleware")` finds relevant files across the codebase
2. **Skim** — `cs_read(path, mode: "stubs")` shows function signatures and class structure without reading implementation details (saves tokens)
3. **Impact analysis** — `cs_imports(path, transitive: true)` maps out what depends on this code before the agent touches it
4. **Deep read** — `cs_read(paths: [...], budget: 8000)` batch-reads the files the agent actually needs, automatically prioritized and packed to fit the token budget

### Token Budget Management

The agent can request multiple files with a token budget and CodeScope handles the rest. Files are ranked by relevance and demoted through tiers — full content, then stubs, then pruned stubs, then just a manifest entry — until everything fits. Files the agent already read in the current session are deprioritized automatically so it doesn't re-read the same code.

## Multi-Repo Support

Index multiple repositories in a single CodeScope instance:

```bash
# Named repos on the command line
codescope --mcp --repo backend=/path/to/api --repo frontend=/path/to/app

# Or via config file
codescope --mcp --config repos.toml

# Single repo (the default)
codescope --mcp --root /path/to/project
```

Config file format (`repos.toml`):

```toml
[repos.backend]
root = "/home/user/my-api"
scan_dirs = ["src"]

[repos.frontend]
root = "/home/user/my-app"
```

All tools gain an optional `repo` parameter. With a single repo it's implicit. With multiple repos, search results are tagged by repo name and cross-repo import edges are resolved automatically.

## Semantic Search

Enabled by default. This is what makes `cs_search` work by concept rather than just string matching — the agent can search for "error handling" and find `try/catch` blocks, exception classes, and error middleware even if none of them contain the word "error" in their names.

Uses [all-MiniLM-L6-v2](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2) (~90MB, downloaded to `~/.cache/codescope/models/` on first use). Adds a few seconds to startup for indexing.

Pre-built binaries use CPU inference. For GPU acceleration, build from source with your local CUDA toolkit:

```bash
# Build from source with CUDA (auto-detects nvcc)
bash setup.sh --cuda

# Pre-build the semantic index during init (avoids first-query delay)
codescope init --semantic

# Disable if you don't need it or are on a constrained system
codescope --mcp --root /path/to/project --no-semantic

# Use a code-optimized model instead
codescope --mcp --semantic-model codebert
```

Available models: `minilm` (default), `codebert`, `starencoder`, or any HuggingFace model ID.

## Web UI

```bash
codescope web /path/to/project
```

Opens at `http://localhost:8432`. Set `PORT=9000` for a custom port.

**Panels:**

- **Explorer** (`Ctrl+B`) — File/module tree with integrated context builder. Select files to build LLM context. Selections sync with the map and graph views.
- **Search** (`Ctrl+K`) — Full-text fuzzy search with real-time results and file preview.

**Views:**

- **Files** — Flat file list for the active module, with inline preview.
- **Map** — Squarified treemap of the entire codebase. Zoom, pan, click to select modules. Double-click to zoom into a subtree.
- **Graph** — 3D force-directed dependency graph (Three.js). Nodes colored by category, edges show public/private dependencies. Handles 1000+ node graphs.
- **Stats** — Language breakdown, file counts, scan timing.

Dark/light/system theme toggle in the activity bar.

## Configuration

Drop a `.codescope.toml` in your project root (or let `codescope init` generate one):

```toml
# Only scan these directories (default: scan everything)
scan_dirs = ["src", "lib"]

# Skip these directories (merged with built-in defaults like node_modules, target, .git)
skip_dirs = ["vendor", "generated"]

# Only index files with these extensions
extensions = ["rs", "ts", "go", "py"]

# Lower search ranking for files in these directories
noise_dirs = ["third_party"]
```

`codescope init` auto-detects your project type and generates sensible defaults. It understands Cargo workspaces, npm/pnpm/yarn workspaces, Go workspaces, uv workspaces, and .NET solution structures.

## CLI Reference

```
codescope [OPTIONS] [COMMAND]

Commands:
  init [PATH]              Auto-detect project, generate .codescope.toml + .mcp.json
    --global               Add to ~/.codescope/repos.toml for persistent indexing
    --semantic             Pre-build semantic index cache
  doctor [PATH]            Check config files, binary, MCP setup, run a test scan
  web [PATH]               Launch the web UI and open in browser
  completions <SHELL>      Generate shell completions (bash, zsh, fish, powershell)

Options:
  --root <PATH>            Project root (default: current directory)
  --repo <NAME=PATH>       Named repository (repeatable)
  --config <PATH>          Load repos from a TOML config file
  --mcp                    Run as MCP stdio server (for Claude Code)
  --dist <PATH>            Path to web UI dist directory
  --no-semantic            Disable semantic code search
  --semantic-model <NAME>  Embedding model: minilm (default), codebert, starencoder
  --wait-semantic          Block startup until semantic index is built (useful for CI)
  --bind-all               Bind 0.0.0.0 instead of localhost
  --tokenizer <NAME>       Token counter: bytes-estimate (default) or tiktoken
  --version                Show version

Environment:
  PORT                     HTTP server port (default: auto-scan 8432-8441)
  RUST_LOG                 Log verbosity (e.g. RUST_LOG=codescope=debug)
```

## Troubleshooting

**`codescope: command not found`** — The installer tells you what to add to your PATH. On Linux/macOS, add `~/.local/bin` to PATH. On Windows, add `%LOCALAPPDATA%\codescope\bin`. Restart your terminal after updating PATH.

**Claude Code doesn't see the tools** — Run `codescope init` in your project directory, then restart Claude Code. Check that `.mcp.json` exists and contains a `codescope` entry. Run `codescope doctor` for a full diagnostic.

**Semantic search not working** — Run `cs_status` to check indexing progress. The model (~90MB) downloads on first use. If you're behind a proxy, try `--no-semantic` or build from source.

**Install fails** — Try building from source: `bash setup.sh --from-source` (requires Rust 1.87+).

**WSL** — The installer detects WSL automatically and installs the Windows binary to `%LOCALAPPDATA%\codescope\bin`. Building from source (`--from-source` / `--cuda`) produces a Linux binary for use within WSL.

**PowerShell `curl` fails** — In PowerShell 5.1, `curl` is an alias for `Invoke-WebRequest`. Use `curl.exe` instead, or use the PowerShell installer: `irm .../setup.ps1 | iex`.

---

## Development

### Prerequisites

- Rust 1.87+ (candle-core requires `unsigned_is_multiple_of`, stabilized in 1.87)
- Node.js 18+ (for the web UI — optional if you only need the server)

### Dev Mode

```bash
# Terminal 1: Rust server
cd server && cargo run -- --root /path/to/project

# Terminal 2: Vite dev server (proxies API to :8432)
npm run dev
```

### Building from Source

```bash
# Via setup script (easiest — handles everything)
bash setup.sh --from-source

# With CUDA GPU acceleration (auto-detects nvcc)
bash setup.sh --cuda

# Manual: server with semantic search
cargo build --release --manifest-path server/Cargo.toml --features semantic

# Manual: with CUDA
cargo build --release --manifest-path server/Cargo.toml --features semantic,cuda

# Web UI
npm ci && npm run build
```

Binary: `server/target/release/codescope`. Web UI: `dist/`.

### Testing

```bash
cargo test --manifest-path server/Cargo.toml
bash tests/integration.sh
cargo fmt --manifest-path server/Cargo.toml -- --check
cargo clippy --manifest-path server/Cargo.toml -- -D warnings
npx tsc --noEmit
```

### CI

PRs run lint + tests. Merges to master trigger AI-assisted versioning (via Claude Agent SDK + CodeScope's own MCP tools), cross-platform builds (6 targets), and release publishing.

## Architecture

```
server/src/
├── lib.rs         Library crate root, re-exports all modules
├── main.rs        CLI (clap derive), HTTP server (Axum), MCP mode entry
├── mcp.rs         MCP JSON-RPC server — 9 tools, stdio transport
├── mcp_http.rs    Streamable HTTP transport for MCP (POST/DELETE /mcp)
├── auth.rs        OAuth discovery (RFC 9728) and origin validation
├── api.rs         HTTP API handlers for the web UI
├── scan.rs        File discovery, module detection, dependency + import scanning
├── stubs.rs       Language-aware stub extraction (signatures without bodies)
├── fuzzy.rs       FZF v2 fuzzy matching (Smith-Waterman scoring, bitmask pre-filter)
├── budget.rs      Token budget allocation (water-fill algorithm)
├── tokenizer.rs   Token counting backends
├── types.rs       Shared types: RepoState, ServerState, scoring
├── init.rs        CLI subcommands: init, doctor
├── git.rs         Git blame, file history, changed files, churn analysis
├── watch.rs       File watcher for incremental live re-indexing
└── semantic.rs    BERT-based semantic code search (feature-gated)

src/               React 18 frontend (Vite + TypeScript)
├── App.tsx              Main shell, view routing, keyboard shortcuts
├── ActivityBar.tsx       Side navigation, theme toggle
├── TreeSidebar.tsx       File/module tree with context builder
├── SearchSidebar.tsx     Fuzzy search panel
├── FileList.tsx          Flat file listing with preview
├── FilePreview.tsx       Source code viewer
├── StatsDashboard.tsx    Language and file statistics
├── selectionActions.ts   Unified selection logic
├── treemap/              Squarified treemap (Canvas 2D)
└── depgraph/             3D force-directed graph (Three.js)
```

### Language Support

**Stub extraction** (function/class signatures):

- Brace-based: C, C++, C#, Java, Kotlin, Scala, Rust, Go, JavaScript, TypeScript, Swift, D, PowerShell, HLSL/GLSL/WGSL
- Indent-based: Python, Ruby
- Config: JSON, YAML, TOML, XML, INI

Brace tracking works best for Rust, TypeScript, and Python. C++ templates and heavily macro'd code can confuse it.

**Import tracing:** C/C++ (`#include`), Python (`import`/`from`), JS/TS (`import`/`require`), Rust (module system), Go, C# (`using`), PowerShell (`Import-Module`)

**Dependency scanning:** Cargo.toml, package.json, go.mod, CMakeLists.txt, .Build.cs

## License

MIT
