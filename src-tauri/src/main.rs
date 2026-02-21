// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use codescope_server::init;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use tauri::{AppHandle, Emitter, Manager};

// ---------------------------------------------------------------------------
// Types shared with frontend
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct RepoInfo {
    path: String,
    name: String,
    ecosystems: Vec<String>,
    workspace_info: Option<String>,
    file_count: usize,
    /// "ready" | "stale" | "needs_setup" | "new"
    status: String,
    /// Human-readable explanation of the status
    status_detail: String,
    /// Number of embedded chunks (0 if no cache)
    semantic_chunks: usize,
    /// Semantic model name ("" if no cache)
    semantic_model: String,
}

#[allow(dead_code)]
#[derive(Serialize)]
struct DoctorCheck {
    label: String,
    status: String, // "pass", "warn", "fail"
    message: String,
}

#[derive(Serialize)]
struct GlobalConfig {
    repos: Vec<RepoEntry>,
    version: String,
    has_semantic: bool,
}

#[derive(Serialize, Deserialize)]
struct RepoEntry {
    name: String,
    path: String,
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Build a RepoInfo for a path, computing status from registered set + semantic cache.
fn make_repo_info(
    entry_path: &std::path::Path,
    registered_set: &std::collections::HashSet<PathBuf>,
    canonical: &PathBuf,
) -> RepoInfo {
    let detection = init::detect_project(entry_path);
    let ecosystems: Vec<String> =
        detection.ecosystems.iter().map(|e| e.label().to_string()).collect();
    let name = entry_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let is_registered = registered_set.contains(canonical);
    let cache = codescope_server::semantic::check_semantic_cache(entry_path);

    let (status, status_detail, semantic_model, semantic_chunks) = match (is_registered, &cache) {
        (true, Some(ci)) if ci.current => (
            "ready",
            format!("{} chunks · {} model", ci.chunks, ci.model),
            ci.model.clone(),
            ci.chunks,
        ),
        (true, Some(ci)) => (
            "stale",
            format!("Embeddings outdated — will rebuild ({} chunks, {} model)", ci.chunks, ci.model),
            ci.model.clone(),
            ci.chunks,
        ),
        (true, None) => (
            "needs_setup",
            "Registered · not yet indexed".to_string(),
            String::new(),
            0,
        ),
        (false, Some(ci)) if ci.current => (
            "ready",
            format!("{} chunks · {} model", ci.chunks, ci.model),
            ci.model.clone(),
            ci.chunks,
        ),
        (false, Some(ci)) => (
            "new",
            format!("Has outdated embeddings ({} chunks)", ci.chunks),
            ci.model.clone(),
            ci.chunks,
        ),
        (false, None) => (
            "new",
            String::new(),
            String::new(),
            0,
        ),
    };

    RepoInfo {
        path: entry_path.to_string_lossy().to_string(),
        name,
        ecosystems,
        workspace_info: detection.workspace_info,
        file_count: 0,
        status: status.to_string(),
        status_detail,
        semantic_chunks,
        semantic_model,
    }
}

/// Scan common directories for project repos (depth-2) and merge registered repos.
/// Only does cheap marker detection — skips full file counting for speed.
#[tauri::command]
fn scan_for_repos(dirs: Vec<String>, registered: Vec<String>) -> Vec<RepoInfo> {
    let mut repos = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let project_markers = [
        "Cargo.toml",
        "package.json",
        "go.mod",
        "pyproject.toml",
        "setup.py",
        "CMakeLists.txt",
    ];
    let skip_dirs = [
        "node_modules", ".cargo", ".rustup", ".cache", ".local",
        ".npm", ".nvm", ".pyenv", ".conda", "snap", ".steam",
        "target", "dist", "build", ".git",
    ];

    let registered_set: std::collections::HashSet<PathBuf> = registered
        .iter()
        .filter_map(|p| PathBuf::from(p).canonicalize().ok())
        .collect();

    // Try to add a directory as a repo. Returns true if it had project markers.
    let try_add = |entry_path: &std::path::Path,
                   seen: &mut std::collections::HashSet<PathBuf>,
                   repos: &mut Vec<RepoInfo>| -> bool {
        let canonical = entry_path.canonicalize().unwrap_or(entry_path.to_path_buf());
        if !seen.insert(canonical.clone()) {
            return true;
        }
        let has_marker = project_markers.iter().any(|m| entry_path.join(m).exists());
        if !has_marker {
            return false;
        }
        repos.push(make_repo_info(entry_path, &registered_set, &canonical));
        true
    };

    for dir in &dirs {
        let path = PathBuf::from(dir);
        if !path.is_dir() {
            continue;
        }
        let entries = match std::fs::read_dir(&path) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if !entry_path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || skip_dirs.contains(&name.as_str()) {
                continue;
            }
            let found = try_add(&entry_path, &mut seen, &mut repos);
            if !found {
                // Depth 2: scan children of non-project dirs
                if let Ok(children) = std::fs::read_dir(&entry_path) {
                    for child in children.flatten() {
                        let child_path = child.path();
                        if !child_path.is_dir() {
                            continue;
                        }
                        let child_name = child.file_name().to_string_lossy().to_string();
                        if child_name.starts_with('.') || skip_dirs.contains(&child_name.as_str()) {
                            continue;
                        }
                        try_add(&child_path, &mut seen, &mut repos);
                    }
                }
            }
        }
    }

    // Merge registered repos not found by scan
    for reg_path in &registered {
        let path = PathBuf::from(reg_path);
        if !path.is_dir() {
            continue;
        }
        let canonical = path.canonicalize().unwrap_or(path.clone());
        if seen.contains(&canonical) {
            continue;
        }
        seen.insert(canonical.clone());
        repos.push(make_repo_info(&path, &registered_set, &canonical));
    }

    // Sort: ready first (informational), then actionable, then new — alpha within each
    repos.sort_by(|a, b| {
        let order = |s: &str| match s { "ready" => 0, "stale" => 1, "needs_setup" => 2, _ => 3 };
        order(&a.status).cmp(&order(&b.status))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    repos
}

/// Detect project ecosystem at a specific path.
#[tauri::command]
fn detect_project(path: String) -> RepoInfo {
    let root = PathBuf::from(&path);
    let detection = init::detect_project(&root);
    let ecosystems: Vec<String> =
        detection.ecosystems.iter().map(|e| e.label().to_string()).collect();
    let file_count = init::validate_scan(&root, &detection.scan_dirs, &detection.extensions);
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    RepoInfo {
        path,
        name,
        ecosystems,
        workspace_info: detection.workspace_info,
        file_count,
        status: "new".to_string(),
        status_detail: String::new(),
        semantic_chunks: 0,
        semantic_model: String::new(),
    }
}

/// Initialize a project (generate .codescope.toml + .mcp.json + register globally).
/// Does NOT build the semantic index — use `build_semantic_async` for that.
#[tauri::command]
fn init_repo(path: String) -> Result<String, String> {
    let root = PathBuf::from(&path)
        .canonicalize()
        .map_err(|e| format!("Invalid path: {}", e))?;

    let detection = init::detect_project(&root);

    // Generate .codescope.toml
    let config_path = root.join(".codescope.toml");
    if !config_path.exists() {
        let toml_content = init::generate_codescope_toml(&detection);
        std::fs::write(&config_path, &toml_content)
            .map_err(|e| format!("Failed to write .codescope.toml: {}", e))?;
    }

    // Generate or merge .mcp.json
    init::write_or_merge_mcp_json(&root)?;

    // Register globally
    init::register_global_repo(&root)?;

    let labels: Vec<&str> = detection.ecosystems.iter().map(|e| e.label()).collect();
    Ok(format!(
        "Initialized {} ({} files)",
        labels.join(" + "),
        init::validate_scan(&root, &detection.scan_dirs, &detection.extensions)
    ))
}

/// Event payload emitted during semantic index building.
#[derive(Clone, Serialize)]
struct SemanticEvent {
    repo: String,
    status: String,           // "extracting", "embedding", "ready", "failed"
    total_chunks: usize,
    total_batches: usize,
    completed_batches: usize,
    device: String,
}

/// Build semantic indexes for multiple repos on a background thread.
/// Emits `semantic-progress` events so the frontend can show granular progress.
#[tauri::command]
fn build_semantic_async(app: AppHandle, paths: Vec<String>, model: String) {
    std::thread::spawn(move || {
        for path in &paths {
            let root = match PathBuf::from(path).canonicalize() {
                Ok(r) => r,
                Err(_) => continue,
            };
            let repo_name = root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            let config = codescope_server::load_codescope_config(&root);
            let (all_files, _categories) = codescope_server::scan::scan_files(&config);
            let progress = codescope_server::types::SemanticProgress::new();
            // Use wizard-selected model, falling back to config file
            let sem_model = if model.is_empty() {
                config.semantic_model.clone()
            } else {
                Some(model.clone())
            };

            // Poll progress on a separate thread while the build runs
            let app2 = app.clone();
            let repo2 = repo_name.clone();
            let progress_ptr = &progress as *const codescope_server::types::SemanticProgress;
            // SAFETY: progress lives on this thread's stack and we join the poller
            // before progress goes out of scope.
            let progress_ref: &'static codescope_server::types::SemanticProgress =
                unsafe { &*progress_ptr };
            let poller = std::thread::spawn(move || {
                let status_labels = ["idle", "extracting", "embedding", "ready", "failed"];
                loop {
                    // Read completed first, then status — if status is terminal,
                    // completed_batches is guaranteed to be final (build sets completed
                    // before status with store ordering).
                    let completed = progress_ref.completed_batches.load(Relaxed);
                    let total_b = progress_ref.total_batches.load(Relaxed);
                    let total_c = progress_ref.total_chunks.load(Relaxed);
                    let status_val = progress_ref.status.load(Relaxed) as usize;
                    let status = status_labels.get(status_val).unwrap_or(&"unknown");
                    let device = progress_ref
                        .device
                        .read()
                        .map(|d| d.clone())
                        .unwrap_or_default();

                    let _ = app2.emit(
                        "semantic-progress",
                        SemanticEvent {
                            repo: repo2.clone(),
                            status: status.to_string(),
                            total_chunks: total_c,
                            total_batches: total_b,
                            // On terminal status, force completed = total to avoid
                            // stale-read jitter from Relaxed ordering
                            completed_batches: if status_val >= 3 { total_b } else { completed },
                            device,
                        },
                    );
                    if status_val >= 3 {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
            });

            // Run the actual build (blocking on this background thread)
            codescope_server::semantic::build_semantic_index(
                &all_files,
                sem_model.as_deref(),
                &progress,
                &root,
            );

            // Ensure we reached a terminal state
            let final_status = progress.status.load(Relaxed);
            if final_status < 3 {
                progress.status.store(4, Relaxed);
            }

            // Wait for poller to see the terminal state and exit
            let _ = poller.join();
            // No second emit — the poller already emitted the terminal event
        }
    });
}

/// Get current global configuration.
#[tauri::command]
fn get_config() -> GlobalConfig {
    let version = env!("CARGO_PKG_VERSION").to_string();
    let has_semantic = true; // codescope-server built with semantic feature by default

    // Read existing repos from global config
    let repos = match codescope_server::config_dir() {
        Some(dir) => {
            let repos_path = dir.join("repos.toml");
            if repos_path.exists() {
                codescope_server::parse_repos_toml(&repos_path)
                    .into_iter()
                    .map(|(name, path): (String, PathBuf)| RepoEntry {
                        name,
                        path: path.to_string_lossy().to_string(),
                    })
                    .collect()
            } else {
                Vec::new()
            }
        }
        None => Vec::new(),
    };

    GlobalConfig {
        repos,
        version,
        has_semantic,
    }
}

/// Get the CodeScope version.
#[tauri::command]
fn get_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Return default directories to scan for projects.
#[tauri::command]
fn get_scan_dirs() -> Vec<String> {
    let home = dirs::home_dir().unwrap_or_default();
    let candidates = [
        "",
        "Code",
        "code",
        "Projects",
        "projects",
        "repos",
        "src",
        "dev",
        "work",
        "github",
    ];
    candidates
        .iter()
        .map(|sub| {
            if sub.is_empty() {
                home.to_string_lossy().to_string()
            } else {
                home.join(sub).to_string_lossy().to_string()
            }
        })
        .filter(|p| PathBuf::from(p).is_dir())
        .collect()
}

/// Check whether `codescope` is on PATH.
#[tauri::command]
fn check_on_path() -> bool {
    std::process::Command::new("which")
        .arg("codescope")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Search window
// ---------------------------------------------------------------------------

/// Probe localhost ports 8432..8442 to find a running codescope server.
fn find_server_port() -> Option<u16> {
    for port in 8432..8442u16 {
        let addr: std::net::SocketAddr = ([127, 0, 0, 1], port).into();
        if std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(50)).is_ok()
        {
            return Some(port);
        }
    }
    None
}

/// Open (or focus) the search window, connecting to a running codescope server.
#[tauri::command]
fn open_search_window(app: AppHandle) -> Result<(), String> {
    // If the window already exists, just focus it
    if let Some(w) = app.get_webview_window("search") {
        w.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    let port = find_server_port().ok_or("No running codescope server found (tried ports 8432-8441). Run `codescope web` first.")?;

    let url = format!("search.html?port={}", port);
    tauri::WebviewWindowBuilder::new(&app, "search", tauri::WebviewUrl::App(url.into()))
        .title("CodeScope Search")
        .inner_size(1100.0, 750.0)
        .center()
        .decorations(false)
        .resizable(true)
        .build()
        .map_err(|e| e.to_string())?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            scan_for_repos,
            detect_project,
            init_repo,
            build_semantic_async,
            get_config,
            get_version,
            get_scan_dirs,
            check_on_path,
            open_search_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running CodeScope Setup");
}
