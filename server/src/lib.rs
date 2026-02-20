//! CodeScope — fast codebase indexer and search server.
//!
//! This crate provides the core library for CodeScope: a codebase indexer that scans
//! source files, extracts function/class signatures, builds import dependency graphs,
//! and exposes everything over MCP (Model Context Protocol) or HTTP.
//!
//! # Modules
//!
//! - [`scan`] — File discovery, module detection, import graph building
//! - [`types`] — Core types shared across the codebase
//! - [`stubs`] — Language-aware stub extraction (signatures without bodies)
//! - [`fuzzy`] — FZF v2 fuzzy matching with Smith-Waterman scoring
//! - [`budget`] — Token budget allocation via water-fill algorithm
//! - [`mcp`] — MCP JSON-RPC server (stdio transport)
//! - [`mcp_http`] — MCP Streamable HTTP transport
//! - [`api`] — HTTP API handlers for the web UI
//! - [`git`] — Git operations (blame, history, changed files, churn)
//! - [`watch`] — File watcher for incremental live re-indexing
//! - [`init`] — CLI subcommands: `init` and `doctor`
//! - [`auth`] — OAuth discovery and origin validation
//! - [`tokenizer`] — Pluggable token counting backends
//! - [`semantic`] — BERT-based semantic code search (feature-gated)

pub mod api;
pub mod auth;
pub mod budget;
pub mod fuzzy;
pub mod git;
pub mod init;
pub mod mcp;
pub mod mcp_http;
pub mod scan;
#[cfg(feature = "semantic")]
pub mod semantic;
pub mod stubs;
pub mod tokenizer;
pub mod types;
pub mod watch;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use tracing::{debug, error, info, warn};

use scan::*;
use types::*;

// ---------------------------------------------------------------------------
// Cross-platform path helpers
// ---------------------------------------------------------------------------

/// Platform-aware home directory: `HOME` on Unix, `USERPROFILE` on Windows.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok().map(PathBuf::from)
}

/// Platform-aware config directory: `~/.codescope` on Unix, `%APPDATA%/codescope` on Windows.
pub fn config_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        std::env::var("APPDATA").ok().map(|a| PathBuf::from(a).join("codescope"))
    } else {
        home_dir().map(|h| h.join(".codescope"))
    }
}

/// Platform-aware data directory: `~/.local/share/codescope` on Unix, `%LOCALAPPDATA%/codescope` on Windows.
pub fn data_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        std::env::var("LOCALAPPDATA")
            .or_else(|_| std::env::var("APPDATA"))
            .ok()
            .map(|a| PathBuf::from(a).join("codescope"))
    } else {
        home_dir().map(|h| h.join(".local/share/codescope"))
    }
}

/// Platform-aware cache directory: `$XDG_CACHE_HOME/codescope` or `~/.cache/codescope` on Unix,
/// `%LOCALAPPDATA%/codescope/cache` on Windows.
pub fn cache_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        std::env::var("LOCALAPPDATA").ok().map(|a| PathBuf::from(a).join("codescope").join("cache"))
    } else {
        std::env::var("XDG_CACHE_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|h| h.join(".cache")))
            .map(|c| c.join("codescope"))
    }
}

// ---------------------------------------------------------------------------
// .codescope.toml config loading
// ---------------------------------------------------------------------------

/// Known keys in `.codescope.toml` for config validation.
const KNOWN_CONFIG_KEYS: &[&str] =
    &["scan_dirs", "skip_dirs", "extensions", "noise_dirs", "semantic_model"];

/// Simple Levenshtein edit distance for typo suggestions.
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// Load scan configuration from `.codescope.toml` in the given project root.
///
/// Returns a [`ScanConfig`] with defaults merged with any overrides from the config file.
/// If the file doesn't exist or can't be parsed, returns defaults with a warning.
/// Unknown keys trigger a warning with a typo suggestion.
pub fn load_codescope_config(project_root: &std::path::Path) -> ScanConfig {
    let mut config = ScanConfig::new(project_root.to_path_buf());
    let config_path = project_root.join(".codescope.toml");

    if config_path.exists() {
        debug!("Loading .codescope.toml");
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(table) = content.parse::<toml::Table>() {
                // Validate keys — warn on unknown
                for key in table.keys() {
                    if !KNOWN_CONFIG_KEYS.contains(&key.as_str()) {
                        let suggestion =
                            KNOWN_CONFIG_KEYS.iter().min_by_key(|k| edit_distance(key, k)).unwrap();
                        let dist = edit_distance(key, suggestion);
                        if dist <= 3 {
                            warn!(
                                key = key.as_str(),
                                suggestion = *suggestion,
                                "Unknown key in .codescope.toml — did you mean '{suggestion}'?"
                            );
                        } else {
                            warn!(
                                key = key.as_str(),
                                "Unknown key in .codescope.toml (known keys: {})",
                                KNOWN_CONFIG_KEYS.join(", ")
                            );
                        }
                    }
                }

                // scan_dirs
                if let Some(dirs) = table.get("scan_dirs").and_then(|v| v.as_array()) {
                    config.scan_dirs =
                        dirs.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
                }

                // skip_dirs — merge with defaults
                if let Some(dirs) = table.get("skip_dirs").and_then(|v| v.as_array()) {
                    for d in dirs {
                        if let Some(s) = d.as_str() {
                            config.skip_dirs.insert(s.to_string());
                        }
                    }
                }

                // extensions
                if let Some(exts) = table.get("extensions").and_then(|v| v.as_array()) {
                    config.extensions =
                        exts.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
                }

                // noise_dirs — merge with defaults
                if let Some(dirs) = table.get("noise_dirs").and_then(|v| v.as_array()) {
                    for d in dirs {
                        if let Some(s) = d.as_str() {
                            config.noise_dirs.insert(s.to_string());
                        }
                    }
                }

                // semantic_model
                #[cfg(feature = "semantic")]
                if let Some(model) = table.get("semantic_model").and_then(|v| v.as_str()) {
                    config.semantic_model = Some(model.to_string());
                }
            } else {
                warn!("Failed to parse .codescope.toml");
            }
        }
    }

    config
}

// ---------------------------------------------------------------------------
// Scan a single repo and return RepoState
// ---------------------------------------------------------------------------

/// Scan a single repository and return its fully indexed [`RepoState`].
///
/// This is a convenience wrapper around [`scan_repo_with_options`] with semantic search disabled.
pub fn scan_repo(
    name: &str,
    root: &std::path::Path,
    _tok: &Arc<dyn tokenizer::Tokenizer>,
) -> RepoState {
    scan_repo_with_options(name, root, _tok, false)
}

/// Scan a single repository with configurable semantic search.
///
/// Performs a parallel directory walk, builds the search index, extracts module structure,
/// resolves import edges, and computes term document frequencies. If `enable_semantic` is true,
/// prepares the semantic index handle (actual embedding happens in a background thread).
pub fn scan_repo_with_options(
    name: &str,
    root: &std::path::Path,
    _tok: &Arc<dyn tokenizer::Tokenizer>,
    _enable_semantic: bool,
) -> RepoState {
    let config = load_codescope_config(root);

    info!(repo = name, root = %root.display(), "Scanning codebase");
    if !config.scan_dirs.is_empty() {
        debug!(repo = name, dirs = ?config.scan_dirs, "Scan dirs");
    }
    if !config.extensions.is_empty() {
        debug!(repo = name, exts = ?config.extensions, "Extension filter");
    }

    let start = Instant::now();

    let (all_files, manifest) = scan_files(&config);
    let file_count = all_files.len();
    let module_count = manifest.len();
    let deps = scan_deps(&config);
    let (search_files, search_modules) = build_search_index(&manifest);
    let import_graph = scan_imports(&all_files);
    let term_doc_freq = build_term_doc_freq(&all_files);

    #[cfg(feature = "semantic")]
    let semantic_index = std::sync::Arc::new(std::sync::RwLock::new(None));
    #[cfg(feature = "semantic")]
    let semantic_progress = std::sync::Arc::new(types::SemanticProgress::new());

    let scan_time_ms = start.elapsed().as_millis() as u64;

    info!(
        repo = name,
        files = file_count,
        modules = module_count,
        dep_modules = deps.len(),
        import_edges = import_graph.imports.len(),
        time_ms = scan_time_ms,
        "Scan complete"
    );

    RepoState {
        name: name.to_string(),
        root: root.to_path_buf(),
        config,
        all_files,
        manifest,
        deps,
        search_files,
        search_modules,
        import_graph,
        stub_cache: DashMap::new(),
        term_doc_freq,
        scan_time_ms,
        #[cfg(feature = "semantic")]
        semantic_index,
        #[cfg(feature = "semantic")]
        semantic_progress,
    }
}

/// Parse a `repos.toml` config file and return a list of `(name, root_path)` pairs.
pub fn parse_repos_toml(path: &std::path::Path) -> Vec<(String, PathBuf)> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            error!(path = %path.display(), error = %e, "Could not read config file");
            std::process::exit(1);
        }
    };
    let table: toml::Table = match content.parse() {
        Ok(t) => t,
        Err(e) => {
            error!(path = %path.display(), error = %e, "Could not parse config file");
            std::process::exit(1);
        }
    };

    let repos_table = match table.get("repos").and_then(|v| v.as_table()) {
        Some(t) => t,
        None => {
            error!("Config file missing [repos] section");
            std::process::exit(1);
        }
    };

    let mut repos = Vec::new();
    for (name, value) in repos_table {
        let root = value.get("root").and_then(|v| v.as_str()).unwrap_or_else(|| {
            error!(repo = name.as_str(), "Missing 'root' field in repos config");
            std::process::exit(1);
        });
        let root = PathBuf::from(root).canonicalize().unwrap_or_else(|e| {
            error!(repo = name.as_str(), path = root, error = %e, "Repository root not found");
            std::process::exit(1);
        });
        repos.push((name.clone(), root));
    }
    repos
}
