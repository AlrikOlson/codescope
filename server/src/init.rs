//! CLI subcommands `init` and `doctor`.
//!
//! `init` auto-detects 8+ project ecosystems (Rust, Node.js, Go, Python, C/C++,
//! .NET, Unreal Engine, pnpm/uv workspaces) and generates `.codescope.toml` and
//! `.mcp.json` config files. `doctor` diagnoses setup issues.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Ecosystem detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Ecosystem {
    Rust,
    Node,
    Pnpm,
    Go,
    Python,
    CppProject,
    DotNet,
    Unreal,
}

impl Ecosystem {
    fn label(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Node => "Node.js",
            Self::Pnpm => "pnpm",
            Self::Go => "Go",
            Self::Python => "Python",
            Self::CppProject => "C/C++",
            Self::DotNet => ".NET",
            Self::Unreal => "Unreal Engine",
        }
    }

    fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Rust => &["rs", "toml"],
            Self::Node | Self::Pnpm => &["ts", "tsx", "js", "jsx", "json"],
            Self::Go => &["go"],
            Self::Python => &["py", "pyi"],
            Self::CppProject => &["h", "hpp", "cpp", "c", "cc"],
            Self::DotNet => &["cs", "csproj", "sln"],
            Self::Unreal => &["h", "hpp", "cpp", "c", "cc", "cs"],
        }
    }
}

struct DetectedProject {
    ecosystems: Vec<Ecosystem>,
    scan_dirs: Vec<String>,
    extensions: HashSet<String>,
    skip_dirs: Vec<String>,
    workspace_info: Option<String>, // e.g. "12 members across 5 directories"
}

// ---------------------------------------------------------------------------
// Workspace member resolvers
// ---------------------------------------------------------------------------

/// Extract unique top-level directory from a workspace member path.
/// "temporal-runtime/temporal-ecs" -> "temporal-runtime"
/// "packages/*" -> "packages"
/// "src" -> "src"
fn top_level_dir(pattern: &str) -> Option<&str> {
    let clean = pattern.trim_end_matches("/*").trim_end_matches("/**").trim_end_matches('/');
    let top = clean.split('/').next()?;
    if top.is_empty() || top == "." {
        None
    } else {
        Some(top)
    }
}

fn resolve_rust_workspace(root: &Path) -> (Vec<String>, Option<String>) {
    let cargo_path = root.join("Cargo.toml");
    let content = match std::fs::read_to_string(&cargo_path) {
        Ok(c) => c,
        Err(_) => return (fallback_dirs(root, &["src", "crates"]), None),
    };

    let table: toml::Table = match content.parse() {
        Ok(t) => t,
        Err(_) => return (fallback_dirs(root, &["src", "crates"]), None),
    };

    if let Some(workspace) = table.get("workspace").and_then(|v| v.as_table()) {
        if let Some(members) = workspace.get("members").and_then(|v| v.as_array()) {
            let mut dirs = BTreeSet::new();
            let mut member_count = 0;

            for member in members {
                if let Some(m) = member.as_str() {
                    member_count += 1;
                    if let Some(top) = top_level_dir(m) {
                        if root.join(top).is_dir() {
                            dirs.insert(top.to_string());
                        }
                    }
                }
            }

            // Also include "src" if it exists at root level (some workspaces have root src/)
            if root.join("src").is_dir() {
                dirs.insert("src".to_string());
            }

            if !dirs.is_empty() {
                let info = format!("{} members across {} directories", member_count, dirs.len());
                return (dirs.into_iter().collect(), Some(info));
            }
        }
    }

    (fallback_dirs(root, &["src", "crates"]), None)
}

fn resolve_node_workspace(root: &Path) -> (Vec<String>, Option<String>) {
    let pkg_path = root.join("package.json");
    let content = match std::fs::read_to_string(&pkg_path) {
        Ok(c) => c,
        Err(_) => return (fallback_dirs(root, &["src", "lib", "app"]), None),
    };

    let data: serde_json::Value = match serde_json::from_str(&content) {
        Ok(d) => d,
        Err(_) => return (fallback_dirs(root, &["src", "lib", "app"]), None),
    };

    // workspaces can be an array or an object with "packages" key
    let workspace_patterns: Vec<&str> =
        if let Some(arr) = data.get("workspaces").and_then(|v| v.as_array()) {
            arr.iter().filter_map(|v| v.as_str()).collect()
        } else if let Some(arr) =
            data.get("workspaces").and_then(|v| v.get("packages")).and_then(|v| v.as_array())
        {
            arr.iter().filter_map(|v| v.as_str()).collect()
        } else {
            return (fallback_dirs(root, &["src", "lib", "app"]), None);
        };

    if workspace_patterns.is_empty() {
        return (fallback_dirs(root, &["src", "lib", "app"]), None);
    }

    let mut dirs = BTreeSet::new();
    let mut member_count = 0;

    for pattern in &workspace_patterns {
        if let Some(top) = top_level_dir(pattern) {
            if root.join(top).is_dir() {
                dirs.insert(top.to_string());
                // Count subdirs for member count
                if pattern.ends_with("/*") || pattern.ends_with("/**") {
                    if let Ok(entries) = std::fs::read_dir(root.join(top)) {
                        for entry in entries.flatten() {
                            if entry.path().is_dir() {
                                member_count += 1;
                            }
                        }
                    }
                } else {
                    member_count += 1;
                }
            }
        }
    }

    // Also include "src" at root if it exists
    if root.join("src").is_dir() {
        dirs.insert("src".to_string());
    }

    if !dirs.is_empty() {
        let info = format!("{} packages across {} directories", member_count, dirs.len());
        return (dirs.into_iter().collect(), Some(info));
    }

    (fallback_dirs(root, &["src", "lib", "app"]), None)
}

fn resolve_pnpm_workspace(root: &Path) -> (Vec<String>, Option<String>) {
    let yaml_path = root.join("pnpm-workspace.yaml");
    let content = match std::fs::read_to_string(&yaml_path) {
        Ok(c) => c,
        Err(_) => return resolve_node_workspace(root), // fall back to package.json
    };

    // Simple YAML parsing — look for lines like "  - packages/*" or "  - 'apps/*'"
    let mut dirs = BTreeSet::new();
    let mut member_count = 0;

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(pattern) = trimmed.strip_prefix("- ") {
            let pattern = pattern.trim().trim_matches('\'').trim_matches('"');
            if let Some(top) = top_level_dir(pattern) {
                if root.join(top).is_dir() {
                    dirs.insert(top.to_string());
                    if pattern.ends_with("/*") || pattern.ends_with("/**") {
                        if let Ok(entries) = std::fs::read_dir(root.join(top)) {
                            for entry in entries.flatten() {
                                if entry.path().is_dir() {
                                    member_count += 1;
                                }
                            }
                        }
                    } else {
                        member_count += 1;
                    }
                }
            }
        }
    }

    // Also include "src" if it exists
    if root.join("src").is_dir() {
        dirs.insert("src".to_string());
    }

    if !dirs.is_empty() {
        let info = format!("{} packages across {} directories", member_count, dirs.len());
        return (dirs.into_iter().collect(), Some(info));
    }

    resolve_node_workspace(root)
}

fn resolve_go_workspace(root: &Path) -> (Vec<String>, Option<String>) {
    // Check go.work first (Go workspace)
    let work_path = root.join("go.work");
    if let Ok(content) = std::fs::read_to_string(&work_path) {
        let mut dirs = BTreeSet::new();
        let mut in_use_block = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == "use (" {
                in_use_block = true;
                continue;
            }
            if trimmed == ")" {
                in_use_block = false;
                continue;
            }
            // Single-line: use ./cmd/foo
            if let Some(path) = trimmed.strip_prefix("use ") {
                let path = path.trim().trim_start_matches("./");
                if let Some(top) = top_level_dir(path) {
                    if root.join(top).is_dir() {
                        dirs.insert(top.to_string());
                    }
                }
            }
            // Inside use ( ... ) block
            if in_use_block {
                let path = trimmed.trim_start_matches("./");
                if let Some(top) = top_level_dir(path) {
                    if root.join(top).is_dir() {
                        dirs.insert(top.to_string());
                    }
                }
            }
        }

        if !dirs.is_empty() {
            let info = format!("Go workspace with {} modules", dirs.len());
            return (dirs.into_iter().collect(), Some(info));
        }
    }

    // Standard Go project — common dirs
    let dirs = fallback_dirs(root, &["cmd", "pkg", "internal", "api", "src"]);
    (dirs, None)
}

fn resolve_python_workspace(root: &Path) -> (Vec<String>, Option<String>) {
    let pyproject_path = root.join("pyproject.toml");
    if let Ok(content) = std::fs::read_to_string(&pyproject_path) {
        if let Ok(table) = content.parse::<toml::Table>() {
            // Check uv workspace: [tool.uv.workspace] members = [...]
            if let Some(members) = table
                .get("tool")
                .and_then(|v| v.get("uv"))
                .and_then(|v| v.get("workspace"))
                .and_then(|v| v.get("members"))
                .and_then(|v| v.as_array())
            {
                let mut dirs = BTreeSet::new();
                let mut member_count = 0;
                for member in members {
                    if let Some(m) = member.as_str() {
                        member_count += 1;
                        if let Some(top) = top_level_dir(m) {
                            if root.join(top).is_dir() {
                                dirs.insert(top.to_string());
                            }
                        }
                    }
                }
                if !dirs.is_empty() {
                    let info =
                        format!("{} packages across {} directories", member_count, dirs.len());
                    return (dirs.into_iter().collect(), Some(info));
                }
            }
        }
    }

    (fallback_dirs(root, &["src", "lib", "app"]), None)
}

fn resolve_dotnet_dirs(root: &Path) -> (Vec<String>, Option<String>) {
    // Look for *.csproj or *.fsproj files in subdirectories
    let mut dirs = BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Check if this dir contains a project file
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub in sub_entries.flatten() {
                        let name = sub.file_name();
                        let name = name.to_string_lossy();
                        if name.ends_with(".csproj") || name.ends_with(".fsproj") {
                            if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                                dirs.insert(dir_name.to_string());
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
    let info = if !dirs.is_empty() { Some(format!("{} projects", dirs.len())) } else { None };
    (dirs.into_iter().collect(), info)
}

fn resolve_unreal_dirs(root: &Path) -> (Vec<String>, Option<String>) {
    let mut dirs = Vec::new();
    for d in &["Source", "Plugins", "Content"] {
        if root.join(d).is_dir() {
            dirs.push(d.to_string());
        }
    }
    let info = Some("Unreal Engine project".to_string());
    (dirs, info)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return only the directories from `candidates` that actually exist under `root`.
fn fallback_dirs(root: &Path, candidates: &[&str]) -> Vec<String> {
    candidates.iter().filter(|d| root.join(d).is_dir()).map(|d| d.to_string()).collect()
}

/// Detect directories that should be skipped (generated, vendored, build output).
fn detect_skip_dirs(root: &Path) -> Vec<String> {
    let candidates = [
        // Build output
        "target",
        "dist",
        "build",
        "out",
        ".next",
        ".nuxt",
        ".output",
        // Dependencies
        "node_modules",
        "vendor",
        ".venv",
        "venv",
        "__pycache__",
        // Generated
        "generated",
        "gen",
        ".generated",
    ];

    candidates.iter().filter(|d| root.join(d).is_dir()).map(|d| d.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Main detection
// ---------------------------------------------------------------------------

fn detect_project(root: &Path) -> DetectedProject {
    let mut ecosystems = Vec::new();
    let mut scan_dirs = BTreeSet::new();
    let mut extensions = HashSet::new();
    let mut workspace_info = None;

    // --- Rust ---
    if root.join("Cargo.toml").exists() {
        ecosystems.push(Ecosystem::Rust);
        let (dirs, info) = resolve_rust_workspace(root);
        for d in dirs {
            scan_dirs.insert(d);
        }
        if workspace_info.is_none() {
            workspace_info = info;
        }
        for ext in Ecosystem::Rust.extensions() {
            extensions.insert(ext.to_string());
        }
    }

    // --- Node.js (npm/yarn/bun vs pnpm) ---
    if root.join("package.json").exists() {
        if root.join("pnpm-workspace.yaml").exists() {
            ecosystems.push(Ecosystem::Pnpm);
            let (dirs, info) = resolve_pnpm_workspace(root);
            for d in dirs {
                scan_dirs.insert(d);
            }
            if workspace_info.is_none() {
                workspace_info = info;
            }
        } else {
            ecosystems.push(Ecosystem::Node);
            let (dirs, info) = resolve_node_workspace(root);
            for d in dirs {
                scan_dirs.insert(d);
            }
            if workspace_info.is_none() {
                workspace_info = info;
            }
        }
        for ext in Ecosystem::Node.extensions() {
            extensions.insert(ext.to_string());
        }
    }

    // --- Go ---
    if root.join("go.mod").exists() || root.join("go.work").exists() {
        ecosystems.push(Ecosystem::Go);
        let (dirs, info) = resolve_go_workspace(root);
        for d in dirs {
            scan_dirs.insert(d);
        }
        if workspace_info.is_none() {
            workspace_info = info;
        }
        for ext in Ecosystem::Go.extensions() {
            extensions.insert(ext.to_string());
        }
    }

    // --- Python ---
    if root.join("pyproject.toml").exists()
        || root.join("setup.py").exists()
        || root.join("setup.cfg").exists()
    {
        ecosystems.push(Ecosystem::Python);
        let (dirs, info) = resolve_python_workspace(root);
        for d in dirs {
            scan_dirs.insert(d);
        }
        if workspace_info.is_none() {
            workspace_info = info;
        }
        for ext in Ecosystem::Python.extensions() {
            extensions.insert(ext.to_string());
        }
    }

    // --- C/C++ ---
    if root.join("CMakeLists.txt").exists() || root.join("Makefile").exists() {
        ecosystems.push(Ecosystem::CppProject);
        for d in fallback_dirs(root, &["src", "include", "lib"]) {
            scan_dirs.insert(d);
        }
        for ext in Ecosystem::CppProject.extensions() {
            extensions.insert(ext.to_string());
        }
    }

    // --- .NET ---
    let has_sln = std::fs::read_dir(root)
        .ok()
        .map(|entries| entries.flatten().any(|e| e.file_name().to_string_lossy().ends_with(".sln")))
        .unwrap_or(false);
    if has_sln {
        ecosystems.push(Ecosystem::DotNet);
        let (dirs, info) = resolve_dotnet_dirs(root);
        for d in dirs {
            scan_dirs.insert(d);
        }
        if workspace_info.is_none() {
            workspace_info = info;
        }
        for ext in Ecosystem::DotNet.extensions() {
            extensions.insert(ext.to_string());
        }
    }

    // --- Unreal Engine ---
    let has_uproject = std::fs::read_dir(root)
        .ok()
        .map(|entries| {
            entries.flatten().any(|e| e.file_name().to_string_lossy().ends_with(".uproject"))
        })
        .unwrap_or(false);
    if has_uproject {
        ecosystems.push(Ecosystem::Unreal);
        let (dirs, info) = resolve_unreal_dirs(root);
        for d in dirs {
            scan_dirs.insert(d);
        }
        if workspace_info.is_none() {
            workspace_info = info;
        }
        for ext in Ecosystem::Unreal.extensions() {
            extensions.insert(ext.to_string());
        }
    }

    // --- Nested ecosystem scan ---
    // Check immediate subdirectories for ecosystem markers not found at root.
    // Common patterns: server/Cargo.toml, api/go.mod, frontend/package.json, etc.
    let skip_set: HashSet<&str> = [
        "node_modules",
        "target",
        "dist",
        "build",
        ".git",
        "__pycache__",
        "vendor",
        ".venv",
        "venv",
        ".next",
        ".nuxt",
        "out",
        ".output",
        ".idea",
        ".vscode",
        ".vs",
    ]
    .iter()
    .copied()
    .collect();

    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let dir_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            // Skip hidden dirs and known non-source dirs
            if dir_name.starts_with('.') || skip_set.contains(dir_name.as_str()) {
                continue;
            }
            // Skip dirs already in scan_dirs (already detected at root level)
            if scan_dirs.contains(&dir_name) {
                continue;
            }

            // Check for ecosystem markers in this subdirectory
            let mut found_nested = false;

            if path.join("Cargo.toml").exists() && !ecosystems.contains(&Ecosystem::Rust) {
                ecosystems.push(Ecosystem::Rust);
                scan_dirs.insert(dir_name.clone());
                for ext in Ecosystem::Rust.extensions() {
                    extensions.insert(ext.to_string());
                }
                found_nested = true;
            }
            if path.join("package.json").exists()
                && !ecosystems.contains(&Ecosystem::Node)
                && !ecosystems.contains(&Ecosystem::Pnpm)
            {
                ecosystems.push(Ecosystem::Node);
                scan_dirs.insert(dir_name.clone());
                for ext in Ecosystem::Node.extensions() {
                    extensions.insert(ext.to_string());
                }
                found_nested = true;
            }
            if (path.join("go.mod").exists() || path.join("go.work").exists())
                && !ecosystems.contains(&Ecosystem::Go)
            {
                ecosystems.push(Ecosystem::Go);
                scan_dirs.insert(dir_name.clone());
                for ext in Ecosystem::Go.extensions() {
                    extensions.insert(ext.to_string());
                }
                found_nested = true;
            }
            if (path.join("pyproject.toml").exists() || path.join("setup.py").exists())
                && !ecosystems.contains(&Ecosystem::Python)
            {
                ecosystems.push(Ecosystem::Python);
                scan_dirs.insert(dir_name.clone());
                for ext in Ecosystem::Python.extensions() {
                    extensions.insert(ext.to_string());
                }
                found_nested = true;
            }
            if (path.join("CMakeLists.txt").exists() || path.join("Makefile").exists())
                && !ecosystems.contains(&Ecosystem::CppProject)
            {
                ecosystems.push(Ecosystem::CppProject);
                scan_dirs.insert(dir_name.clone());
                for ext in Ecosystem::CppProject.extensions() {
                    extensions.insert(ext.to_string());
                }
                found_nested = true;
            }

            // Even if ecosystem already detected at root, add subdirs that have their
            // own ecosystem marker (e.g. root has package.json AND server/Cargo.toml)
            if !found_nested {
                let nested_markers = [
                    "Cargo.toml",
                    "package.json",
                    "go.mod",
                    "go.work",
                    "pyproject.toml",
                    "setup.py",
                    "CMakeLists.txt",
                ];
                for marker in &nested_markers {
                    if path.join(marker).exists() {
                        scan_dirs.insert(dir_name.clone());
                        // Add extensions for the nested ecosystem
                        match *marker {
                            "Cargo.toml" => {
                                for ext in Ecosystem::Rust.extensions() {
                                    extensions.insert(ext.to_string());
                                }
                            }
                            "package.json" => {
                                for ext in Ecosystem::Node.extensions() {
                                    extensions.insert(ext.to_string());
                                }
                            }
                            "go.mod" | "go.work" => {
                                for ext in Ecosystem::Go.extensions() {
                                    extensions.insert(ext.to_string());
                                }
                            }
                            "pyproject.toml" | "setup.py" => {
                                for ext in Ecosystem::Python.extensions() {
                                    extensions.insert(ext.to_string());
                                }
                            }
                            "CMakeLists.txt" => {
                                for ext in Ecosystem::CppProject.extensions() {
                                    extensions.insert(ext.to_string());
                                }
                            }
                            _ => {}
                        }
                        break;
                    }
                }
            }
        }
    }

    // --- Fallback: if no workspace dirs found, check for common directories ---
    if scan_dirs.is_empty() && !ecosystems.is_empty() {
        for d in &["src", "lib", "app", "pkg", "cmd", "internal"] {
            if root.join(d).is_dir() {
                scan_dirs.insert(d.to_string());
            }
        }
    }

    // If STILL empty, leave scan_dirs empty (scanner will scan everything from root)

    let skip_dirs = detect_skip_dirs(root);

    DetectedProject {
        ecosystems,
        scan_dirs: scan_dirs.into_iter().collect(),
        extensions,
        skip_dirs,
        workspace_info,
    }
}

// ---------------------------------------------------------------------------
// Validation — quick scan to count files
// ---------------------------------------------------------------------------

fn validate_scan(root: &Path, scan_dirs: &[String], extensions: &HashSet<String>) -> usize {
    let dirs_to_scan: Vec<String> =
        if scan_dirs.is_empty() { vec![".".to_string()] } else { scan_dirs.to_vec() };

    let default_skip: HashSet<&str> = [
        "node_modules",
        "target",
        "dist",
        "build",
        ".git",
        "__pycache__",
        "vendor",
        ".venv",
        "venv",
        ".next",
        ".nuxt",
        "out",
        ".output",
    ]
    .iter()
    .copied()
    .collect();

    let mut count = 0usize;
    let limit = 10_000; // Cap to avoid slow scans on huge repos

    for dir_name in &dirs_to_scan {
        let dir = if dir_name == "." { root.to_path_buf() } else { root.join(dir_name) };
        if !dir.exists() {
            continue;
        }

        let walker = ignore::WalkBuilder::new(&dir).hidden(true).git_ignore(true).build();

        for entry in walker.flatten() {
            if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                continue;
            }

            let path = entry.path();

            // Skip known dirs
            let mut skip = false;
            for component in path.components() {
                if let std::path::Component::Normal(name) = component {
                    if let Some(name_str) = name.to_str() {
                        if default_skip.contains(name_str) {
                            skip = true;
                            break;
                        }
                    }
                }
            }
            if skip {
                continue;
            }

            // Extension filter
            if !extensions.is_empty() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if !extensions.contains(ext) {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            count += 1;
            if count >= limit {
                return count;
            }
        }
    }

    count
}

// ---------------------------------------------------------------------------
// .codescope.toml generation
// ---------------------------------------------------------------------------

fn generate_codescope_toml(detection: &DetectedProject) -> String {
    let mut out = String::new();
    out.push_str("# Generated by codescope-server init\n");

    // Project type label
    let labels: Vec<&str> = detection.ecosystems.iter().map(|e| e.label()).collect();
    if labels.is_empty() {
        out.push_str("# Project type: Unknown\n\n");
    } else {
        out.push_str(&format!("# Project type: {}\n\n", labels.join(" + ")));
    }

    if !detection.scan_dirs.is_empty() {
        let quoted: Vec<String> =
            detection.scan_dirs.iter().map(|d| format!("\"{}\"", d)).collect();
        out.push_str(&format!("scan_dirs = [{}]\n", quoted.join(", ")));
    }

    if !detection.extensions.is_empty() {
        let mut exts: Vec<&String> = detection.extensions.iter().collect();
        exts.sort();
        let quoted: Vec<String> = exts.iter().map(|e| format!("\"{}\"", e)).collect();
        out.push_str(&format!("extensions = [{}]\n", quoted.join(", ")));
    }

    if !detection.skip_dirs.is_empty() {
        let quoted: Vec<String> =
            detection.skip_dirs.iter().map(|d| format!("\"{}\"", d)).collect();
        out.push_str(&format!("skip_dirs = [{}]\n", quoted.join(", ")));
    }

    out
}

// ---------------------------------------------------------------------------
// .mcp.json generation / merge
// ---------------------------------------------------------------------------

fn codescope_mcp_entry(root: &Path) -> serde_json::Value {
    serde_json::json!({
        "type": "stdio",
        "command": "codescope-server",
        "args": ["--mcp", "--root", root.to_string_lossy()]
    })
}

fn write_or_merge_mcp_json(root: &Path) -> Result<(), String> {
    let mcp_path = root.join(".mcp.json");
    let entry = codescope_mcp_entry(root);

    if mcp_path.exists() {
        let content = std::fs::read_to_string(&mcp_path)
            .map_err(|e| format!("Failed to read {}: {}", mcp_path.display(), e))?;
        let mut data: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", mcp_path.display(), e))?;

        // Check if codescope already exists
        if let Some(servers) = data.get("mcpServers").and_then(|v| v.as_object()) {
            if servers.contains_key("codescope") {
                eprintln!("  codescope already configured in .mcp.json");
                return Ok(());
            }
        }

        // Merge
        let servers = data
            .as_object_mut()
            .ok_or("Invalid .mcp.json: not an object")?
            .entry("mcpServers")
            .or_insert_with(|| serde_json::json!({}));
        servers
            .as_object_mut()
            .ok_or("Invalid .mcp.json: mcpServers is not an object")?
            .insert("codescope".to_string(), entry);

        let output = serde_json::to_string_pretty(&data)
            .map_err(|e| format!("Failed to serialize .mcp.json: {}", e))?;
        std::fs::write(&mcp_path, format!("{}\n", output))
            .map_err(|e| format!("Failed to write {}: {}", mcp_path.display(), e))?;
        eprintln!("  Added codescope to existing .mcp.json");
    } else {
        let data = serde_json::json!({
            "mcpServers": {
                "codescope": entry
            }
        });
        let output = serde_json::to_string_pretty(&data)
            .map_err(|e| format!("Failed to serialize .mcp.json: {}", e))?;
        std::fs::write(&mcp_path, format!("{}\n", output))
            .map_err(|e| format!("Failed to write {}: {}", mcp_path.display(), e))?;
        eprintln!("  Created .mcp.json");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Global repos.toml merge
// ---------------------------------------------------------------------------

fn merge_global_repos_toml(root: &Path) -> Result<(), String> {
    let repo_name = root.file_name().and_then(|n| n.to_str()).unwrap_or("default");
    crate::merge_global_repos_toml(repo_name, root)?;
    eprintln!("  Added '{}' to ~/.codescope/repos.toml", repo_name);
    Ok(())
}

// ---------------------------------------------------------------------------
// codescope-server init
// ---------------------------------------------------------------------------

/// Auto-detect project ecosystem and generate `.codescope.toml` + `.mcp.json` config files.
pub fn run_init(args: &[String]) -> i32 {
    let global = args.iter().any(|a| a == "--global");
    #[cfg(feature = "semantic")]
    let build_semantic = args.iter().any(|a| a == "--semantic");

    // Find the path argument (skip "init", skip flags)
    let path_arg = args
        .iter()
        .skip(1) // skip "init"
        .find(|a| !a.starts_with('-'));

    let root = match path_arg {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().unwrap_or_else(|e| {
            eprintln!("Error: Could not determine current directory: {}", e);
            std::process::exit(1);
        }),
    };

    let root = root.canonicalize().unwrap_or_else(|e| {
        eprintln!("Error: Path '{}' not found: {}", root.display(), e);
        std::process::exit(1);
    });

    let version = env!("CARGO_PKG_VERSION");
    eprintln!("codescope-server {} init", version);
    eprintln!("  Project root: {}", root.display());

    // Detect project ecosystems and workspace structure
    let detection = detect_project(&root);

    // Report what was detected
    if detection.ecosystems.is_empty() {
        eprintln!("  Detected: no recognized project type");
        eprintln!("  Will scan all files from project root");
    } else {
        let labels: Vec<&str> = detection.ecosystems.iter().map(|e| e.label()).collect();
        let type_str = labels.join(" + ");
        if let Some(ref info) = detection.workspace_info {
            eprintln!("  Detected: {} ({info})", type_str);
        } else {
            eprintln!("  Detected: {} project", type_str);
        }
    }

    if !detection.scan_dirs.is_empty() {
        eprintln!("  Scan dirs: {:?}", detection.scan_dirs);
    }

    // Generate .codescope.toml
    let config_path = root.join(".codescope.toml");
    if config_path.exists() {
        eprintln!("  .codescope.toml already exists, skipping");
    } else {
        let toml_content = generate_codescope_toml(&detection);
        if let Err(e) = std::fs::write(&config_path, &toml_content) {
            eprintln!("Error: Failed to write .codescope.toml: {}", e);
            return 1;
        }
        eprintln!("  Created .codescope.toml");
    }

    // Generate or merge .mcp.json
    if let Err(e) = write_or_merge_mcp_json(&root) {
        eprintln!("Error: {}", e);
        return 1;
    }

    // Global repos.toml
    if global {
        if let Err(e) = merge_global_repos_toml(&root) {
            eprintln!("Error: {}", e);
            return 1;
        }
    }

    // Validate — quick scan to verify files are found
    let file_count = validate_scan(&root, &detection.scan_dirs, &detection.extensions);
    if file_count > 0 {
        if file_count >= 10_000 {
            eprintln!("  Validated: 10,000+ source files found");
        } else {
            eprintln!("  Validated: {} source files found", file_count);
        }
    } else {
        eprintln!("  [WARN] No source files found with current settings.");
        eprintln!("         Try removing scan_dirs from .codescope.toml to scan everything.");
    }

    // Build semantic index if requested (pre-populates centralized cache)
    #[cfg(feature = "semantic")]
    if build_semantic {
        eprintln!("  Building semantic index...");
        let config = crate::load_codescope_config(&root);
        let (all_files, _categories) = crate::scan::scan_files(&config);
        let progress = crate::types::SemanticProgress::new();
        let sem_model: Option<String> = config.semantic_model.clone();
        let start = std::time::Instant::now();
        match crate::semantic::build_semantic_index(
            &all_files,
            sem_model.as_deref(),
            &progress,
            &root,
        ) {
            Some(idx) => {
                let chunks: usize = idx.chunk_meta.len();
                eprintln!(
                    "  Semantic index built: {} chunks in {:.1}s (cached to ~/.cache/codescope/)",
                    chunks,
                    start.elapsed().as_secs_f64()
                );
            }
            None => {
                eprintln!("  [WARN] Semantic index build failed (non-fatal)");
            }
        }
    }

    eprintln!();
    eprintln!("  Open Claude Code in {} -- CodeScope tools are now available.", root.display());
    0
}

// ---------------------------------------------------------------------------
// codescope-server doctor
// ---------------------------------------------------------------------------

/// Diagnose CodeScope setup issues: check config files, binary location, and MCP integration.
pub fn run_doctor(args: &[String]) -> i32 {
    // Find the path argument (skip "doctor", skip flags)
    let path_arg = args
        .iter()
        .skip(1) // skip "doctor"
        .find(|a| !a.starts_with('-'));

    let root = match path_arg {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().unwrap_or_else(|e| {
            eprintln!("Error: Could not determine current directory: {}", e);
            std::process::exit(1);
        }),
    };

    let root = root.canonicalize().unwrap_or_else(|e| {
        eprintln!("Error: Path '{}' not found: {}", root.display(), e);
        std::process::exit(1);
    });

    let version = env!("CARGO_PKG_VERSION");
    let mut has_warn = false;
    let mut has_fail = false;

    eprintln!("codescope-server doctor");
    eprintln!();

    // 1. Binary version
    eprintln!("  [PASS] codescope-server v{}", version);

    // 2. Check .codescope.toml
    let config_path = root.join(".codescope.toml");
    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path).unwrap_or_default();
        match content.parse::<toml::Table>() {
            Ok(_) => eprintln!("  [PASS] .codescope.toml exists and is valid TOML"),
            Err(e) => {
                eprintln!("  [FAIL] .codescope.toml exists but is invalid: {}", e);
                has_fail = true;
            }
        }
    } else {
        eprintln!("  [WARN] .codescope.toml not found (will use defaults)");
        has_warn = true;
    }

    // 3. Check .mcp.json
    let mcp_path = root.join(".mcp.json");
    if mcp_path.exists() {
        let content = std::fs::read_to_string(&mcp_path).unwrap_or_default();
        match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(data) => {
                if data.get("mcpServers").and_then(|v| v.get("codescope")).is_some() {
                    eprintln!("  [PASS] .mcp.json has codescope entry");
                } else {
                    eprintln!("  [WARN] .mcp.json exists but missing codescope entry");
                    has_warn = true;
                }
            }
            Err(e) => {
                eprintln!("  [FAIL] .mcp.json exists but is invalid JSON: {}", e);
                has_fail = true;
            }
        }
    } else {
        eprintln!("  [FAIL] .mcp.json not found (run: codescope-server init)");
        has_fail = true;
    }

    // 4. Quick test scan (limit 100 files)
    let config = crate::load_codescope_config(&root);

    let scan_dirs: Vec<String> =
        if config.scan_dirs.is_empty() { vec![".".to_string()] } else { config.scan_dirs.clone() };

    let start = std::time::Instant::now();
    let mut file_count: usize = 0;
    let mut estimated_total: usize = 0;
    let scan_limit = 100;

    for dir_name in &scan_dirs {
        let scan_root = if dir_name == "." { root.clone() } else { root.join(dir_name) };
        if !scan_root.exists() {
            continue;
        }

        let walker = ignore::WalkBuilder::new(&scan_root).hidden(true).git_ignore(true).build();

        for entry in walker.flatten() {
            if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                continue;
            }

            let path = entry.path();

            // Apply skip_dirs
            let mut skip = false;
            for component in path.components() {
                if let std::path::Component::Normal(name) = component {
                    if let Some(name_str) = name.to_str() {
                        if config.skip_dirs.contains(name_str) {
                            skip = true;
                            break;
                        }
                    }
                }
            }
            if skip {
                continue;
            }

            // Apply extension filter
            if !config.extensions.is_empty() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if !config.extensions.contains(ext) {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            estimated_total += 1;
            if file_count < scan_limit {
                file_count += 1;
            }
        }
    }
    let elapsed = start.elapsed();

    if file_count > 0 {
        eprintln!("  [PASS] Test scan: found {} files in {:.0?}", file_count, elapsed);
    } else {
        eprintln!("  [WARN] Test scan: no files found");
        has_warn = true;
    }

    // 5. Total estimated file count
    eprintln!("  [INFO] Estimated total files: {}", estimated_total);

    // 6. Check for nested .git dirs (too-broad root)
    let mut git_dirs = 0;
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join(".git").exists() {
                git_dirs += 1;
            }
        }
    }
    if git_dirs > 1 {
        eprintln!("  [WARN] Found {} subdirectories with .git -- root may be too broad", git_dirs);
        has_warn = true;
    }

    // Summary
    eprintln!();
    if has_fail {
        eprintln!("  Result: FAIL -- fix the issues above");
        1
    } else if has_warn {
        eprintln!("  Result: PASS with warnings");
        0
    } else {
        eprintln!("  Result: ALL PASS");
        0
    }
}
