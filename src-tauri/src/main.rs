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

            // Build AST index if treesitter is available (needed by semantic chunker)
            #[cfg(feature = "treesitter")]
            let ast_index = codescope_server::ast::build_ast_index(&all_files);

            // Run the actual build (blocking on this background thread)
            codescope_server::semantic::build_semantic_index(
                &all_files,
                sem_model.as_deref(),
                &progress,
                &root,
                #[cfg(feature = "treesitter")]
                Some(&ast_index),
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
    let cmd = if cfg!(target_os = "windows") { "where" } else { "which" };
    std::process::Command::new(cmd)
        .arg("codescope")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Find the directory where `cargo install` puts binaries.
fn cargo_bin_dir() -> Option<PathBuf> {
    // Check CARGO_INSTALL_ROOT first (e.g. ~/.local → ~/.local/bin)
    if let Ok(root) = std::env::var("CARGO_INSTALL_ROOT") {
        let dir = PathBuf::from(root).join("bin");
        if dir.is_dir() {
            return Some(dir);
        }
    }
    // Check CARGO_HOME (defaults to ~/.cargo)
    if let Ok(home) = std::env::var("CARGO_HOME") {
        let dir = PathBuf::from(home).join("bin");
        if dir.is_dir() {
            return Some(dir);
        }
    }
    // Default: ~/.cargo/bin
    if let Some(home) = dirs::home_dir() {
        let dir = home.join(".cargo").join("bin");
        if dir.is_dir() {
            return Some(dir);
        }
    }
    None
}

/// Add the cargo bin directory to the user's PATH, cross-platform.
/// Returns a human-readable message describing what was done.
#[tauri::command]
fn fix_path() -> Result<String, String> {
    let bin_dir = cargo_bin_dir()
        .ok_or_else(|| "Could not find cargo bin directory".to_string())?;
    let bin_str = bin_dir.to_string_lossy().to_string();

    // Check if already on PATH
    if let Ok(path) = std::env::var("PATH") {
        let sep = if cfg!(target_os = "windows") { ';' } else { ':' };
        for entry in path.split(sep) {
            if PathBuf::from(entry) == bin_dir {
                return Ok(format!("{} is already on PATH", bin_str));
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        fix_path_windows(&bin_str)
    }
    #[cfg(not(target_os = "windows"))]
    {
        fix_path_unix(&bin_str)
    }
}

/// Windows: Add to user-level PATH via the registry and broadcast the change.
#[cfg(target_os = "windows")]
fn fix_path_windows(bin_dir: &str) -> Result<String, String> {
    use std::os::windows::ffi::OsStrExt;

    // Read current user PATH from HKCU\Environment
    let output = std::process::Command::new("reg")
        .args(["query", r"HKCU\Environment", "/v", "Path"])
        .output()
        .map_err(|e| format!("Failed to query registry: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let current_path = stdout
        .lines()
        .find(|l| l.contains("REG_SZ") || l.contains("REG_EXPAND_SZ"))
        .and_then(|l| {
            // Format: "    Path    REG_EXPAND_SZ    value"
            l.split("REG_SZ").last()
                .or_else(|| l.split("REG_EXPAND_SZ").last())
                .map(|v| v.trim().to_string())
        })
        .unwrap_or_default();

    // Check if already present (case-insensitive on Windows)
    let bin_lower = bin_dir.to_lowercase();
    for entry in current_path.split(';') {
        if entry.trim().to_lowercase() == bin_lower {
            return Ok(format!("{} is already on PATH", bin_dir));
        }
    }

    // Append to user PATH
    let new_path = if current_path.is_empty() {
        bin_dir.to_string()
    } else if current_path.ends_with(';') {
        format!("{}{}", current_path, bin_dir)
    } else {
        format!("{};{}", current_path, bin_dir)
    };

    let status = std::process::Command::new("reg")
        .args(["add", r"HKCU\Environment", "/v", "Path", "/t", "REG_EXPAND_SZ", "/d", &new_path, "/f"])
        .status()
        .map_err(|e| format!("Failed to write registry: {}", e))?;

    if !status.success() {
        return Err("Failed to update PATH in registry".to_string());
    }

    // Broadcast WM_SETTINGCHANGE so new terminals pick up the change
    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
    extern "system" {
        fn SendMessageTimeoutW(
            hwnd: usize, msg: u32, wparam: usize, lparam: *const u16,
            flags: u32, timeout: u32, result: *mut usize,
        ) -> usize;
    }
    const HWND_BROADCAST: usize = 0xFFFF;
    const WM_SETTINGCHANGE: u32 = 0x001A;
    const SMTO_ABORTIFHUNG: u32 = 0x0002;
    let env = wide("Environment");
    let mut _result: usize = 0;
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST, WM_SETTINGCHANGE, 0,
            env.as_ptr(), SMTO_ABORTIFHUNG, 5000, &mut _result,
        );
    }

    Ok(format!("Added {} to PATH. New terminals will pick it up automatically.", bin_dir))
}

/// Unix: Append an export line to the user's shell profile.
#[cfg(not(target_os = "windows"))]
fn fix_path_unix(bin_dir: &str) -> Result<String, String> {
    use std::io::Write;

    let home = dirs::home_dir()
        .ok_or_else(|| "Could not determine home directory".to_string())?;

    // Determine which shell profile to modify
    let shell = std::env::var("SHELL").unwrap_or_default();
    let profile_path = if shell.ends_with("zsh") {
        home.join(".zshrc")
    } else if shell.ends_with("bash") {
        // Prefer .bashrc if it exists, otherwise .bash_profile (macOS)
        let bashrc = home.join(".bashrc");
        if bashrc.exists() { bashrc } else { home.join(".bash_profile") }
    } else if shell.ends_with("fish") {
        // Fish uses a different syntax, handled below
        home.join(".config").join("fish").join("config.fish")
    } else {
        home.join(".profile")
    };

    let profile_name = profile_path.file_name()
        .unwrap_or_default().to_string_lossy().to_string();

    // Check if the export line is already present
    if let Ok(content) = std::fs::read_to_string(&profile_path) {
        if content.contains(bin_dir) {
            return Ok(format!("{} already contains {}", profile_name, bin_dir));
        }
    }

    let line = if shell.ends_with("fish") {
        format!("\n# Added by CodeScope\nfish_add_path {}\n", bin_dir)
    } else {
        format!("\n# Added by CodeScope\nexport PATH=\"{}:$PATH\"\n", bin_dir)
    };

    // Ensure parent directory exists (for fish config)
    if let Some(parent) = profile_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&profile_path)
        .map_err(|e| format!("Failed to open {}: {}", profile_name, e))?;

    file.write_all(line.as_bytes())
        .map_err(|e| format!("Failed to write to {}: {}", profile_name, e))?;

    Ok(format!("Added {} to {}. Run `source ~/{}` or open a new terminal.", bin_dir, profile_name, profile_name))
}

// ---------------------------------------------------------------------------
// MCP configuration
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize)]
struct McpStatus {
    configured: usize,
    total: usize,
    /// Paths that are missing .mcp.json or the codescope entry
    missing: Vec<String>,
}

/// Check how many of the selected repos have .mcp.json with a codescope entry.
#[tauri::command]
fn check_mcp_status(paths: Vec<String>) -> McpStatus {
    let mut configured = 0;
    let mut missing = Vec::new();
    for p in &paths {
        let mcp_path = PathBuf::from(p).join(".mcp.json");
        if mcp_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&mcp_path) {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                    if data.get("mcpServers")
                        .and_then(|v| v.as_object())
                        .map(|o| o.contains_key("codescope"))
                        .unwrap_or(false)
                    {
                        configured += 1;
                        continue;
                    }
                }
            }
        }
        missing.push(p.clone());
    }
    McpStatus { configured, total: paths.len(), missing }
}

/// Create or merge .mcp.json for the given repo paths.
#[tauri::command]
fn configure_mcp(paths: Vec<String>) -> Result<String, String> {
    let mut ok = 0;
    let mut errors = Vec::new();
    for p in &paths {
        let root = PathBuf::from(p);
        match init::write_or_merge_mcp_json(&root) {
            Ok(()) => ok += 1,
            Err(e) => errors.push(format!("{}: {}", p, e)),
        }
    }
    if errors.is_empty() {
        Ok(format!("Configured .mcp.json for {} project{}", ok, if ok == 1 { "" } else { "s" }))
    } else {
        Err(format!("Configured {} but failed for: {}", ok, errors.join("; ")))
    }
}

// ---------------------------------------------------------------------------
// Shell completions
// ---------------------------------------------------------------------------

/// Detect which completions file path to use based on the current platform/shell.
/// Returns (shell_name_for_clap, install_path).
fn completions_target() -> Option<(String, PathBuf)> {
    let home = dirs::home_dir()?;

    if cfg!(target_os = "windows") {
        // Install to a separate file; install_completions() dot-sources it from the PowerShell profile
        let comp_file = home.join(".config").join("codescope").join("completions.ps1");
        return Some(("powershell".to_string(), comp_file));
    }

    let shell = std::env::var("SHELL").unwrap_or_default();
    if shell.ends_with("zsh") {
        Some(("zsh".to_string(), home.join(".zfunc").join("_codescope")))
    } else if shell.ends_with("fish") {
        Some(("fish".to_string(), home.join(".config").join("fish").join("completions").join("codescope.fish")))
    } else {
        // Bash (default)
        Some(("bash".to_string(), home.join(".local").join("share").join("bash-completion").join("completions").join("codescope")))
    }
}

/// Check if shell completions are installed at the standard location.
#[tauri::command]
fn check_completions() -> bool {
    completions_target()
        .map(|(_, path)| path.exists())
        .unwrap_or(false)
}

/// Install shell completions by running `codescope completions <shell>` and writing the output.
#[tauri::command]
fn install_completions() -> Result<String, String> {
    let (shell_name, install_path) = completions_target()
        .ok_or_else(|| "Could not determine shell completions target".to_string())?;

    // Find the codescope binary
    let bin = find_codescope_binary()
        .ok_or_else(|| "codescope binary not found. Install it first with `cargo install --path server`".to_string())?;

    // Generate completions
    let output = std::process::Command::new(&bin)
        .args(["completions", &shell_name])
        .output()
        .map_err(|e| format!("Failed to run codescope completions: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("codescope completions failed: {}", stderr.trim()));
    }

    let completions_content = String::from_utf8_lossy(&output.stdout);

    // Ensure parent directory exists
    if let Some(parent) = install_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
    }

    // Write completions file
    std::fs::write(&install_path, completions_content.as_bytes())
        .map_err(|e| format!("Failed to write {}: {}", install_path.display(), e))?;

    // Shell-specific profile updates
    #[cfg(target_os = "windows")]
    {
        // Dot-source the completions file from the PowerShell profile
        let home = dirs::home_dir().unwrap();
        let profile = home
            .join("Documents")
            .join("PowerShell")
            .join("Microsoft.PowerShell_profile.ps1");
        let source_line = format!(". \"{}\"", install_path.display());
        let needs_update = std::fs::read_to_string(&profile)
            .map(|c| !c.contains(&source_line))
            .unwrap_or(true);
        if needs_update {
            if let Some(parent) = profile.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&profile)
                .map_err(|e| format!("Failed to update PowerShell profile: {}", e))?;
            use std::io::Write;
            writeln!(file, "\n# CodeScope tab completions\n{}", source_line)
                .map_err(|e| format!("Failed to write to profile: {}", e))?;
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let shell_env = std::env::var("SHELL").unwrap_or_default();
        if shell_env.ends_with("zsh") {
            // Ensure fpath includes ~/.zfunc and compinit is called
            let home = dirs::home_dir().unwrap();
            let zshrc = home.join(".zshrc");
            let zfunc_line = "fpath=(~/.zfunc $fpath)";
            let needs_fpath = std::fs::read_to_string(&zshrc)
                .map(|c| !c.contains(zfunc_line))
                .unwrap_or(true);
            if needs_fpath {
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&zshrc)
                    .map_err(|e| format!("Failed to update .zshrc: {}", e))?;
                use std::io::Write;
                writeln!(file, "\n# CodeScope completions\n{}\nautoload -Uz compinit && compinit", zfunc_line)
                    .map_err(|e| format!("Failed to write to .zshrc: {}", e))?;
            }
        }
        // bash and fish auto-discover from their standard directories — no profile edit needed
    }

    Ok(format!("Installed {} completions to {}", shell_name, install_path.display()))
}

/// Find the codescope binary, checking PATH first then known cargo bin locations.
fn find_codescope_binary() -> Option<PathBuf> {
    let bin_name = if cfg!(target_os = "windows") { "codescope.exe" } else { "codescope" };

    // Check if it's on PATH
    let which_cmd = if cfg!(target_os = "windows") { "where" } else { "which" };
    if let Ok(output) = std::process::Command::new(which_cmd).arg("codescope").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().lines().next()?.to_string();
            return Some(PathBuf::from(path));
        }
    }

    // Check cargo bin directory
    if let Some(bin_dir) = cargo_bin_dir() {
        let candidate = bin_dir.join(bin_name);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// GPU detection & CUDA installation
// ---------------------------------------------------------------------------

/// GPU and CUDA toolkit status returned to the frontend.
#[derive(Clone, Serialize)]
struct GpuInfo {
    gpu_detected: bool,
    gpu_name: String,
    driver_version: String,
    cuda_installed: bool,
    cuda_version: String,
    cuda_path: String,
    /// "windows" | "linux" | "macos"
    platform: String,
    /// Whether automatic CUDA install is possible
    can_auto_install: bool,
    /// Shell command for manual install (Linux)
    manual_install_cmd: String,
}

fn detect_nvidia_gpu() -> (bool, String, String) {
    let output = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=name,driver_version", "--format=csv,noheader,nounits"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let line = stdout.trim();
            if let Some((name, driver)) = line.split_once(',') {
                (true, name.trim().to_string(), driver.trim().to_string())
            } else {
                (true, line.to_string(), String::new())
            }
        }
        _ => (false, String::new(), String::new()),
    }
}

fn detect_cuda_toolkit() -> (bool, String, String) {
    // 1. Check CUDA_PATH env var
    if let Ok(path) = std::env::var("CUDA_PATH") {
        let p = PathBuf::from(&path);
        let nvcc = if cfg!(target_os = "windows") {
            p.join("bin").join("nvcc.exe")
        } else {
            p.join("bin").join("nvcc")
        };
        if nvcc.exists() {
            let version = nvcc_version(&path);
            return (true, version, path);
        }
    }

    // 2. Check known install paths
    #[cfg(target_os = "windows")]
    {
        let base = r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA";
        if let Ok(entries) = std::fs::read_dir(base) {
            let mut versions: Vec<std::fs::DirEntry> = entries
                .flatten()
                .filter(|e| e.path().join("bin").join("nvcc.exe").exists())
                .collect();
            versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
            if let Some(entry) = versions.first() {
                let path = entry.path().to_string_lossy().to_string();
                let version = entry
                    .file_name()
                    .to_string_lossy()
                    .trim_start_matches('v')
                    .to_string();
                return (true, version, path);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        for p in &["/usr/local/cuda", "/opt/cuda"] {
            if PathBuf::from(p).join("bin/nvcc").exists() {
                let version = nvcc_version(p);
                return (true, version, p.to_string());
            }
        }
        // Also try nvcc on PATH
        if let Ok(o) = std::process::Command::new("nvcc").arg("--version").output() {
            if o.status.success() {
                let out = String::from_utf8_lossy(&o.stdout);
                let version = parse_nvcc_output(&out);
                return (true, version, String::new());
            }
        }
    }

    (false, String::new(), String::new())
}

fn nvcc_version(cuda_path: &str) -> String {
    let nvcc = if cfg!(target_os = "windows") {
        PathBuf::from(cuda_path).join("bin").join("nvcc.exe")
    } else {
        PathBuf::from(cuda_path).join("bin").join("nvcc")
    };
    if let Ok(o) = std::process::Command::new(nvcc).arg("--version").output() {
        if o.status.success() {
            return parse_nvcc_output(&String::from_utf8_lossy(&o.stdout));
        }
    }
    String::new()
}

/// Parse "release 12.6" from nvcc --version output.
fn parse_nvcc_output(output: &str) -> String {
    for line in output.lines() {
        if let Some(idx) = line.find("release ") {
            let rest = &line[idx + 8..];
            if let Some(end) = rest.find(',') {
                return rest[..end].trim().to_string();
            }
            return rest.trim().to_string();
        }
    }
    String::new()
}

fn linux_install_command() -> String {
    // Detect distro for the right command
    let os_release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    if os_release.contains("Ubuntu") || os_release.contains("Debian") {
        let codename = if os_release.contains("24.04") {
            "ubuntu2404"
        } else if os_release.contains("22.04") {
            "ubuntu2204"
        } else {
            "ubuntu2204"
        };
        format!(
            "wget https://developer.download.nvidia.com/compute/cuda/repos/{codename}/x86_64/cuda-keyring_1.1-1_all.deb && \
sudo dpkg -i cuda-keyring_1.1-1_all.deb && \
sudo apt-get update && \
sudo apt-get install -y cuda-toolkit-12-6"
        )
    } else if os_release.contains("Fedora") || os_release.contains("Red Hat") || os_release.contains("CentOS") {
        "sudo dnf config-manager --add-repo \
https://developer.download.nvidia.com/compute/cuda/repos/rhel9/x86_64/cuda-rhel9.repo && \
sudo dnf install -y cuda-toolkit-12-6".to_string()
    } else if os_release.contains("Arch") {
        "sudo pacman -S --noconfirm cuda".to_string()
    } else {
        "# Visit https://developer.nvidia.com/cuda-downloads for your distribution".to_string()
    }
}

/// Detect GPU and CUDA toolkit status.
#[tauri::command]
fn detect_gpu() -> GpuInfo {
    let platform = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };

    let (gpu_detected, gpu_name, driver_version) = detect_nvidia_gpu();
    let (cuda_installed, cuda_version, cuda_path) = detect_cuda_toolkit();

    let can_auto_install = platform == "windows" && gpu_detected && !cuda_installed;
    let manual_install_cmd = if platform == "linux" && gpu_detected && !cuda_installed {
        linux_install_command()
    } else {
        String::new()
    };

    GpuInfo {
        gpu_detected,
        gpu_name,
        driver_version,
        cuda_installed,
        cuda_version,
        cuda_path,
        platform: platform.to_string(),
        can_auto_install,
        manual_install_cmd,
    }
}

/// CUDA install progress event.
#[derive(Clone, Serialize)]
struct CudaInstallEvent {
    /// "downloading" | "installing" | "complete" | "failed"
    status: String,
    /// 0.0–1.0 download progress
    progress: f64,
    message: String,
}

/// Download and install CUDA toolkit. Emits `cuda-install-progress` events.
#[tauri::command]
fn install_cuda(app: AppHandle) {
    std::thread::spawn(move || {
        let result = run_cuda_install(&app);
        if let Err(e) = result {
            let _ = app.emit(
                "cuda-install-progress",
                CudaInstallEvent {
                    status: "failed".to_string(),
                    progress: 0.0,
                    message: e,
                },
            );
        }
    });
}

fn emit_cuda(app: &AppHandle, status: &str, progress: f64, message: &str) {
    let _ = app.emit(
        "cuda-install-progress",
        CudaInstallEvent {
            status: status.to_string(),
            progress,
            message: message.to_string(),
        },
    );
}

fn run_cuda_install(app: &AppHandle) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        run_cuda_install_windows(app)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Err("Automatic CUDA installation is only supported on Windows. Use the manual command shown above.".to_string())
    }
}

#[cfg(target_os = "windows")]
fn run_cuda_install_windows(app: &AppHandle) -> Result<(), String> {
    use std::time::Duration;

    let temp_dir = std::env::temp_dir();
    let installer_path = temp_dir.join("cuda_12.6_network_installer.exe");
    let installer_str = installer_path.to_string_lossy().to_string();
    let log_dir = temp_dir.join("codescope-cuda-log");
    let url =
        "https://developer.download.nvidia.com/compute/cuda/12.6.3/network_installers/cuda_12.6.3_windows_network.exe";
    // Network installer is ~30MB
    let expected_size: u64 = 32_000_000;

    emit_cuda(app, "downloading", 0.0, "Downloading CUDA 12.6 installer...");

    // Download using curl.exe (built into Windows 10+)
    let dl_path = installer_str.clone();
    let download_handle = std::thread::spawn(move || {
        std::process::Command::new("curl.exe")
            .args(["-L", "-o", &dl_path, "--silent", "--show-error", url])
            .output()
    });

    // Poll file size for progress
    loop {
        if download_handle.is_finished() {
            break;
        }
        if let Ok(meta) = std::fs::metadata(&installer_path) {
            let progress = (meta.len() as f64 / expected_size as f64).min(0.99);
            let mb = meta.len() as f64 / 1_000_000.0;
            emit_cuda(
                app,
                "downloading",
                progress,
                &format!("Downloading... {:.1} MB", mb),
            );
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    let output = download_handle
        .join()
        .map_err(|_| "Download thread panicked".to_string())?
        .map_err(|e| format!("Failed to start download: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Download failed: {}", stderr.trim()));
    }

    // Verify file exists and has reasonable size
    let meta = std::fs::metadata(&installer_path)
        .map_err(|_| "Downloaded file not found".to_string())?;
    if meta.len() < 1_000_000 {
        let _ = std::fs::remove_file(&installer_path);
        return Err("Downloaded file is too small — download may have failed".to_string());
    }

    emit_cuda(
        app,
        "installing",
        0.0,
        "Installing CUDA Toolkit — this will request administrator access...",
    );

    // Prepare log directory for progress monitoring
    let _ = std::fs::create_dir_all(&log_dir);
    let log_dir_str = log_dir.to_string_lossy().to_string();

    // Launch installer with logging enabled for progress tracking.
    // -s: silent, -n: no reboot, -log: write install logs, -loglevel:6: max verbosity
    let install_args = format!("-s -n -log:\"{}\" -loglevel:6", log_dir_str);
    let process = shell_execute_runas(&installer_path, &install_args)?;

    // CUDA install directory — monitor for subdirectory creation as progress signal
    let cuda_install_dir = std::path::PathBuf::from(
        r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.6"
    );

    // Known components that get installed as subdirectories, in rough order.
    // Each one created bumps the progress bar forward.
    let component_dirs: &[(&str, &str)] = &[
        ("bin",       "Installing CUDA binaries..."),
        ("include",   "Installing CUDA headers..."),
        ("lib",       "Installing CUDA libraries..."),
        ("nvvm",      "Installing NVVM compiler..."),
        ("extras",    "Installing extras & samples..."),
        ("libnvvp",   "Installing Visual Profiler..."),
        ("nsight",    "Installing Nsight tools..."),
        ("compute-sanitizer", "Installing Compute Sanitizer..."),
    ];

    // Track which components we've seen so far
    let mut seen_components = vec![false; component_dirs.len()];
    let mut last_log_size: u64 = 0;
    let mut last_log_component = String::new();

    // Poll for progress until the installer exits
    loop {
        // Check if the process has finished
        if let Some(exit_code) = process.try_wait() {
            // Clean up installer and log dir
            let _ = std::fs::remove_file(&installer_path);
            let _ = std::fs::remove_dir_all(&log_dir);

            if exit_code != 0 {
                return Err(format!(
                    "Installer exited with code {}. You may need to download and run the installer manually from https://developer.nvidia.com/cuda-downloads",
                    exit_code
                ));
            }
            break;
        }

        // --- Progress source 1: Monitor target directory for component subdirectories ---
        let mut components_found = 0usize;
        for (i, (dir_name, msg)) in component_dirs.iter().enumerate() {
            if !seen_components[i] && cuda_install_dir.join(dir_name).exists() {
                seen_components[i] = true;
                last_log_component = msg.to_string();
            }
            if seen_components[i] {
                components_found += 1;
            }
        }

        // --- Progress source 2: Poll the log file for component extraction info ---
        let log_message = scan_cuda_log_dir(&log_dir, &mut last_log_size);

        // Compute progress: directory monitoring gives us a 0..1 range over known components
        // Reserve 0.0-0.05 for "starting", 0.05-0.95 for component installs, 0.95-1.0 for finalization
        let dir_progress = if component_dirs.is_empty() {
            0.0
        } else {
            components_found as f64 / component_dirs.len() as f64
        };
        let progress = 0.05 + dir_progress * 0.90;

        // Pick the best status message: log file detail > component dir > generic
        let message = if let Some(ref log_msg) = log_message {
            log_msg.clone()
        } else if !last_log_component.is_empty() {
            last_log_component.clone()
        } else {
            "Installing CUDA Toolkit components...".to_string()
        };

        emit_cuda(app, "installing", progress.min(0.95), &message);

        std::thread::sleep(Duration::from_millis(800));
    }

    // Give Windows a moment to finalize PATH changes
    std::thread::sleep(Duration::from_secs(2));

    // Verify installation
    let (installed, version, _path) = detect_cuda_toolkit();
    if installed {
        emit_cuda(
            app,
            "complete",
            1.0,
            &format!("CUDA {} installed successfully! Restart the app to enable GPU acceleration.", version),
        );
    } else {
        emit_cuda(
            app,
            "complete",
            1.0,
            "Installation finished. A system restart may be needed for CUDA to be detected.",
        );
    }

    Ok(())
}

/// Scan the CUDA installer log directory for the most recent meaningful status line.
/// Tracks `last_size` to only read new content since the last poll.
#[cfg(target_os = "windows")]
fn scan_cuda_log_dir(log_dir: &std::path::Path, last_size: &mut u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};

    // Find the most recently modified .log file in the directory
    let log_file = std::fs::read_dir(log_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "log")
                .unwrap_or(false)
        })
        .max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()))?;

    let path = log_file.path();
    let file_size = std::fs::metadata(&path).ok()?.len();

    // Only read if the file has grown
    if file_size <= *last_size {
        return None;
    }

    let mut file = std::fs::File::open(&path).ok()?;
    // Seek to where we left off (or near the end for large files)
    let seek_pos = if *last_size > 0 {
        *last_size
    } else {
        file_size.saturating_sub(4096)
    };
    file.seek(SeekFrom::Start(seek_pos)).ok()?;

    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;
    *last_size = file_size;

    // Look for meaningful status lines in the new log content.
    // NVIDIA installer logs contain lines like:
    //   [Component Name]: Extracting...
    //   Installing <component>...
    //   Extracting <component>...
    let mut best_line = None;
    for line in buf.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Look for lines that indicate component progress
        if trimmed.contains("Installing") || trimmed.contains("Extracting")
            || trimmed.contains("Configuring") || trimmed.contains("Setting up")
        {
            // Clean up the line for display — strip timestamps and noise
            let display = trimmed
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == '-' || c == ':' || c == '.' || c == ' ' || c == 'T' || c == 'Z');
            if !display.is_empty() && display.len() < 120 {
                best_line = Some(display.to_string());
                break;
            }
        }
    }

    best_line
}

/// Opaque handle to an elevated process launched via UAC.
#[cfg(target_os = "windows")]
struct ElevatedProcess {
    handle: usize,
}

#[cfg(target_os = "windows")]
impl ElevatedProcess {
    /// Check if the process has exited. Returns `Some(exit_code)` if done, `None` if still running.
    fn try_wait(&self) -> Option<u32> {
        extern "system" {
            fn WaitForSingleObject(hHandle: usize, dwMilliseconds: u32) -> u32;
            fn GetExitCodeProcess(hProcess: usize, lpExitCode: *mut u32) -> i32;
        }
        const WAIT_OBJECT_0: u32 = 0;
        let result = unsafe { WaitForSingleObject(self.handle, 0) }; // 0ms = non-blocking
        if result == WAIT_OBJECT_0 {
            let mut exit_code: u32 = 1;
            unsafe { GetExitCodeProcess(self.handle, &mut exit_code) };
            Some(exit_code)
        } else {
            None
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for ElevatedProcess {
    fn drop(&mut self) {
        extern "system" {
            fn CloseHandle(hObject: usize) -> i32;
        }
        if self.handle != 0 {
            unsafe { CloseHandle(self.handle) };
        }
    }
}

/// Launch an executable elevated via UAC ("Run as administrator").
/// Returns an ElevatedProcess handle that can be polled for completion.
#[cfg(target_os = "windows")]
fn shell_execute_runas(exe: &std::path::Path, args: &str) -> Result<ElevatedProcess, String> {
    use std::os::windows::ffi::OsStrExt;

    // Wide-string helper
    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
    fn wide_path(p: &std::path::Path) -> Vec<u16> {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let verb = wide("runas");
    let file = wide_path(exe);
    let params = wide(args);

    // SHELLEXECUTEINFOW struct — 112 bytes on 64-bit
    // We build it manually to avoid pulling in the `windows` crate API surface.
    #[repr(C)]
    #[allow(non_snake_case)]
    struct SHELLEXECUTEINFOW {
        cbSize: u32,
        fMask: u32,
        hwnd: usize,
        lpVerb: *const u16,
        lpFile: *const u16,
        lpParameters: *const u16,
        lpDirectory: *const u16,
        nShow: i32,
        hInstApp: usize,
        lpIDList: usize,
        lpClass: *const u16,
        hkeyClass: usize,
        dwHotKey: u32,
        hIcon: usize,   // union with hMonitor
        hProcess: usize,
    }

    const SEE_MASK_NOCLOSEPROCESS: u32 = 0x00000040;
    const SW_SHOWNORMAL: i32 = 1;

    extern "system" {
        fn ShellExecuteExW(pExecInfo: *mut SHELLEXECUTEINFOW) -> i32;
    }

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: 0,
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: params.as_ptr(),
        lpDirectory: std::ptr::null(),
        nShow: SW_SHOWNORMAL,
        hInstApp: 0,
        lpIDList: 0,
        lpClass: std::ptr::null(),
        hkeyClass: 0,
        dwHotKey: 0,
        hIcon: 0,
        hProcess: 0,
    };

    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok == 0 || info.hProcess == 0 {
        return Err(
            "Failed to launch installer with administrator privileges. \
             The UAC prompt may have been declined."
                .to_string(),
        );
    }

    Ok(ElevatedProcess { handle: info.hProcess })
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
/// Window opens immediately; search state is pre-warmed in the background
/// so the UI never blocks on repo scanning.
#[tauri::command]
async fn open_search_window(app: AppHandle) -> Result<(), String> {
    // If the window already exists, just focus it
    if let Some(w) = app.get_webview_window("search") {
        w.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    // Open window IMMEDIATELY — don't wait for state initialization
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

    // Pre-warm search state in background — the first search_find call will
    // wait on the Mutex if init is still running, so no race condition.
    tauri::async_runtime::spawn_blocking(|| {
        let _ = get_search_state();
    });

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

    // 2. Content grep (only if query is >= 2 chars) — parallelized with rayon
    if q.len() >= 2 {
        use rayon::prelude::*;

        let terms: Vec<String> = q.split_whitespace().map(|s| s.to_string()).collect();
        let terms_lower: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();
        let pattern_str = terms.iter().map(|t| regex::escape(t)).collect::<Vec<_>>().join("|");

        if let Ok(pattern) = RegexBuilder::new(&pattern_str).case_insensitive(true).build() {
            let idf_weights: Vec<f64> = terms_lower.iter().map(|t| repo.term_doc_freq.idf(t)).collect();

            // Parallel grep across all files
            let grep_hits: Vec<_> = repo.all_files.par_iter().filter_map(|file| {
                if let Some(ref exts) = ext_filter {
                    if !exts.contains(&file.ext) { return None; }
                }
                let content = std::fs::read_to_string(&file.abs_path).ok()?;
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

                if match_count == 0 { return None; }

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

                Some((file.rel_path.clone(), MergedFind {
                    path: file.rel_path.clone(),
                    filename: fname,
                    dir,
                    ext: ext_str,
                    desc: String::new(),
                    category,
                    name_score: 0.0,
                    grep_score,
                    grep_count: match_count,
                    top_match: best_snippet,
                    top_match_line: best_snippet_line,
                    filename_indices: Vec::new(),
                    terms_matched: file_terms_matched,
                    total_terms: terms_lower.len(),
                }))
            }).collect();

            // Merge parallel grep results into the merged map
            for (path, grep_find) in grep_hits {
                let entry = merged.entry(path).or_insert(grep_find);
                if entry.grep_score == 0.0 {
                    // Entry existed from filename search — fill in grep data
                    entry.grep_score = grep_find.grep_score;
                    entry.grep_count = grep_find.grep_count;
                    entry.top_match = grep_find.top_match;
                    entry.top_match_line = grep_find.top_match_line;
                    entry.terms_matched = grep_find.terms_matched;
                    entry.total_terms = grep_find.total_terms;
                }
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

/// Return status info about indexed repos (for the empty-state display).
#[tauri::command]
async fn search_status() -> Result<serde_json::Value, String> {
    let state = get_search_state()?;
    match tauri::async_runtime::spawn_blocking(move || -> Result<serde_json::Value, String> {
        let s = state.read().map_err(|_| "State lock poisoned".to_string())?;
        let mut repos = Vec::new();
        let mut total_files = 0usize;
        let mut lang_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

        for (_name, repo) in &s.repos {
            let file_count = repo.all_files.len();
            total_files += file_count;

            // Count extensions for language breakdown
            for f in &repo.all_files {
                if !f.ext.is_empty() {
                    *lang_counts.entry(f.ext.clone()).or_default() += 1;
                }
            }

            repos.push(serde_json::json!({
                "name": repo.name,
                "root": repo.root.to_string_lossy(),
                "files": file_count,
                "scanTime": repo.scan_time_ms,
            }));
        }

        // Top languages sorted by count
        let mut langs: Vec<_> = lang_counts.into_iter().collect();
        langs.sort_by(|a, b| b.1.cmp(&a.1));
        let top_langs: Vec<_> = langs.into_iter().take(8).map(|(ext, count)| {
            serde_json::json!({ "ext": ext, "count": count })
        }).collect();

        Ok(serde_json::json!({
            "repos": repos,
            "totalFiles": total_files,
            "topLangs": top_langs,
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
            fix_path,
            check_mcp_status,
            configure_mcp,
            check_completions,
            install_completions,
            detect_gpu,
            install_cuda,
            open_search_window,
            search_find,
            search_read_file,
            search_status,
        ])
        .setup(move |app| {
            if search_mode {
                // Hide the default setup window
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }

                // Open search window IMMEDIATELY — state warms in background
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

                // Pre-warm search state in background thread
                std::thread::spawn(|| {
                    if let Err(e) = get_search_state() {
                        eprintln!("Error initializing search state: {}", e);
                    }
                });

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
