//! CodeScope — fast codebase indexer and search server.

mod api;
mod budget;
mod fuzzy;
mod init;
mod mcp;
mod scan;
#[cfg(feature = "semantic")]
mod semantic;
mod stubs;
mod tokenizer;
mod types;

use axum::{
    routing::{get, post},
    Router,
};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

use api::*;
use mcp::run_mcp;
use scan::*;
use types::*;

// ---------------------------------------------------------------------------
// .codescope.toml config loading
// ---------------------------------------------------------------------------

pub(crate) fn load_codescope_config(project_root: &std::path::Path) -> ScanConfig {
    let mut config = ScanConfig::new(project_root.to_path_buf());
    let config_path = project_root.join(".codescope.toml");

    if config_path.exists() {
        eprintln!("  Loading .codescope.toml...");
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(table) = content.parse::<toml::Table>() {
                // scan_dirs
                if let Some(dirs) = table.get("scan_dirs").and_then(|v| v.as_array()) {
                    config.scan_dirs = dirs
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
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
                    config.extensions = exts
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                }

                // noise_dirs — merge with defaults
                if let Some(dirs) = table.get("noise_dirs").and_then(|v| v.as_array()) {
                    for d in dirs {
                        if let Some(s) = d.as_str() {
                            config.noise_dirs.insert(s.to_string());
                        }
                    }
                }
            } else {
                eprintln!("  Warning: Failed to parse .codescope.toml");
            }
        }
    }

    config
}

// ---------------------------------------------------------------------------
// Scan a single repo and return RepoState
// ---------------------------------------------------------------------------

pub fn scan_repo(name: &str, root: &std::path::Path, _tok: &Arc<dyn tokenizer::Tokenizer>) -> RepoState {
    scan_repo_with_options(name, root, _tok, false)
}

pub fn scan_repo_with_options(
    name: &str,
    root: &std::path::Path,
    _tok: &Arc<dyn tokenizer::Tokenizer>,
    _enable_semantic: bool,
) -> RepoState {
    let config = load_codescope_config(root);

    eprintln!(
        "  [{name}] Scanning codebase at {}...",
        root.display()
    );
    if !config.scan_dirs.is_empty() {
        eprintln!("  [{name}] Scan dirs: {:?}", config.scan_dirs);
    }
    if !config.extensions.is_empty() {
        eprintln!("  [{name}] Extensions: {:?}", config.extensions);
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
    let semantic_index = if _enable_semantic {
        eprintln!("  [{name}] Building semantic index...");
        let sem_start = Instant::now();
        let idx = semantic::build_semantic_index(&all_files);
        if let Some(ref idx) = idx {
            eprintln!(
                "  [{name}] Semantic index: {} chunks ({}ms)",
                idx.chunk_meta.len(),
                sem_start.elapsed().as_millis()
            );
        }
        idx
    } else {
        None
    };

    let scan_time_ms = start.elapsed().as_millis() as u64;

    eprintln!(
        "  [{name}] Scanned {file_count} files -> {module_count} modules, {} dep modules, {} import edges ({scan_time_ms}ms)",
        deps.len(),
        import_graph.imports.len(),
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
        stub_cache: dashmap::DashMap::new(),
        term_doc_freq,
        scan_time_ms,
        #[cfg(feature = "semantic")]
        semantic_index,
    }
}

// ---------------------------------------------------------------------------
// Parse repos.toml config file
// ---------------------------------------------------------------------------

fn parse_repos_toml(path: &std::path::Path) -> Vec<(String, PathBuf)> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: Could not read config file {}: {e}", path.display());
            std::process::exit(1);
        }
    };
    let table: toml::Table = match content.parse() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error: Could not parse {}: {e}", path.display());
            std::process::exit(1);
        }
    };

    let repos_table = match table.get("repos").and_then(|v| v.as_table()) {
        Some(t) => t,
        None => {
            eprintln!("Error: Config file missing [repos] section");
            std::process::exit(1);
        }
    };

    let mut repos = Vec::new();
    for (name, value) in repos_table {
        let root = value
            .get("root")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                eprintln!("Error: repos.{name} missing 'root' field");
                std::process::exit(1);
            });
        let root = PathBuf::from(root).canonicalize().unwrap_or_else(|e| {
            eprintln!("Error: repos.{name} root '{}' not found: {e}", root);
            std::process::exit(1);
        });
        repos.push((name.clone(), root));
    }
    repos
}

// ---------------------------------------------------------------------------
// CLI help
// ---------------------------------------------------------------------------

fn print_help() {
    let version = env!("CARGO_PKG_VERSION");
    eprintln!("codescope-server {version}");
    eprintln!("Fast codebase indexer and search server");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  codescope-server [COMMAND] [OPTIONS]");
    eprintln!();
    eprintln!("COMMANDS:");
    eprintln!("  init [PATH]           Initialize CodeScope in a project (generates config files)");
    eprintln!("  doctor [PATH]         Check project setup and diagnose issues");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("  --root <PATH>         Project root directory (default: current directory)");
    eprintln!("  --repo <NAME=PATH>    Named repository (repeatable for multi-repo)");
    eprintln!("  --config <PATH>       Load repos from a TOML config file");
    eprintln!("  --mcp                 Run as MCP stdio server (for Claude Code)");
    eprintln!("  --dist <PATH>         Path to web UI dist directory");
    eprintln!("  --tokenizer <NAME>    Token counter: bytes-estimate (default) or tiktoken");
    #[cfg(feature = "semantic")]
    eprintln!("  --semantic            Enable semantic code search (downloads ML model on first use)");
    eprintln!("  --help                Show this help message");
    eprintln!("  --version             Show version");
    eprintln!();
    eprintln!("MULTI-REPO:");
    eprintln!("  codescope-server --mcp --repo engine=/path/to/engine --repo game=/path/to/game");
    eprintln!("  codescope-server --mcp --config ~/.codescope/repos.toml");
    eprintln!();
    eprintln!("ENVIRONMENT:");
    eprintln!("  PORT                  HTTP server port (default: auto-scan 8432-8441)");
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }

    if args.iter().any(|a| a == "--version" || a == "-V") {
        eprintln!("codescope-server {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // Check for subcommands before flag-based parsing
    if args.get(1).map(|s| s.as_str()) == Some("init") {
        std::process::exit(init::run_init(&args[1..]));
    }
    if args.get(1).map(|s| s.as_str()) == Some("doctor") {
        std::process::exit(init::run_doctor(&args[1..]));
    }

    let mcp_mode = args.iter().any(|a| a == "--mcp");

    // Tokenizer: --tokenizer flag or default bytes-estimate
    let tokenizer_name = args
        .iter()
        .position(|a| a == "--tokenizer")
        .and_then(|pos| args.get(pos + 1))
        .map(|s| s.as_str())
        .unwrap_or("bytes-estimate");

    let tok = tokenizer::create_tokenizer(tokenizer_name);

    eprintln!("\n  Tokenizer: {}", tok.name());

    // ---------------------------------------------------------------------------
    // Determine repo list from CLI args
    // ---------------------------------------------------------------------------

    let mut repo_specs: Vec<(String, PathBuf)> = Vec::new();

    // --repo name=/path flags (repeatable)
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--repo" {
            if let Some(spec) = args.get(i + 1) {
                if let Some((name, path)) = spec.split_once('=') {
                    let root = PathBuf::from(path).canonicalize().unwrap_or_else(|e| {
                        eprintln!("Error: --repo {name}={path} not found: {e}");
                        std::process::exit(1);
                    });
                    repo_specs.push((name.to_string(), root));
                } else {
                    eprintln!("Error: --repo requires NAME=PATH format (e.g. --repo engine=/path/to/engine)");
                    std::process::exit(1);
                }
                i += 2;
            } else {
                eprintln!("Error: --repo requires a NAME=PATH argument");
                std::process::exit(1);
            }
        } else {
            i += 1;
        }
    }

    // --config file
    if let Some(pos) = args.iter().position(|a| a == "--config") {
        let config_path = args.get(pos + 1).unwrap_or_else(|| {
            eprintln!("Error: --config requires a path argument");
            std::process::exit(1);
        });
        let parsed = parse_repos_toml(std::path::Path::new(config_path));
        repo_specs.extend(parsed);
    }

    // Fallback: --root or cwd (single repo, backwards compat)
    if repo_specs.is_empty() {
        let project_root = if let Some(pos) = args.iter().position(|a| a == "--root") {
            match args.get(pos + 1) {
                Some(path) => PathBuf::from(path),
                None => {
                    eprintln!("Error: --root requires a path argument");
                    std::process::exit(1);
                }
            }
        } else {
            // Check global config fallback
            let global_config = std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".codescope/repos.toml"))
                .unwrap_or_default();
            if global_config.exists() && mcp_mode {
                let parsed = parse_repos_toml(&global_config);
                repo_specs.extend(parsed);
                // Fall through — repo_specs now populated
                PathBuf::new() // won't be used
            } else {
                std::env::current_dir().unwrap_or_else(|_| {
                    eprintln!("Error: Could not determine current directory. Use --root <path>");
                    std::process::exit(1);
                })
            }
        };

        if repo_specs.is_empty() {
            let project_root = project_root.canonicalize().unwrap_or(project_root);
            let name = project_root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("default")
                .to_string();
            repo_specs.push((name, project_root));
        }
    }

    // ---------------------------------------------------------------------------
    // Semantic search: opt-in at runtime via --semantic flag
    // ---------------------------------------------------------------------------

    #[cfg(feature = "semantic")]
    let enable_semantic = args.iter().any(|a| a == "--semantic");
    #[cfg(not(feature = "semantic"))]
    let enable_semantic = false;

    if args.iter().any(|a| a == "--semantic") && !cfg!(feature = "semantic") {
        eprintln!("  Warning: --semantic flag ignored (binary not compiled with 'semantic' feature)");
        eprintln!("  Recompile with: cargo build --release --features semantic");
    }

    // ---------------------------------------------------------------------------
    // Scan all repos (parallel via rayon)
    // ---------------------------------------------------------------------------

    let tok_ref = &tok;
    let repo_states: Vec<RepoState> = repo_specs
        .par_iter()
        .map(|(name, root)| scan_repo_with_options(name, root, tok_ref, enable_semantic))
        .collect();

    let mut repos = BTreeMap::new();
    let default_repo = if repo_states.len() == 1 {
        Some(repo_states[0].name.clone())
    } else {
        None
    };
    for repo in repo_states {
        repos.insert(repo.name.clone(), repo);
    }

    // Build cross-repo import edges
    let cross_repo_edges = scan::resolve_cross_repo_imports(&repos);

    let total_files: usize = repos.values().map(|r| r.all_files.len()).sum();
    let total_modules: usize = repos.values().map(|r| r.manifest.len()).sum();
    eprintln!(
        "\n  Total: {} files, {} modules across {} repo(s)\n",
        total_files,
        total_modules,
        repos.len()
    );

    // Build unified ServerState (shared by MCP and HTTP modes)
    let server_state = ServerState {
        repos,
        default_repo,
        cross_repo_edges,
        tokenizer: tok,
    };
    let state = Arc::new(RwLock::new(server_state));

    if mcp_mode {
        run_mcp(state);
        return;
    }

    // HTTP mode — build pre-computed JSON cache from default repo
    let cache = {
        let s = state.read().unwrap();
        let repo = s.default_repo();
        let tree = build_tree(&repo.manifest);
        Arc::new(HttpCache {
            tree_json: serde_json::to_string(&tree).unwrap(),
            manifest_json: serde_json::to_string(&repo.manifest).unwrap(),
            deps_json: serde_json::to_string(&repo.deps).unwrap(),
        })
    };

    let ctx = AppContext { state, cache };

    // Resolve dist dir: --dist flag, then cwd/dist, then ~/.local/share/codescope/dist
    let dist_dir = if let Some(pos) = args.iter().position(|a| a == "--dist") {
        match args.get(pos + 1) {
            Some(path) => PathBuf::from(path),
            None => {
                eprintln!("Error: --dist requires a path argument");
                std::process::exit(1);
            }
        }
    } else {
        let cwd = std::env::current_dir().unwrap();
        let home_dist = std::env::var("HOME")
            .map(|h| PathBuf::from(h).join(".local/share/codescope/dist"))
            .unwrap_or_default();
        let candidates = [cwd.join("dist"), cwd.join("../dist"), home_dist];
        candidates
            .into_iter()
            .find(|p| p.join("index.html").exists())
            .unwrap_or_else(|| {
                eprintln!("  Warning: No dist/ directory found. Run setup.sh with Node.js to build the web UI.");
                cwd.join("dist")
            })
    };

    let index_html = dist_dir.join("index.html");

    // API routes take priority, then fall back to static files from dist/
    let app = Router::new()
        .route("/api/tree", get(api_tree))
        .route("/api/manifest", get(api_manifest))
        .route("/api/deps", get(api_deps))
        .route("/api/file", get(api_file))
        .route("/api/files", post(api_files))
        .route("/api/grep", get(api_grep))
        .route("/api/search", get(api_search))
        .route("/api/find", get(api_find))
        .route("/api/context", post(api_context))
        .route("/api/imports", get(api_imports))
        .fallback_service(
            ServeDir::new(&dist_dir).not_found_service(ServeFile::new(&index_html)),
        )
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .with_state(ctx);

    let explicit_port: Option<u16> = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok());

    let listener = if let Some(port) = explicit_port {
        // User chose a port explicitly — fail hard if busy
        tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
            .await
            .unwrap_or_else(|e| {
                eprintln!("Error: Could not bind to port {port}: {e}");
                eprintln!("  PORT={port} was set explicitly. Choose a different port.");
                std::process::exit(1);
            })
    } else {
        // Auto-scan: try 8432..=8441
        const BASE: u16 = 8432;
        const RANGE: u16 = 10;
        let mut found = None;
        for port in BASE..BASE + RANGE {
            match tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await {
                Ok(l) => {
                    found = Some(l);
                    break;
                }
                Err(_) => continue,
            }
        }
        found.unwrap_or_else(|| {
            eprintln!("Error: No free port found in {BASE}..{}", BASE + RANGE - 1);
            eprintln!("  Try: PORT=<port> codescope-server");
            std::process::exit(1);
        })
    };

    let port = listener.local_addr().unwrap().port();

    eprintln!("  Serving UI from {}", dist_dir.display());
    eprintln!("  http://localhost:{port}");
    eprintln!("CODESCOPE_PORT={port}");
    axum::serve(listener, app).await.unwrap();
}
