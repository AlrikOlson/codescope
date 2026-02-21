// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use codescope_server::init;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

/// Scan common directories for project repos.
/// Only does cheap marker detection — skips full file counting for speed.
#[tauri::command]
fn scan_for_repos(dirs: Vec<String>) -> Vec<RepoInfo> {
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
    // Heavy dirs to skip when scanning home
    let skip_dirs = [
        "node_modules", ".cargo", ".rustup", ".cache", ".local",
        ".npm", ".nvm", ".pyenv", ".conda", "snap", ".steam",
    ];

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
            // Deduplicate across scan dirs
            let canonical = entry_path.canonicalize().unwrap_or(entry_path.clone());
            if !seen.insert(canonical) {
                continue;
            }
            // Check for project markers (cheap fs::exists checks)
            let has_marker = project_markers.iter().any(|m| entry_path.join(m).exists());
            if !has_marker {
                continue;
            }
            // Quick ecosystem detection — skip expensive file counting
            let detection = init::detect_project(&entry_path);
            let ecosystems: Vec<String> =
                detection.ecosystems.iter().map(|e| e.label().to_string()).collect();

            repos.push(RepoInfo {
                path: entry_path.to_string_lossy().to_string(),
                name,
                ecosystems,
                workspace_info: detection.workspace_info,
                file_count: 0, // filled on demand, not during scan
            });
        }
    }
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
    }
}

/// Initialize a project (generate .codescope.toml + .mcp.json + register globally).
#[tauri::command]
fn init_repo(path: String, semantic: bool) -> Result<String, String> {
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

    // Build semantic index if requested
    if semantic {
        init::build_semantic_index(&root, None);
    }

    let labels: Vec<&str> = detection.ecosystems.iter().map(|e| e.label()).collect();
    Ok(format!(
        "Initialized {} ({} files)",
        labels.join(" + "),
        init::validate_scan(&root, &detection.scan_dirs, &detection.extensions)
    ))
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
            get_config,
            get_version,
            get_scan_dirs,
            check_on_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running CodeScope Setup");
}
