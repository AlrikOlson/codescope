# Contributing to CodeScope

Thanks for your interest in contributing. This document covers how to get started.

## Development Setup

**Prerequisites:**

- Rust 1.87+ (`rustup` recommended — candle-core requires 1.87)
- Node.js 18+ (for the web UI, optional)

**Clone and build:**

```bash
git clone https://github.com/AlrikOlson/codescope.git
cd codescope/server
cargo build --release
```

**Run the web UI in dev mode:**

```bash
# Terminal 1: backend
cd server && cargo run -- --root /path/to/project

# Terminal 2: frontend (hot-reload, proxies to :8432)
npm install && npm run dev
```

## Quality Gates

All PRs must pass these checks (run locally before pushing):

```bash
# Unit tests
cargo test --manifest-path server/Cargo.toml

# Formatting
cargo fmt --manifest-path server/Cargo.toml -- --check

# Linting (warnings are errors)
cargo clippy --manifest-path server/Cargo.toml -- -D warnings

# TypeScript
npx tsc --noEmit

# Integration tests (requires built server binary)
bash tests/integration.sh
```

Set `RUST_LOG=codescope=debug` for verbose server output when debugging.

CI runs all of these automatically on pull requests that change code.

## Pull Requests

- Keep PRs small and focused. One feature or fix per PR.
- Reference related issues in the PR description.
- Use [conventional commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`.
- Add tests for new functionality when possible.
- Update documentation if your change affects user-facing behavior.

## Architecture

The backend lives in `server/src/` with these modules:

| Module | Purpose |
|--------|-------|
| `lib.rs` | Library crate root, re-exports all modules |
| `main.rs` | CLI shell (clap derive), HTTP server (Axum), MCP entry |
| `mcp.rs` | MCP JSON-RPC server, 9 consolidated tools (stdio + HTTP) |
| `mcp_http.rs` | Streamable HTTP transport for MCP |
| `auth.rs` | OAuth discovery (RFC 9728) and origin validation |
| `api.rs` | HTTP API handlers |
| `scan.rs` | File discovery, module detection, dependency + import scanning |
| `stubs.rs` | Structural stub extraction (signatures without bodies) |
| `fuzzy.rs` | FZF v2 fuzzy matching |
| `budget.rs` | Token budget allocation |
| `tokenizer.rs` | Token counting (bytes-estimate or tiktoken) |
| `types.rs` | Shared types and helpers |
| `init.rs` | `init` and `doctor` subcommands |
| `git.rs` | Git operations: blame, file history, changed files, churn analysis |
| `watch.rs` | File watcher for incremental live re-indexing |
| `semantic.rs` | Semantic search via BERT embeddings |

Key dependencies: `clap` (CLI parsing), `tracing` (structured logging), `axum` (HTTP), `git2` (libgit2), `candle` (BERT embeddings).

The frontend is a React 18 + TypeScript app in `src/`, built with Vite.

## Code Style

- Rust: `rustfmt` with the project's `rustfmt.toml`, `clippy` with `-D warnings`.
- TypeScript: strict mode, no explicit linter config (tsc catches issues).
- Keep functions under 30 lines when practical.
- Guard clauses over nested conditionals.

## Reporting Bugs

Open a [GitHub issue](https://github.com/AlrikOlson/codescope/issues) with:

- What you expected to happen
- What actually happened
- Steps to reproduce
- CodeScope version (`codescope --version`)
