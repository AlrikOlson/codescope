# Changelog

All notable changes to CodeScope will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [0.9.2] - 2026-02-21

### Fixed
- Cargo.lock version synchronization in release process
- CUDA binary auto-detection in installation script

## [0.9.1] - 2026-02-21

### Fixed
- Hardened AI agent prompts for better reliability
- Improved CI workflow change detection to include testing when workflow files are modified

## [0.9.0] - 2026-02-20

### Added
- `--wait-semantic` CLI flag to block startup until semantic index is ready
- Comprehensive MCP HTTP transport test suite for protocol compliance
- Caching of codescope-server binary and BERT model across CI jobs
- E2E tests for semantic relevance with agent-crafted queries
- Structured outputs using Claude Agent SDK

### Fixed
- CUDA library linking issues (libcublas, libcurand, nvrtc)
- CI workflow concurrency and cancellation problems
- Agent tool usage constraints to prevent turn exhaustion
- E2E test reliability with increased turns and deterministic queries

### Changed
- Split e2e tests into separate workflow for better isolation
- Improved change detection in CI to include .github/scripts/
- Enhanced error logging and usage data collection

## [0.8.1] - 2026-02-20

### Changed
- Release v0.8.1

## [0.10.0] — 2026-02-20

### Added
- MCP tool annotations (`readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`) on all 9 tools per MCP spec 2025-11-25
- Cosine similarity relevance threshold (0.25) for semantic search — eliminates garbage results for irrelevant queries
- `tools.listChanged` capability declaration in MCP initialize response

### Changed
- **Soft errors:** Tool call errors no longer set `isError: true` in MCP responses, preventing Claude Code's sibling tool call cascade failure where one error kills all parallel calls
- **Reciprocal Rank Fusion (RRF):** Replaced ad-hoc 1.3x score boosting for hybrid keyword+semantic search with rank-based RRF (k=60), producing better-calibrated result rankings
- **Semantic previews:** Semantic-only search results now show actual matching code snippets instead of `// File:` header comments, plus file descriptions from the index
- **Grep minimum query:** Lowered from 2 characters to 1, allowing single-character searches
- **Module deps description:** Clarified that `cs_modules deps` returns package-manifest dependencies, not file-level imports (use `cs_imports` for that)

### Fixed
- `cs_git blame` on uncommitted/new files now returns a clear error message instead of a cryptic libgit2 "path does not exist in tree" error
- `cs_search` for gibberish/irrelevant queries no longer returns 30 low-quality semantic results

## [0.9.0] — 2026-02-20

### Added
- Library crate (`lib.rs`) — CodeScope can now be embedded as a Rust library
- CLI powered by `clap` with derive macros, auto-generated `--help`, and error messages
- Shell completions: `codescope-server completions bash/zsh/fish/powershell`
- Structured logging via `tracing` — control verbosity with `RUST_LOG` env var
- Graceful shutdown on SIGINT/SIGTERM with connection draining
- `/health` endpoint returning server status, version, repo count, and uptime
- `.codescope.toml` config validation with typo suggestions for unknown keys
- Unit tests for fuzzy matching, path validation, budget allocation, stub extraction, and date conversion
- HTTP request/response tracing via `tower-http::TraceLayer`

### Changed
- CLI parsing migrated from hand-rolled args to `clap` derive (same flags, better UX)
- All logging migrated from `eprintln!` to structured `tracing` macros
- MCP tool count in README corrected from 19 to 9 (consolidated in v0.8.0)

### Fixed
- README MCP tools table now reflects the actual 9 consolidated tools

## [0.8.0] - 2026-02-20

### Added
- MCP Streamable HTTP transport for HTTP-based MCP clients (`server/src/mcp_http.rs`)
- OAuth 2.0 discovery and spec-compliant authorization support (`server/src/auth.rs`)
- PowerShell installer for Windows and WSL environments (`server/setup.ps1`)
- Dedicated `release.yml` CI/CD workflow split from `ci.yml`

### Changed
- Consolidated MCP tools from 19 → 4 with unified semantic search interface (`server/src/mcp.rs`)
- `main.rs` updated to register HTTP transport routes and OAuth endpoints
- AI agent blocks built-in Read/Glob/Grep tools to enforce CodeScope MCP usage

### Fixed
- `setup.sh` `dirname` resolution error; improved WSL and PowerShell environment detection
- AI release agent misclassifying existing features as new additions
- AI doc-sync performance; pinned checkout refs; blocked runaway sub-agent spawning

## [0.7.0] - 2026-02-20

### Added
- Centralized semantic cache at `~/.cache/codescope/semantic/{repo-identity}/semantic.cache` with automatic migration from legacy in-repo `.codescope/semantic.cache`
- Path context breadcrumbs prepended to each embedding chunk for improved semantic disambiguation
- Relevance reranking in `cs_semantic_search`: 6× oversampling with `adjusted_score()` path-based signals
- `SEMANTIC_SKIP_DIRS` filter excludes `ThirdParty`, `External`, `Intermediate`, `Deploy` from embedding
- Device (CPU/GPU), batch progress %, and chunk count displayed in `cs_status` semantic output
- `--semantic` flag for `codescope-server init` subcommand

### Changed
- Semantic cache format version 1 → 2 (includes path context; old caches are discarded and rebuilt automatically)
- `cs_semantic_search` errors now include index build progress (device, batch count) when index is still loading
- `is_annotation_or_macro()` in `stubs.rs` made public

### Fixed
- Resolved Clippy lints for Rust 1.93 CI compatibility

## [0.6.2] - 2026-02-20

### Changed
- CI workflow now uses path-based change detection to skip unnecessary lint, test, and release jobs when unrelated files are modified

## [0.6.1] - 2026-02-20

### Added
- AI-powered documentation sync step in the release pipeline: automatically detects and fixes factual inaccuracies in `README.md` and `CONTRIBUTING.md` at release time using the Claude Agent SDK and CodeScope MCP tools (`.github/scripts/ai-docs-sync.mjs`, `.github/scripts/lib/docs.mjs`)

## [0.6.0] - 2026-02-20

### Added
- `cs_blame`: git blame for any file with optional line-range scoping
- `cs_file_history`: recent commits that touched a specific file
- `cs_changed_since`: files changed since a given commit, branch, or tag
- `cs_hot_files`: churn ranking of most frequently modified files over a time window
- `cs_session_info`: files read and tokens served in the current MCP session
- `cs_impact`: transitive reverse-dependency BFS analysis up to configurable depth
- `cs_status`: indexed repo overview with file counts, language breakdown, and scan time
- `cs_rescan`: re-index one or all repos at runtime without server restart
- `cs_add_repo`: dynamically add a repository to the live index at runtime

### Fixed
- `cs_grep` "all" mode: multi-term queries now require ALL terms per matching line (was OR-only)
- `cs_find`: added `match_mode` parameter with `require_all_terms` post-filter
- `cs_grep` context output: proper range merging with `---` separators between non-contiguous blocks
- `cs_grep` `files_only` output mode now emits correct per-file summary lines
- Stub extraction: multi-line C++ class declarations with inheritance continuations no longer collapsed into function stubs
- Stub extraction: constructor initializer lists no longer misidentified as structural scopes

### Changed
- MCP protocol version updated to `2025-06-18`
- `KNOWN_EXTS` in `preprocess_search_query` expanded to cover more file types

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
