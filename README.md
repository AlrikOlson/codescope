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

This installs the Rust toolchain (if needed), builds the binary, and puts `codescope-server` and `codescope-init` in `~/.local/bin/`.

Then, in any project you want to index:

```bash
cd /path/to/your/project
codescope-init
```

This creates a `.mcp.json` that Claude Code picks up automatically. Open Claude Code in that directory and the `cs_*` tools are available.

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

## HTTP Mode

Without `--mcp`, the server starts an HTTP server with a web UI:

```bash
cd codescope
npm install && npm run build
codescope-server --root /path/to/project --dist ./dist
```

Opens at `http://localhost:8432`. Features a file browser, treemap visualization, dependency graph, and full-text search.

## Configuration

Drop a `.codescope.toml` in your project root to customize scanning:

```toml
# Only scan these directories (default: scan everything)
scan_dirs = ["src", "lib"]

# Skip these directories (merged with built-in defaults)
skip_dirs = ["vendor", "generated"]

# Only index these extensions (default: common source extensions)
extensions = [".rs", ".ts", ".go", ".py"]

# Treat these as noise/library directories (lower priority in search)
noise_dirs = ["third_party"]
```

## Building from Source

Requires Rust 1.75+.

```bash
cd server
cargo build --release
```

Binary lands at `server/target/release/codescope-server`.

## License

MIT
