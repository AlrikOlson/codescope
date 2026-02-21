// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use codescope_server::init;
use codescope_server::types::ServerState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex, RwLock};
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
// Search window — embedded server state
// ---------------------------------------------------------------------------

/// Lazily-initialized server state for the search window.
/// Scans registered repos on first access, then reuses for subsequent queries.
static SEARCH_STATE: Mutex<Option<Arc<RwLock<ServerState>>>> = Mutex::new(None);

fn get_search_state() -> Result<Arc<RwLock<ServerState>>, String> {
    let mut guard = SEARCH_STATE.lock().map_err(|_| "Lock poisoned".to_string())?;
    if let Some(ref state) = *guard {
        return Ok(Arc::clone(state));
    }

    // Read registered repos from global config
    let repo_specs: Vec<(String, PathBuf)> = match codescope_server::config_dir() {
        Some(dir) => {
            let repos_path = dir.join("repos.toml");
            if repos_path.exists() {
                codescope_server::parse_repos_toml(&repos_path)
            } else {
                Vec::new()
            }
        }
        None => Vec::new(),
    };

    if repo_specs.is_empty() {
        return Err("No repos registered. Run `codescope init <path>` first.".to_string());
    }

    let tok: Arc<dyn codescope_server::tokenizer::Tokenizer> =
        Arc::new(codescope_server::tokenizer::BytesEstimateTokenizer);

    let repo_states: Vec<codescope_server::types::RepoState> = repo_specs
        .iter()
        .map(|(name, root)| codescope_server::scan_repo_with_options(name, root, &tok, false))
        .collect();

    let default_repo = if repo_states.len() == 1 {
        Some(repo_states[0].name.clone())
    } else {
        None
    };
    let mut repos = BTreeMap::new();
    for repo in repo_states {
        repos.insert(repo.name.clone(), repo);
    }

    let cross_repo_edges = codescope_server::scan::resolve_cross_repo_imports(&repos);

    let state = ServerState {
        repos,
        default_repo,
        cross_repo_edges,
        tokenizer: tok,
        semantic_enabled: false,
        semantic_model: None,
    };

    let arc = Arc::new(RwLock::new(state));
    *guard = Some(Arc::clone(&arc));
    Ok(arc)
}

/// Open (or focus) the search window.
#[tauri::command]
fn open_search_window(app: AppHandle) -> Result<(), String> {
    // If the window already exists, just focus it
    if let Some(w) = app.get_webview_window("search") {
        w.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    // Pre-initialize the search state so the window opens fast
    get_search_state()?;

    tauri::WebviewWindowBuilder::new(
        &app,
        "search",
        tauri::WebviewUrl::App("search.html".into()),
    )
    .title("CodeScope Search")
    .inner_size(1100.0, 750.0)
    .center()
    .decorations(false)
    .resizable(true)
    .build()
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Search across all indexed repos (equivalent to /api/find).
/// Runs the heavy work on a blocking thread so the UI stays responsive.
#[tauri::command]
async fn search_find(q: String, ext: Option<String>, limit: Option<usize>) -> Result<serde_json::Value, String> {
    let state = get_search_state()?;
    match tauri::async_runtime::spawn_blocking(move || search_find_blocking(q, ext, limit, state)).await {
        Ok(inner) => inner,
        Err(e) => Err(e.to_string()),
    }
}

fn search_find_blocking(q: String, ext: Option<String>, limit: Option<usize>, state: Arc<RwLock<ServerState>>) -> Result<serde_json::Value, String> {
    use codescope_server::fuzzy::{preprocess_search_query, run_search};
    use codescope_server::scan::get_category_path;
    use codescope_server::types::grep_relevance_score;
    use regex::RegexBuilder;
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;

    if q.is_empty() {
        return Ok(serde_json::json!({ "results": [], "queryTime": 0, "extCounts": {}, "catCounts": {} }));
    }

    let s = state.read().map_err(|_| "State lock poisoned".to_string())?;
    let repo = s.default_repo();
    let start = Instant::now();
    let limit = limit.unwrap_or(50).min(200);

    let ext_filter: Option<HashSet<String>> = ext.as_ref().map(|exts| {
        exts.split(',')
            .map(|e| e.trim().strip_prefix('.').unwrap_or(e.trim()).to_string())
            .collect()
    });

    #[derive(serde::Serialize)]
    struct MergedFind {
        path: String,
        filename: String,
        dir: String,
        ext: String,
        desc: String,
        category: String,
        name_score: f64,
        grep_score: f64,
        grep_count: usize,
        top_match: Option<String>,
        top_match_line: Option<usize>,
        filename_indices: Vec<usize>,
        terms_matched: usize,
        total_terms: usize,
    }

    let mut merged: HashMap<String, MergedFind> = HashMap::new();

    // 1. Fuzzy filename search
    let query = preprocess_search_query(&q);
    let search_resp = run_search(&repo.search_files, &repo.search_modules, &query, limit, 0);

    for f in &search_resp.files {
        if let Some(ref exts) = ext_filter {
            let e = f.ext.trim_start_matches('.');
            if !exts.contains(e) { continue; }
        }
        merged.insert(f.path.clone(), MergedFind {
            path: f.path.clone(),
            filename: f.filename.clone(),
            dir: f.dir.clone(),
            ext: f.ext.clone(),
            desc: f.desc.clone(),
            category: f.category.clone(),
            name_score: f.score,
            grep_score: 0.0,
            grep_count: 0,
            top_match: None,
            top_match_line: None,
            filename_indices: f.filename_indices.clone(),
            terms_matched: 0,
            total_terms: 0,
        });
    }

    // 2. Content grep (only if query is >= 2 chars)
    if q.len() >= 2 {
        let terms: Vec<String> = q.split_whitespace().map(|s| s.to_string()).collect();
        let terms_lower: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();
        let pattern_str = terms.iter().map(|t| regex::escape(t)).collect::<Vec<_>>().join("|");

        if let Ok(pattern) = RegexBuilder::new(&pattern_str).case_insensitive(true).build() {
            let idf_weights: Vec<f64> = terms_lower.iter().map(|t| repo.term_doc_freq.idf(t)).collect();

            for file in &repo.all_files {
                if let Some(ref exts) = ext_filter {
                    if !exts.contains(&file.ext) { continue; }
                }
                let content = match std::fs::read_to_string(&file.abs_path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let total_lines = content.lines().count().max(1);
                let mut match_count = 0usize;
                let mut best_snippet: Option<String> = None;
                let mut best_snippet_line: Option<usize> = None;
                let mut best_snippet_term_count: usize = 0;
                let mut first_match_line_idx = usize::MAX;
                let mut terms_seen: HashSet<usize> = HashSet::new();

                for (i, line) in content.lines().enumerate() {
                    if pattern.is_match(line) {
                        match_count += 1;
                        if first_match_line_idx == usize::MAX {
                            first_match_line_idx = i;
                        }
                        let line_lower = line.to_lowercase();
                        let line_term_count = terms_lower
                            .iter()
                            .filter(|t| line_lower.contains(t.as_str()))
                            .count();
                        for (ti, term) in terms_lower.iter().enumerate() {
                            if line_lower.contains(term.as_str()) {
                                terms_seen.insert(ti);
                            }
                        }
                        if line_term_count > best_snippet_term_count {
                            best_snippet_term_count = line_term_count;
                            let trimmed = if line.len() > 120 {
                                format!("{}...", &line[..line.floor_char_boundary(120)])
                            } else {
                                line.to_string()
                            };
                            best_snippet = Some(trimmed);
                            best_snippet_line = Some(i + 1);
                        }
                    }
                }

                if match_count == 0 { continue; }

                let filename = file.rel_path.rsplit('/').next().unwrap_or(&file.rel_path).to_lowercase();
                let grep_score = grep_relevance_score(
                    match_count,
                    total_lines,
                    &filename,
                    &file.ext,
                    &terms_lower,
                    terms_seen.len(),
                    if first_match_line_idx == usize::MAX { 0 } else { first_match_line_idx },
                    &idf_weights,
                );

                let fname = file.rel_path.rsplit('/').next().unwrap_or(&file.rel_path).to_string();
                let dir = file.rel_path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
                let ext_str = file.ext.clone();
                let category = get_category_path(&file.rel_path, &repo.config).join(" > ");
                let file_terms_matched = terms_seen.len();

                let entry = merged.entry(file.rel_path.clone()).or_insert_with(|| MergedFind {
                    path: file.rel_path.clone(),
                    filename: fname,
                    dir,
                    ext: ext_str,
                    desc: String::new(),
                    category,
                    name_score: 0.0,
                    grep_score: 0.0,
                    grep_count: 0,
                    top_match: None,
                    top_match_line: None,
                    filename_indices: Vec::new(),
                    terms_matched: 0,
                    total_terms: terms_lower.len(),
                });
                entry.grep_score = grep_score;
                entry.grep_count = match_count;
                entry.top_match = best_snippet;
                entry.top_match_line = best_snippet_line;
                entry.terms_matched = file_terms_matched;
                entry.total_terms = terms_lower.len();
            }
        }
    }

    // 3. Score, sort, truncate
    let query_term_count = q.split_whitespace().count();
    let (name_w, grep_w) = if query_term_count > 1 { (0.4, 0.6) } else { (0.6, 0.4) };
    let mut ranked: Vec<MergedFind> = merged.into_values().collect();
    let max_name = ranked.iter().map(|r| r.name_score).fold(0.0f64, f64::max).max(1.0);
    let max_grep = ranked.iter().map(|r| r.grep_score).fold(0.0f64, f64::max).max(1.0);

    ranked.sort_by(|a, b| {
        let norm_a = (a.name_score / max_name) * name_w + (a.grep_score / max_grep) * grep_w;
        let norm_b = (b.name_score / max_name) * name_w + (b.grep_score / max_grep) * grep_w;
        let boost_a = if a.name_score > 0.0 && a.grep_count > 0 { 1.25 } else { 1.0 };
        let boost_b = if b.name_score > 0.0 && b.grep_count > 0 { 1.25 } else { 1.0 };
        (norm_b * boost_b).partial_cmp(&(norm_a * boost_a)).unwrap_or(std::cmp::Ordering::Equal)
    });
    ranked.truncate(limit);

    // 4. Build response
    let mut ext_counts: HashMap<String, usize> = HashMap::new();
    let mut cat_counts: HashMap<String, usize> = HashMap::new();
    let results: Vec<serde_json::Value> = ranked
        .into_iter()
        .map(|r| {
            let norm_score = (r.name_score / max_name) * name_w + (r.grep_score / max_grep) * grep_w;
            let boost = if r.name_score > 0.0 && r.grep_count > 0 { 1.25 } else { 1.0 };
            let combined_score = norm_score * boost;
            let match_type = if r.name_score > 0.0 && r.grep_count > 0 {
                "both"
            } else if r.name_score > 0.0 {
                "name"
            } else {
                "content"
            };
            *ext_counts.entry(r.ext.clone()).or_insert(0) += 1;
            if !r.category.is_empty() {
                *cat_counts.entry(r.category.clone()).or_insert(0) += 1;
            }

            serde_json::json!({
                "path": r.path,
                "filename": r.filename,
                "dir": r.dir,
                "ext": r.ext,
                "desc": r.desc,
                "category": r.category,
                "nameScore": r.name_score,
                "grepScore": r.grep_score,
                "combinedScore": combined_score,
                "matchType": match_type,
                "grepCount": r.grep_count,
                "topMatch": r.top_match,
                "topMatchLine": r.top_match_line,
                "filenameIndices": r.filename_indices,
                "termsMatched": r.terms_matched,
                "totalTerms": r.total_terms,
            })
        })
        .collect();

    let query_time = start.elapsed().as_millis() as u64;

    Ok(serde_json::json!({
        "results": results,
        "queryTime": query_time,
        "extCounts": ext_counts,
        "catCounts": cat_counts,
    }))
}

/// Read a single file by path (equivalent to /api/file).
/// Runs on a blocking thread to avoid stalling the UI.
#[tauri::command]
async fn search_read_file(path: String) -> Result<serde_json::Value, String> {
    let state = get_search_state()?;
    match tauri::async_runtime::spawn_blocking(move || -> Result<serde_json::Value, String> {
        let s = state.read().map_err(|_| "State lock poisoned".to_string())?;
        let repo = s.default_repo();

        let full_path = codescope_server::types::validate_path(&repo.root, &path)
            .map_err(|e| e.to_string())?;

        let metadata = std::fs::metadata(&full_path).map_err(|_| "File not found".to_string())?;
        let file_size = metadata.len();
        let raw = std::fs::read_to_string(&full_path).map_err(|_| "Read error".to_string())?;

        let max_read = codescope_server::types::MAX_FILE_READ;
        let truncated = raw.len() > max_read;
        let content = if truncated {
            let mut end = max_read;
            while !raw.is_char_boundary(end) && end > 0 { end -= 1; }
            raw[..end].to_string()
        } else {
            raw
        };
        let lines = content.lines().count();

        Ok(serde_json::json!({
            "content": content,
            "lines": lines,
            "size": file_size,
            "path": path,
            "truncated": truncated,
        }))
    }).await {
        Ok(inner) => inner,
        Err(e) => Err(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let search_mode = std::env::args().any(|a| a == "--search");

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
            search_find,
            search_read_file,
        ])
        .setup(move |app| {
            if search_mode {
                // Hide the default setup window
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
                // Pre-initialize search state (scans repos)
                if let Err(e) = get_search_state() {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
                let search = tauri::WebviewWindowBuilder::new(
                    app,
                    "search",
                    tauri::WebviewUrl::App("search.html".into()),
                )
                .title("CodeScope Search")
                .inner_size(1100.0, 750.0)
                .center()
                .decorations(false)
                .resizable(true)
                .build()?;

                // Close the hidden setup window now that search is ready
                let app_handle = app.handle().clone();
                search.on_window_event(move |event| {
                    if let tauri::WindowEvent::Destroyed = event {
                        // When search window closes, exit the app
                        app_handle.exit(0);
                    }
                });
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.close();
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running CodeScope");
}
