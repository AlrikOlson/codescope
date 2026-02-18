mod api;
mod budget;
mod fuzzy;
mod mcp;
mod scan;
mod stubs;
mod tokenizer;
mod types;

use axum::{
    routing::{get, post},
    Router,
};
use std::path::PathBuf;
use std::sync::Arc;
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

fn load_codescope_config(project_root: &std::path::Path) -> ScanConfig {
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
// CLI help
// ---------------------------------------------------------------------------

fn print_help() {
    let version = env!("CARGO_PKG_VERSION");
    eprintln!("codescope-server {version}");
    eprintln!("Fast codebase indexer and search server");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  codescope-server [OPTIONS]");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("  --root <PATH>       Project root directory (default: current directory)");
    eprintln!("  --mcp               Run as MCP stdio server (for Claude Code)");
    eprintln!("  --dist <PATH>       Path to web UI dist directory");
    eprintln!("  --tokenizer <NAME>  Token counter: bytes-estimate (default) or tiktoken");
    eprintln!("  --help              Show this help message");
    eprintln!("  --version           Show version");
    eprintln!();
    eprintln!("ENVIRONMENT:");
    eprintln!("  PORT                HTTP server port (default: 8432)");
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

    let mcp_mode = args.iter().any(|a| a == "--mcp");

    // Project root: --root flag or current directory
    let project_root = if let Some(pos) = args.iter().position(|a| a == "--root") {
        match args.get(pos + 1) {
            Some(path) => PathBuf::from(path),
            None => {
                eprintln!("Error: --root requires a path argument");
                std::process::exit(1);
            }
        }
    } else {
        std::env::current_dir().unwrap_or_else(|_| {
            eprintln!("Error: Could not determine current directory. Use --root <path>");
            std::process::exit(1);
        })
    };

    let project_root = project_root.canonicalize().unwrap_or(project_root);

    // Tokenizer: --tokenizer flag or default bytes-estimate
    let tokenizer_name = args
        .iter()
        .position(|a| a == "--tokenizer")
        .and_then(|pos| args.get(pos + 1))
        .map(|s| s.as_str())
        .unwrap_or("bytes-estimate");

    let tok = tokenizer::create_tokenizer(tokenizer_name);

    // Load config
    let config = load_codescope_config(&project_root);

    eprintln!(
        "\n  Scanning codebase at {}...",
        project_root.display()
    );
    if !config.scan_dirs.is_empty() {
        eprintln!("  Scan dirs: {:?}", config.scan_dirs);
    }
    if !config.extensions.is_empty() {
        eprintln!("  Extensions: {:?}", config.extensions);
    }
    eprintln!("  Tokenizer: {}", tok.name());

    let start = Instant::now();

    let (all_files, manifest) = scan_files(&config);
    let file_count = all_files.len();
    let module_count = manifest.len();
    let tree = build_tree(&manifest);
    let deps = scan_deps(&config);
    let (search_files, search_modules) = build_search_index(&manifest);
    let import_graph = scan_imports(&all_files);

    let elapsed = start.elapsed();
    eprintln!(
        "  Scanned {} files -> {} modules, {} dep modules, {} import edges ({:.0}ms)\n",
        file_count,
        module_count,
        deps.len(),
        import_graph.imports.len(),
        elapsed.as_millis()
    );

    if mcp_mode {
        let mcp_state = McpState {
            project_root,
            config,
            all_files,
            manifest,
            deps,
            search_files,
            search_modules,
            import_graph,
            stub_cache: dashmap::DashMap::new(),
            tokenizer: tok,
        };
        run_mcp(mcp_state);
        return;
    }

    // HTTP server mode
    let tree_json = serde_json::to_string(&tree).unwrap();
    let manifest_json = serde_json::to_string(&manifest).unwrap();
    let deps_json = serde_json::to_string(&deps).unwrap();

    let state = Arc::new(AppState {
        project_root,
        config,
        tree_json,
        manifest_json,
        deps_json,
        deps,
        all_files,
        search_files,
        search_modules,
        import_graph,
        stub_cache: dashmap::DashMap::new(),
        tokenizer: tok,
    });

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
        .route("/api/context", post(api_context))
        .route("/api/imports", get(api_imports))
        .fallback_service(
            ServeDir::new(&dist_dir).not_found_service(ServeFile::new(&index_html)),
        )
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8432);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .unwrap_or_else(|e| {
            eprintln!("Error: Could not bind to port {port}: {e}");
            eprintln!("  Is another instance already running? Try PORT={} codescope-server", port + 1);
            std::process::exit(1);
        });

    eprintln!("  Serving UI from {}", dist_dir.display());
    eprintln!("  http://localhost:{port}\n");
    axum::serve(listener, app).await.unwrap();
}
