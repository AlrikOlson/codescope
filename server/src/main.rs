//! CodeScope binary — thin CLI shell over the [`codescope_server`] library crate.

use axum::{
    routing::{get, post},
    Router,
};
use clap::{CommandFactory, Parser, Subcommand};
use dashmap::DashMap;
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tracing::{debug, error, info, warn};

use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use codescope_server::api::*;
use codescope_server::mcp::run_mcp;
use codescope_server::scan::*;
use codescope_server::types::*;
use codescope_server::{config_dir, data_dir, parse_repos_toml, scan_repo_with_options, tokenizer};

// ---------------------------------------------------------------------------
// CLI definition (clap derive)
// ---------------------------------------------------------------------------

/// Fast codebase indexer and search server — MCP server for Claude Code and standalone web UI.
#[derive(Parser)]
#[command(name = "codescope", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Project root directory (default: current directory)
    #[arg(long)]
    root: Option<PathBuf>,

    /// Named repository (repeatable, format: NAME=PATH)
    #[arg(long = "repo", value_name = "NAME=PATH")]
    repos: Vec<String>,

    /// Load repos from a TOML config file
    #[arg(long)]
    config: Option<PathBuf>,

    /// Run as MCP stdio server (for Claude Code)
    #[arg(long)]
    mcp: bool,

    /// Path to web UI dist directory
    #[arg(long)]
    dist: Option<PathBuf>,

    /// Token counter: bytes-estimate (default) or tiktoken
    #[arg(long, default_value = "bytes-estimate")]
    tokenizer: String,

    /// Disable semantic code search (enabled by default)
    #[arg(long)]
    no_semantic: bool,

    /// Embedding model: minilm (default), codebert, starencoder, or HuggingFace model ID
    #[arg(long)]
    semantic_model: Option<String>,

    /// Block startup until semantic index is fully loaded (useful for CI)
    #[arg(long)]
    wait_semantic: bool,

    /// Enable OAuth with authorization server URL
    #[arg(long)]
    auth_issuer: Option<String>,

    /// Comma-separated allowed Origin headers for MCP HTTP transport
    #[arg(long)]
    allowed_origins: Option<String>,

    /// Bind to 0.0.0.0 instead of 127.0.0.1 (localhost)
    #[arg(long)]
    bind_all: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize CodeScope in a project (generates config files)
    Init {
        /// Project path (default: current directory)
        path: Option<PathBuf>,

        /// Add to global config (~/.codescope/repos.toml) instead of local
        #[arg(long)]
        global: bool,

        /// Pre-build semantic index cache during init
        #[arg(long)]
        semantic: bool,
    },
    /// Check project setup and diagnose issues
    Doctor {
        /// Project path (default: current directory)
        path: Option<PathBuf>,
    },
    /// Launch the web UI in a browser
    Web {
        /// Project path (default: current directory)
        path: Option<PathBuf>,
    },
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

// ---------------------------------------------------------------------------
// Graceful shutdown signal
// ---------------------------------------------------------------------------

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to register SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => info!("Received SIGINT, shutting down..."),
            _ = sigterm.recv() => info!("Received SIGTERM, shutting down..."),
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await.expect("failed to listen for Ctrl+C");
        info!("Received Ctrl+C, shutting down...");
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // Initialize structured logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("codescope=info".parse().unwrap()),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    // Handle subcommands
    if let Some(command) = &cli.command {
        match command {
            Commands::Init { path, global, semantic } => {
                // Build args vector matching init::run_init's expected format
                let mut args = vec!["init".to_string()];
                if let Some(p) = path {
                    args.push(p.display().to_string());
                }
                if *global {
                    args.push("--global".to_string());
                }
                if *semantic {
                    args.push("--semantic".to_string());
                }
                std::process::exit(codescope_server::init::run_init(&args));
            }
            Commands::Doctor { path } => {
                let mut args = vec!["doctor".to_string()];
                if let Some(p) = path {
                    args.push(p.display().to_string());
                }
                std::process::exit(codescope_server::init::run_doctor(&args));
            }
            Commands::Web { path } => {
                let root = path.clone().unwrap_or_else(|| std::env::current_dir().unwrap());
                let root = root.canonicalize().unwrap_or_else(|e| {
                    eprintln!("Error: Path '{}' not found: {}", root.display(), e);
                    std::process::exit(1);
                });

                // Resolve dist directory
                let dist_dir = data_dir()
                    .map(|d| d.join("dist"))
                    .filter(|d| d.join("index.html").exists())
                    .unwrap_or_else(|| {
                        eprintln!("Error: Web UI not installed.");
                        eprintln!("  Re-run setup.sh with Node.js available to build the web UI.");
                        std::process::exit(1);
                    });

                eprintln!("Project: {}", root.display());

                // Build the command to re-exec ourselves as a server
                let exe = std::env::current_exe().unwrap();
                let status = std::process::Command::new(&exe)
                    .arg("--root")
                    .arg(&root)
                    .arg("--dist")
                    .arg(&dist_dir)
                    .status()
                    .unwrap_or_else(|e| {
                        eprintln!("Error: Failed to start server: {}", e);
                        std::process::exit(1);
                    });
                std::process::exit(status.code().unwrap_or(1));
            }
            Commands::Completions { shell } => {
                clap_complete::generate(
                    *shell,
                    &mut Cli::command(),
                    "codescope",
                    &mut std::io::stdout(),
                );
                return;
            }
        }
    }

    // Tokenizer
    let tok = tokenizer::create_tokenizer(&cli.tokenizer);
    info!(tokenizer = tok.name(), "Initialized tokenizer");

    // ---------------------------------------------------------------------------
    // Determine repo list from CLI args
    // ---------------------------------------------------------------------------

    let mut repo_specs: Vec<(String, PathBuf)> = Vec::new();

    // --repo name=/path flags (repeatable)
    for spec in &cli.repos {
        if let Some((name, path)) = spec.split_once('=') {
            let root = PathBuf::from(path).canonicalize().unwrap_or_else(|e| {
                error!(repo = name, path = path, error = %e, "Repository path not found");
                std::process::exit(1);
            });
            repo_specs.push((name.to_string(), root));
        } else {
            error!(spec = spec.as_str(), "Invalid --repo format, expected NAME=PATH");
            std::process::exit(1);
        }
    }

    // --config file
    if let Some(config_path) = &cli.config {
        let parsed = parse_repos_toml(config_path);
        repo_specs.extend(parsed);
    }

    // Fallback: --root or cwd (single repo, backwards compat)
    if repo_specs.is_empty() {
        let project_root = if let Some(root) = &cli.root {
            root.clone()
        } else {
            // Check global config fallback
            let global_config = config_dir().map(|d| d.join("repos.toml")).unwrap_or_default();
            if global_config.exists() && cli.mcp {
                let parsed = parse_repos_toml(&global_config);
                repo_specs.extend(parsed);
                PathBuf::new() // won't be used
            } else {
                std::env::current_dir().unwrap_or_else(|_| {
                    error!("Could not determine current directory. Use --root <path>");
                    std::process::exit(1);
                })
            }
        };

        if repo_specs.is_empty() {
            let project_root = project_root.canonicalize().unwrap_or(project_root);
            let name =
                project_root.file_name().and_then(|n| n.to_str()).unwrap_or("default").to_string();
            repo_specs.push((name, project_root));
        }
    }

    // ---------------------------------------------------------------------------
    // Semantic search: on by default, --no-semantic to disable
    // ---------------------------------------------------------------------------

    #[cfg(feature = "semantic")]
    let enable_semantic = !cli.no_semantic;
    #[cfg(not(feature = "semantic"))]
    let enable_semantic = false;

    #[cfg(feature = "semantic")]
    let semantic_model: Option<String> = cli.semantic_model.clone();
    #[cfg(not(feature = "semantic"))]
    let _semantic_model: Option<String> = None;

    if cli.no_semantic && !cfg!(feature = "semantic") {
        warn!("--no-semantic flag ignored — this binary does not include semantic search");
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
    let default_repo =
        if repo_states.len() == 1 { Some(repo_states[0].name.clone()) } else { None };
    for repo in repo_states {
        repos.insert(repo.name.clone(), repo);
    }

    // Build cross-repo import edges
    let cross_repo_edges = codescope_server::scan::resolve_cross_repo_imports(&repos);

    let total_files: usize = repos.values().map(|r| r.all_files.len()).sum();
    let total_modules: usize = repos.values().map(|r| r.manifest.len()).sum();
    info!(files = total_files, modules = total_modules, repos = repos.len(), "Scan complete");

    // Build unified ServerState (shared by MCP and HTTP modes)
    let server_state = ServerState {
        repos,
        default_repo,
        cross_repo_edges,
        tokenizer: tok,
        #[cfg(feature = "semantic")]
        semantic_enabled: enable_semantic,
        #[cfg(feature = "semantic")]
        semantic_model: semantic_model.clone(),
    };
    let state = Arc::new(RwLock::new(server_state));

    // Spawn semantic indexing — background by default, blocking with --wait-semantic
    #[cfg(feature = "semantic")]
    if enable_semantic {
        let state_bg = Arc::clone(&state);
        let sem_model = semantic_model.clone();
        let wait = cli.wait_semantic;
        let handle = std::thread::spawn(move || {
            let s = state_bg.read().unwrap();
            type SemWork = (
                String,
                PathBuf,
                Vec<ScannedFile>,
                std::sync::Arc<std::sync::RwLock<Option<SemanticIndex>>>,
                std::sync::Arc<SemanticProgress>,
            );
            let work: Vec<SemWork> = s
                .repos
                .values()
                .map(|r| {
                    (
                        r.name.clone(),
                        r.root.clone(),
                        r.all_files.clone(),
                        std::sync::Arc::clone(&r.semantic_index),
                        std::sync::Arc::clone(&r.semantic_progress),
                    )
                })
                .collect();
            drop(s);

            for (name, root, files, sem_handle, progress) in work {
                info!(repo = name.as_str(), "Building semantic index...");
                let sem_start = std::time::Instant::now();
                if let Some(idx) = codescope_server::semantic::build_semantic_index(
                    &files,
                    sem_model.as_deref(),
                    &progress,
                    &root,
                ) {
                    info!(
                        repo = name.as_str(),
                        chunks = idx.chunk_meta.len(),
                        time_ms = sem_start.elapsed().as_millis() as u64,
                        "Semantic index ready"
                    );
                    *sem_handle.write().unwrap() = Some(idx);
                }
            }
        });
        if wait {
            info!("--wait-semantic: blocking until semantic index is ready");
            handle.join().expect("semantic indexing thread panicked");
            info!("Semantic index loaded — starting server");
        }
    }

    // Start file watcher for incremental live re-indexing
    let _watcher = codescope_server::watch::start_watcher(Arc::clone(&state));

    if cli.mcp {
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

    let ctx = AppContext { state: state.clone(), cache, start_time: std::time::Instant::now() };

    // Resolve dist dir: --dist flag, then cwd/dist, then ~/.local/share/codescope/dist
    let dist_dir = if let Some(path) = &cli.dist {
        path.clone()
    } else {
        let cwd = std::env::current_dir().unwrap();
        let home_dist = data_dir().map(|d| d.join("dist")).unwrap_or_default();
        let candidates = [cwd.join("dist"), cwd.join("../dist"), home_dist];
        candidates.into_iter().find(|p| p.join("index.html").exists()).unwrap_or_else(|| {
            warn!("No dist/ directory found — run setup.sh with Node.js to build the web UI");
            cwd.join("dist")
        })
    };

    let index_html = dist_dir.join("index.html");

    // Bind address: 127.0.0.1 by default (MCP spec), --bind-all for 0.0.0.0
    let bind_addr = if cli.bind_all { "0.0.0.0" } else { "127.0.0.1" };

    let explicit_port: Option<u16> = std::env::var("PORT").ok().and_then(|p| p.parse().ok());

    let listener = if let Some(port) = explicit_port {
        tokio::net::TcpListener::bind(format!("{bind_addr}:{port}")).await.unwrap_or_else(|e| {
            error!(port = port, error = %e, "Could not bind to port");
            eprintln!("  PORT={port} was set explicitly. Choose a different port.");
            std::process::exit(1);
        })
    } else {
        // Auto-scan: try 8432..=8441
        const BASE: u16 = 8432;
        const RANGE: u16 = 10;
        let mut found = None;
        for port in BASE..BASE + RANGE {
            match tokio::net::TcpListener::bind(format!("{bind_addr}:{port}")).await {
                Ok(l) => {
                    found = Some(l);
                    break;
                }
                Err(_) => continue,
            }
        }
        found.unwrap_or_else(|| {
            error!(range_start = BASE, range_end = BASE + RANGE - 1, "No free port found");
            eprintln!("  Try: PORT=<port> codescope");
            std::process::exit(1);
        })
    };

    let port = listener.local_addr().unwrap().port();

    // Build MCP HTTP transport config
    let cli_allowed_origins: Option<Vec<String>> =
        cli.allowed_origins.map(|s| s.split(',').map(|o| o.trim().to_string()).collect());

    let allowed_origins = cli_allowed_origins.unwrap_or_else(|| {
        vec![
            format!("http://localhost:{port}"),
            format!("http://127.0.0.1:{port}"),
            format!("http://localhost"),
            format!("http://127.0.0.1"),
            "null".to_string(),
        ]
    });

    let mcp_config = McpConfig {
        allowed_origins,
        auth_issuer: cli.auth_issuer,
        server_url: format!("http://{}:{port}", if cli.bind_all { "0.0.0.0" } else { "127.0.0.1" }),
    };

    let sessions: Arc<DashMap<String, McpSession>> = Arc::new(DashMap::new());
    let mcp_ctx = McpAppContext { state, sessions: sessions.clone(), config: Arc::new(mcp_config) };

    // MCP HTTP transport routes (with origin validation middleware)
    let mcp_router = Router::new()
        .route(
            "/mcp",
            post(codescope_server::mcp_http::handle_mcp_post)
                .delete(codescope_server::mcp_http::handle_mcp_delete)
                .get(codescope_server::mcp_http::handle_mcp_get),
        )
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(codescope_server::auth::prm_endpoint),
        )
        .layer(axum::middleware::from_fn_with_state(
            mcp_ctx.clone(),
            codescope_server::auth::validate_origin,
        ))
        .with_state(mcp_ctx);

    // Web UI API routes + MCP transport + static files
    let app = Router::new()
        .route("/health", get(api_health))
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
        .merge(mcp_router)
        .fallback_service(ServeDir::new(&dist_dir).not_found_service(ServeFile::new(&index_html)))
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .with_state(ctx);

    // Session cleanup: prune idle sessions every 5 minutes
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            let cutoff = std::time::Instant::now() - std::time::Duration::from_secs(1800);
            let before = sessions.len();
            sessions.retain(|_, session| session.last_activity > cutoff);
            let pruned = before - sessions.len();
            if pruned > 0 {
                debug!(pruned = pruned, remaining = sessions.len(), "Pruned idle MCP sessions");
            }
        }
    });

    info!(dist = %dist_dir.display(), "Serving web UI");
    info!("MCP HTTP transport at /mcp");
    info!(port = port, "http://localhost:{port}");
    // Machine-readable line for scripts (not through tracing)
    eprintln!("CODESCOPE_PORT={port}");

    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await.unwrap();
}
