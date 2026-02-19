# Changelog

All notable changes to CodeScope will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [0.3.0] - 2026-02-18

Initial public release.

### Features

- **MCP server** with 14 tools for Claude Code integration: search, read, navigate, analyze
- **Web UI** with file browser, full-text search, treemap visualization, and 3D dependency graph
- **Multi-repo support** -- index and search across multiple repositories simultaneously
- **Impact analysis** -- trace import dependency chains to answer "what breaks if I change this?"
- **Token budget allocation** -- water-fill algorithm to fit file contents into LLM context windows
- **Structural stubs** -- extract function/class signatures without bodies for compact code summaries
- **FZF v2 fuzzy matching** -- Smith-Waterman scoring with bitmask pre-filtering
- **Cross-repo import resolution** -- trace imports that cross repository boundaries
- **18+ language support** -- stub extraction and import tracing for Rust, TypeScript, Python, Go, C/C++, C#, Java, Kotlin, Swift, Ruby, PHP, Lua, Zig, PowerShell, and more
- **Dependency scanning** -- parse Cargo.toml, package.json, go.mod, .csproj
- **CLI subcommands** -- `init` for auto-setup, `doctor` for diagnostics
- **Configuration** -- `.codescope.toml` for per-project scan customization
