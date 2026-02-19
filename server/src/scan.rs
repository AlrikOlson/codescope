use crate::fuzzy::char_bitmask;
use crate::types::*;
use ignore::WalkBuilder;
use rayon::prelude::*;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Descriptions and categories
// ---------------------------------------------------------------------------

/// Generate a human-readable description for a file by splitting its stem into words and appending a language hint.
pub fn describe(rel_path: &str) -> String {
    let file_name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let stem = file_name.rsplit_once('.').map(|(s, _)| s).unwrap_or(file_name);

    // CamelCase word splitting
    let mut words = String::new();
    let chars: Vec<char> = stem.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 {
            let prev = chars[i - 1];
            if (prev.is_lowercase() && c.is_uppercase())
                || (i + 1 < chars.len()
                    && prev.is_uppercase()
                    && c.is_uppercase()
                    && chars[i + 1].is_lowercase())
            {
                words.push(' ');
            }
        }
        if c == '_' || c == '-' {
            words.push(' ');
        } else {
            words.push(c);
        }
    }
    let words = words.trim().to_string();

    let ext = rel_path.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let hint = match ext {
        // Headers
        "h" | "hpp" | "hxx" => "header",
        // Implementations
        "cpp" | "cxx" | "cc" | "c" => "impl",
        // Shaders
        "usf" | "ush" | "hlsl" | "glsl" | "vert" | "frag" | "comp" | "wgsl" => "shader",
        // Config
        "ini" | "cfg" | "conf" | "toml" | "yaml" | "yml" | "json" | "xml" => "config",
        // Scripts
        "py" | "rb" | "lua" | "sh" | "bash" | "zsh" | "ps1" | "psm1" | "psd1" | "bat"
        | "cmd" => "script",
        // C# source
        "cs" => "source",
        // Build
        "csproj" | "sln" | "cmake" | "make" | "gradle" | "props" | "targets" => "build",
        // Web source
        "js" | "ts" | "jsx" | "tsx" | "mjs" | "cjs" => "source",
        // Style
        "css" | "scss" | "less" | "sass" => "style",
        // Template
        "html" | "htm" | "vue" | "svelte" => "template",
        // Primary languages
        "rs" | "go" | "java" | "kt" | "scala" | "swift" => "source",
        // Docs
        "md" | "rst" | "txt" | "adoc" => "doc",
        _ => "",
    };
    if hint.is_empty() {
        words
    } else {
        format!("{words} ({hint})")
    }
}

/// Derive a category path (breadcrumb trail) from a file's directory, stripping noise dirs and scan prefixes.
pub fn get_category_path(rel_path: &str, config: &ScanConfig) -> Vec<String> {
    let mut parts: Vec<&str> = rel_path.split('/').collect();

    // Strip any matching scan_dirs prefix
    for scan_dir in &config.scan_dirs {
        let prefix_parts: Vec<&str> = scan_dir.split('/').collect();
        if parts.len() > prefix_parts.len()
            && parts[..prefix_parts.len()] == prefix_parts[..]
        {
            parts = parts[prefix_parts.len()..].to_vec();
            break;
        }
    }

    // Remove the filename
    if !parts.is_empty() {
        parts.pop();
    }

    // Filter out noise directories
    let filtered: Vec<String> = parts
        .into_iter()
        .filter(|p| !config.noise_dirs.contains(*p))
        .map(|s| s.to_string())
        .collect();

    if filtered.is_empty() {
        vec!["Other".to_string()]
    } else if filtered.len() > 5 {
        filtered[..5].to_vec()
    } else {
        filtered
    }
}

// ---------------------------------------------------------------------------
// Binary file detection
// ---------------------------------------------------------------------------

/// Check if a file appears to be text by reading the first 8KB and looking for null bytes.
fn is_text_file(path: &Path) -> bool {
    let mut file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = [0u8; 8192];
    let n = match std::io::Read::read(&mut file, &mut buf) {
        Ok(n) => n,
        Err(_) => return false,
    };
    !buf[..n].contains(&0)
}

// ---------------------------------------------------------------------------
// Parallel file walking helper
// ---------------------------------------------------------------------------

/// Collect files matching an extension filter using parallel directory walk.
fn walk_files_parallel(
    project_root: &Path,
    scan_dirs: &[String],
    skip_dirs: &HashSet<String>,
    ext_filter: Option<&HashSet<String>>,
) -> Vec<(std::path::PathBuf, String)> {
    let results: Mutex<Vec<(std::path::PathBuf, String)>> = Mutex::new(Vec::new());

    for scan_dir in scan_dirs {
        let dir = project_root.join(scan_dir);
        if !dir.exists() {
            eprintln!("  Skipping {scan_dir} (not found)");
            continue;
        }

        let skip = skip_dirs.clone();
        WalkBuilder::new(&dir)
            .hidden(true)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .threads(rayon::current_num_threads().min(12))
            .filter_entry(move |entry| {
                if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                    let name = entry.file_name().to_string_lossy();
                    return !skip.contains(name.as_ref());
                }
                true
            })
            .build_parallel()
            .run(|| {
                Box::new(|entry| {
                    let entry = match entry {
                        Ok(e) => e,
                        Err(_) => return ignore::WalkState::Continue,
                    };
                    if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                        return ignore::WalkState::Continue;
                    }

                    let abs_path = entry.path().to_path_buf();
                    let ext_str = abs_path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("");

                    if let Some(exts) = ext_filter {
                        if !exts.contains(ext_str) {
                            return ignore::WalkState::Continue;
                        }
                    }

                    let rel_path = abs_path
                        .strip_prefix(project_root)
                        .unwrap_or(&abs_path)
                        .to_string_lossy()
                        .replace('\\', "/");

                    results.lock().unwrap().push((abs_path, rel_path));
                    ignore::WalkState::Continue
                })
            });
    }

    results.into_inner().unwrap()
}

// ---------------------------------------------------------------------------
// File scanning
// ---------------------------------------------------------------------------

/// Walk the project directory tree and return all discovered files plus a category-keyed manifest.
pub fn scan_files(config: &ScanConfig) -> (Vec<ScannedFile>, BTreeMap<String, Vec<FileEntry>>) {
    // If scan_dirs is empty, scan root itself
    let scan_dirs: Vec<String> = if config.scan_dirs.is_empty() {
        vec![".".to_string()]
    } else {
        config.scan_dirs.clone()
    };

    // Extension filter: None means scan all (with text check)
    let ext_filter: Option<HashSet<String>> = if config.extensions.is_empty() {
        None
    } else {
        Some(config.extensions.clone())
    };

    // Parallel walk
    let raw_files = walk_files_parallel(
        &config.root,
        &scan_dirs,
        &config.skip_dirs,
        ext_filter.as_ref(),
    );

    // If no extension filter, apply binary file check
    let raw_files: Vec<(std::path::PathBuf, String)> = if ext_filter.is_none() {
        raw_files
            .into_par_iter()
            .filter(|(abs_path, _)| is_text_file(abs_path))
            .collect()
    } else {
        raw_files
    };

    // Process in parallel with rayon
    let processed: Vec<(ScannedFile, String, FileEntry)> = raw_files
        .par_iter()
        .map(|(abs_path, rel_path)| {
            let size = fs::metadata(abs_path).map(|m| m.len()).unwrap_or(0);
            let desc = describe(rel_path);
            let cat_parts = get_category_path(rel_path, config);
            let cat_key = cat_parts.join(" > ");
            let ext = abs_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_string();

            let scanned = ScannedFile {
                rel_path: rel_path.clone(),
                abs_path: abs_path.clone(),
                desc: desc.clone(),
                ext,
            };
            let entry = FileEntry {
                path: rel_path.clone(),
                desc,
                size,
            };
            (scanned, cat_key, entry)
        })
        .collect();

    let mut all_files = Vec::with_capacity(processed.len());
    let mut category_files: BTreeMap<String, Vec<FileEntry>> = BTreeMap::new();

    for (scanned, cat_key, entry) in processed {
        category_files.entry(cat_key).or_default().push(entry);
        all_files.push(scanned);
    }

    for files in category_files.values_mut() {
        files.sort_by(|a, b| a.path.cmp(&b.path));
    }

    (all_files, category_files)
}

// ---------------------------------------------------------------------------
// Tree and dependency building
// ---------------------------------------------------------------------------

/// Build a nested JSON tree from the flat category manifest for the tree API endpoint.
pub fn build_tree(manifest: &BTreeMap<String, Vec<FileEntry>>) -> serde_json::Value {
    let mut root = serde_json::Map::new();

    for (cat_key, files) in manifest {
        let parts: Vec<&str> = cat_key.split(" > ").collect();
        let mut node = &mut root;

        for part in &parts {
            let entry = node
                .entry(part.to_string())
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            node = entry.as_object_mut().unwrap();
        }

        let file_entries: Vec<serde_json::Value> = files
            .iter()
            .map(|f| {
                serde_json::json!({
                    "path": f.path,
                    "desc": f.desc,
                    "size": f.size,
                })
            })
            .collect();

        node.insert("_files".to_string(), serde_json::Value::Array(file_entries));
    }

    serde_json::Value::Object(root)
}

// ---------------------------------------------------------------------------
// Dependency scanning — trait-based, multi-language
// ---------------------------------------------------------------------------

/// A dependency scanner detects and parses project dependency files.
pub trait DependencyScanner: Send + Sync {
    /// Does this scanner handle the given file?
    fn matches(&self, abs_path: &Path) -> bool;
    /// Extract a module/package name from the file path.
    fn module_name(&self, abs_path: &Path) -> Option<String>;
    /// Parse dependencies from file content. Returns (public/direct, private/dev).
    fn parse_deps(&self, content: &str) -> (Vec<String>, Vec<String>);
}

/// Cargo.toml scanner
struct CargoTomlScanner;

impl DependencyScanner for CargoTomlScanner {
    fn matches(&self, abs_path: &Path) -> bool {
        abs_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == "Cargo.toml")
            .unwrap_or(false)
    }

    fn module_name(&self, abs_path: &Path) -> Option<String> {
        // Try to read [package].name, fall back to directory name
        if let Ok(content) = fs::read_to_string(abs_path) {
            let name_re = regex::Regex::new(r#"(?m)^\s*name\s*=\s*"([^"]+)""#).unwrap();
            if let Some(cap) = name_re.captures(&content) {
                return Some(cap[1].to_string());
            }
        }
        abs_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
    }

    fn parse_deps(&self, content: &str) -> (Vec<String>, Vec<String>) {
        let dep_key_re = regex::Regex::new(r#"(?m)^(\w[\w-]*)\s*="#).unwrap();
        let mut public = Vec::new();
        let mut private = Vec::new();
        let mut in_deps = false;
        let mut in_dev_deps = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("[dependencies]") {
                in_deps = true;
                in_dev_deps = false;
                continue;
            }
            if trimmed.starts_with("[dev-dependencies]") {
                in_deps = false;
                in_dev_deps = true;
                continue;
            }
            if trimmed.starts_with('[') {
                in_deps = false;
                in_dev_deps = false;
                continue;
            }
            if in_deps {
                if let Some(cap) = dep_key_re.captures(trimmed) {
                    public.push(cap[1].to_string());
                }
            } else if in_dev_deps {
                if let Some(cap) = dep_key_re.captures(trimmed) {
                    private.push(cap[1].to_string());
                }
            }
        }

        (public, private)
    }
}

/// package.json scanner
struct PackageJsonScanner;

impl DependencyScanner for PackageJsonScanner {
    fn matches(&self, abs_path: &Path) -> bool {
        abs_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == "package.json")
            .unwrap_or(false)
    }

    fn module_name(&self, abs_path: &Path) -> Option<String> {
        if let Ok(content) = fs::read_to_string(abs_path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(name) = json["name"].as_str() {
                    return Some(name.to_string());
                }
            }
        }
        abs_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
    }

    fn parse_deps(&self, content: &str) -> (Vec<String>, Vec<String>) {
        let json: serde_json::Value = match serde_json::from_str(content) {
            Ok(v) => v,
            Err(_) => return (vec![], vec![]),
        };

        let extract_keys = |key: &str| -> Vec<String> {
            json[key]
                .as_object()
                .map(|obj| obj.keys().cloned().collect())
                .unwrap_or_default()
        };

        (extract_keys("dependencies"), extract_keys("devDependencies"))
    }
}

/// go.mod scanner
struct GoModScanner;

impl DependencyScanner for GoModScanner {
    fn matches(&self, abs_path: &Path) -> bool {
        abs_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == "go.mod")
            .unwrap_or(false)
    }

    fn module_name(&self, abs_path: &Path) -> Option<String> {
        if let Ok(content) = fs::read_to_string(abs_path) {
            let module_re = regex::Regex::new(r"(?m)^module\s+(\S+)").unwrap();
            if let Some(cap) = module_re.captures(&content) {
                // Use the last path component as the short name
                let full = &cap[1];
                return Some(
                    full.rsplit('/')
                        .next()
                        .unwrap_or(full)
                        .to_string(),
                );
            }
        }
        abs_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
    }

    fn parse_deps(&self, content: &str) -> (Vec<String>, Vec<String>) {
        let require_re = regex::Regex::new(r#"(?m)^\s+(\S+)\s+v"#).unwrap();
        let mut public = Vec::new();
        let mut in_require = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("require (") || trimmed == "require (" {
                in_require = true;
                continue;
            }
            if in_require && trimmed == ")" {
                in_require = false;
                continue;
            }
            if in_require {
                if let Some(cap) = require_re.captures(line) {
                    // Use last path component as short name
                    let full = &cap[1];
                    let short = full.rsplit('/').next().unwrap_or(full);
                    public.push(short.to_string());
                }
            }
        }

        (public, vec![])
    }
}

/// Get the default set of dependency scanners.
fn default_scanners() -> Vec<Box<dyn DependencyScanner>> {
    vec![
        Box::new(CargoTomlScanner),
        Box::new(PackageJsonScanner),
        Box::new(GoModScanner),
    ]
}

/// Scan for dependency manifest files (Cargo.toml, package.json, go.mod) and extract module dependencies.
pub fn scan_deps(config: &ScanConfig) -> BTreeMap<String, DepEntry> {
    let scanners = default_scanners();

    let scan_dirs: Vec<String> = if config.scan_dirs.is_empty() {
        vec![".".to_string()]
    } else {
        config.scan_dirs.clone()
    };

    // Walk all files — no ext filter, scanners decide what they match
    let raw_files = walk_files_parallel(&config.root, &scan_dirs, &config.skip_dirs, None);

    // Process matching files in parallel
    let entries: Vec<(String, DepEntry)> = raw_files
        .par_iter()
        .filter_map(|(abs_path, _rel_path)| {
            // Find the first scanner that matches this file
            let scanner = scanners.iter().find(|s| s.matches(abs_path))?;
            let module_name = scanner.module_name(abs_path)?;
            let content = fs::read_to_string(abs_path).ok()?;
            let (public, private) = scanner.parse_deps(&content);

            let rel_dir = abs_path
                .parent()?
                .strip_prefix(&config.root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");

            let cat_parts: Vec<&str> = rel_dir.split('/').collect();
            // Strip scan_dirs prefix
            let mut filtered_parts = cat_parts.clone();
            for scan_dir in &config.scan_dirs {
                let prefix_parts: Vec<&str> = scan_dir.split('/').collect();
                if filtered_parts.len() > prefix_parts.len()
                    && filtered_parts[..prefix_parts.len()] == prefix_parts[..]
                {
                    filtered_parts = filtered_parts[prefix_parts.len()..].to_vec();
                    break;
                }
            }
            let filtered: Vec<&str> = filtered_parts
                .into_iter()
                .filter(|p| !config.noise_dirs.contains(*p))
                .collect();
            let category_path = filtered.join(" > ");

            Some((
                module_name,
                DepEntry {
                    public,
                    private,
                    category_path,
                },
            ))
        })
        .collect();

    entries.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Search index
// ---------------------------------------------------------------------------

/// Build the fuzzy search index from the manifest, producing file and module entries with pre-computed bitmasks.
pub fn build_search_index(
    manifest: &BTreeMap<String, Vec<FileEntry>>,
) -> (Vec<SearchFileEntry>, Vec<SearchModuleEntry>) {
    // Flatten all (category, entries) pairs into a vec for parallel iteration
    let all_entries: Vec<(&String, &Vec<FileEntry>)> = manifest.iter().collect();

    let modules: Vec<SearchModuleEntry> = all_entries
        .par_iter()
        .map(|(category, entries)| {
            let parts: Vec<&str> = category.split(" > ").collect();
            let name = parts.last().unwrap_or(&"").to_string();
            let id_lower = category.to_lowercase();
            let name_lower = name.to_lowercase();
            SearchModuleEntry {
                name_mask: char_bitmask(&name_lower),
                id_mask: char_bitmask(&id_lower),
                id: (*category).clone(),
                id_lower,
                name_lower,
                name,
                file_count: entries.len(),
            }
        })
        .collect();

    let files: Vec<SearchFileEntry> = all_entries
        .par_iter()
        .flat_map(|(category, entries)| {
            entries
                .iter()
                .map(|entry| {
                    let filename = entry
                        .path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&entry.path)
                        .to_string();
                    let dir = entry
                        .path
                        .rsplit_once('/')
                        .map(|(d, _)| d.to_string())
                        .unwrap_or_default();
                    let ext = entry
                        .path
                        .rsplit_once('.')
                        .map(|(_, e)| format!(".{e}"))
                        .unwrap_or_default();

                    let path_lower = entry.path.to_lowercase();
                    let filename_lower = filename.to_lowercase();
                    let desc_lower = entry.desc.to_lowercase();
                    SearchFileEntry {
                        filename_mask: char_bitmask(&filename_lower),
                        path_mask: char_bitmask(&path_lower),
                        desc_mask: char_bitmask(&desc_lower),
                        path: entry.path.clone(),
                        path_lower,
                        filename_lower,
                        filename,
                        dir,
                        ext,
                        desc: entry.desc.clone(),
                        desc_lower,
                        category: (*category).clone(),
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect();

    (files, modules)
}

// ---------------------------------------------------------------------------
// Import graph — multi-language import/include resolution
// ---------------------------------------------------------------------------

/// Extension families for import pattern matching
fn import_exts_cpp() -> HashSet<&'static str> {
    ["h", "cpp", "c", "cc", "cxx", "hpp", "hxx", "usf", "ush", "hlsl"]
        .iter()
        .copied()
        .collect()
}

fn import_exts_python() -> HashSet<&'static str> {
    ["py"].iter().copied().collect()
}

fn import_exts_js() -> HashSet<&'static str> {
    ["js", "ts", "jsx", "tsx", "mjs", "cjs"]
        .iter()
        .copied()
        .collect()
}

fn import_exts_rust() -> HashSet<&'static str> {
    ["rs"].iter().copied().collect()
}

fn import_exts_go() -> HashSet<&'static str> {
    ["go"].iter().copied().collect()
}

fn import_exts_csharp() -> HashSet<&'static str> {
    ["cs"].iter().copied().collect()
}

fn import_exts_powershell() -> HashSet<&'static str> {
    ["ps1", "psm1", "psd1"].iter().copied().collect()
}

/// Parse import/include directives across all files and build a bidirectional import graph.
pub fn scan_imports(all_files: &[ScannedFile]) -> ImportGraph {
    let cpp_exts = import_exts_cpp();
    let py_exts = import_exts_python();
    let js_exts = import_exts_js();
    let rust_exts = import_exts_rust();
    let go_exts = import_exts_go();
    let cs_exts = import_exts_csharp();
    let ps_exts = import_exts_powershell();

    // Regex patterns for each language family
    let include_re = regex::Regex::new(r#"#include\s+"([^"]+)""#).unwrap();
    let py_import_re =
        regex::Regex::new(r#"(?m)(?:from\s+([\w.]+)\s+import|^import\s+([\w.]+))"#).unwrap();
    let js_import_re =
        regex::Regex::new(r#"(?:from\s+['"]([^'"]+)['"]|require\s*\(\s*['"]([^'"]+)['"]\s*\))"#)
            .unwrap();
    let rust_import_re =
        regex::Regex::new(r#"(?:use\s+(?:crate|super)::([\w]+)|mod\s+([\w]+)\s*;)"#).unwrap();
    let go_import_re = regex::Regex::new(r#"import\s+(?:\(\s*)?(?:"([^"]+)")"#).unwrap();
    let cs_using_re = regex::Regex::new(r#"(?m)^using\s+(?:static\s+)?([\w.]+)\s*;"#).unwrap();
    // PowerShell: dot-source (. .\file.ps1) and Import-Module
    let ps_dotsource_re = regex::Regex::new(r#"(?m)\.\s+['".]?\.?[\\/]?([^\s'"]+\.ps[md]?1)"#).unwrap();
    let ps_import_re = regex::Regex::new(r#"(?mi)Import-Module\s+['".]?\.?[\\/]?([^\s'";\)]+)"#).unwrap();
    let cs_namespace_re =
        regex::Regex::new(r#"(?m)^(?:namespace\s+([\w.]+))"#).unwrap();

    // Build a lookup: filename (without ext) → Vec<rel_path> for resolving imports
    let mut filename_to_paths: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut filename_ext_to_paths: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in all_files {
        let full_filename = f.rel_path.rsplit('/').next().unwrap_or(&f.rel_path);
        filename_ext_to_paths
            .entry(full_filename.to_string())
            .or_default()
            .push(f.rel_path.clone());

        let stem = full_filename.rsplit_once('.').map(|(s, _)| s).unwrap_or(full_filename);
        filename_to_paths
            .entry(stem.to_string())
            .or_default()
            .push(f.rel_path.clone());
    }

    // Build namespace → files index for C# resolution
    let namespace_to_files: BTreeMap<String, Vec<String>> = {
        let mut ns_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let cs_files: Vec<&ScannedFile> = all_files
            .iter()
            .filter(|f| cs_exts.contains(f.ext.as_str()))
            .collect();
        let ns_pairs: Vec<(String, String)> = cs_files
            .par_iter()
            .filter_map(|f| {
                let content = fs::read_to_string(&f.abs_path).ok()?;
                let ns = cs_namespace_re
                    .captures(&content)
                    .and_then(|cap| cap.get(1))
                    .map(|m| m.as_str().to_string())?;
                Some((ns, f.rel_path.clone()))
            })
            .collect();
        for (ns, path) in ns_pairs {
            ns_map.entry(ns).or_default().push(path);
        }
        ns_map
    };

    // Resolve an import string to a file path
    let resolve_import = |import_str: &str| -> Option<String> {
        // Try exact filename match first (for C/C++ includes)
        let filename = import_str.rsplit('/').next().unwrap_or(import_str);
        if let Some(candidates) = filename_ext_to_paths.get(filename) {
            if candidates.len() == 1 {
                return Some(candidates[0].clone());
            }
            // Multiple files with same name — pick the one whose path ends with the import
            let best = candidates
                .iter()
                .find(|c| c.ends_with(import_str))
                .or_else(|| candidates.first());
            if let Some(b) = best {
                return Some(b.clone());
            }
        }

        // Try matching the last component of a dotted/slashed path to filename stems
        let last_component = import_str
            .rsplit(&['.', '/'][..])
            .next()
            .unwrap_or(import_str);
        if let Some(candidates) = filename_to_paths.get(last_component) {
            if candidates.len() == 1 {
                return Some(candidates[0].clone());
            }
            return candidates.first().cloned();
        }

        None
    };

    // Parse imports in parallel
    let pairs: Vec<(String, Vec<String>)> = all_files
        .par_iter()
        .filter_map(|f| {
            let ext = f.ext.as_str();
            let has_patterns = cpp_exts.contains(ext)
                || py_exts.contains(ext)
                || js_exts.contains(ext)
                || rust_exts.contains(ext)
                || go_exts.contains(ext)
                || cs_exts.contains(ext)
                || ps_exts.contains(ext);
            if !has_patterns {
                return None;
            }

            let content = fs::read_to_string(&f.abs_path).ok()?;
            let mut resolved = Vec::new();

            if cpp_exts.contains(ext) {
                for cap in include_re.captures_iter(&content) {
                    if let Some(path) = resolve_import(&cap[1]) {
                        resolved.push(path);
                    }
                }
            }

            if py_exts.contains(ext) {
                for cap in py_import_re.captures_iter(&content) {
                    let import_str = cap
                        .get(1)
                        .or_else(|| cap.get(2))
                        .map(|m| m.as_str())
                        .unwrap_or("");
                    if !import_str.is_empty() {
                        if let Some(path) = resolve_import(import_str) {
                            resolved.push(path);
                        }
                    }
                }
            }

            if js_exts.contains(ext) {
                for cap in js_import_re.captures_iter(&content) {
                    let import_str = cap
                        .get(1)
                        .or_else(|| cap.get(2))
                        .map(|m| m.as_str())
                        .unwrap_or("");
                    if !import_str.is_empty() && !import_str.starts_with('.') {
                        // Skip relative imports for now, they need path resolution
                        if let Some(path) = resolve_import(import_str) {
                            resolved.push(path);
                        }
                    } else if !import_str.is_empty() {
                        // Relative import — try resolving the last component
                        if let Some(path) = resolve_import(import_str) {
                            resolved.push(path);
                        }
                    }
                }
            }

            if rust_exts.contains(ext) {
                for cap in rust_import_re.captures_iter(&content) {
                    let import_str = cap
                        .get(1)
                        .or_else(|| cap.get(2))
                        .map(|m| m.as_str())
                        .unwrap_or("");
                    if !import_str.is_empty() {
                        if let Some(path) = resolve_import(import_str) {
                            resolved.push(path);
                        }
                    }
                }
            }

            if go_exts.contains(ext) {
                for cap in go_import_re.captures_iter(&content) {
                    if let Some(m) = cap.get(1) {
                        if let Some(path) = resolve_import(m.as_str()) {
                            resolved.push(path);
                        }
                    }
                }
            }

            if cs_exts.contains(ext) {
                for cap in cs_using_re.captures_iter(&content) {
                    let ns = &cap[1];
                    // Skip System/Microsoft framework namespaces
                    if ns.starts_with("System") || ns.starts_with("Microsoft") {
                        continue;
                    }
                    // Try exact namespace match first
                    if let Some(files) = namespace_to_files.get(ns) {
                        for file in files {
                            if file != &f.rel_path {
                                resolved.push(file.clone());
                            }
                        }
                        continue;
                    }
                    // Try prefix match: using Foo.Bar matches namespace Foo.Bar.* files
                    let prefix = format!("{}.", ns);
                    for (full_ns, files) in namespace_to_files.iter() {
                        if full_ns.starts_with(&prefix) || full_ns == ns {
                            for file in files {
                                if file != &f.rel_path {
                                    resolved.push(file.clone());
                                }
                            }
                        }
                    }
                    // Fallback: resolve by last component (filename-based)
                    if let Some(path) = resolve_import(ns) {
                        if path != f.rel_path {
                            resolved.push(path);
                        }
                    }
                }
            }

            if ps_exts.contains(ext) {
                // Dot-source: . .\helpers.ps1, . "$PSScriptRoot\utils.ps1"
                for cap in ps_dotsource_re.captures_iter(&content) {
                    if let Some(path) = resolve_import(&cap[1]) {
                        resolved.push(path);
                    }
                }
                // Import-Module .\MyModule or Import-Module MyModule
                for cap in ps_import_re.captures_iter(&content) {
                    if let Some(path) = resolve_import(&cap[1]) {
                        resolved.push(path);
                    }
                }
            }

            if resolved.is_empty() {
                None
            } else {
                resolved.sort();
                resolved.dedup();
                Some((f.rel_path.clone(), resolved))
            }
        })
        .collect();

    // Build bidirectional graph
    let mut imports: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut imported_by: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (file, deps) in pairs {
        for dep in &deps {
            imported_by
                .entry(dep.clone())
                .or_default()
                .push(file.clone());
        }
        imports.insert(file, deps);
    }

    // Sort imported_by lists for consistent output
    for list in imported_by.values_mut() {
        list.sort();
    }

    ImportGraph {
        imports,
        imported_by,
    }
}

// ---------------------------------------------------------------------------
// Cross-repo import resolution
// ---------------------------------------------------------------------------

/// Resolve imports that cross repository boundaries.
/// For each repo, unresolved filenames are matched against other repos' files.
pub fn resolve_cross_repo_imports(
    repos: &std::collections::BTreeMap<String, crate::types::RepoState>,
) -> Vec<crate::types::CrossRepoEdge> {
    if repos.len() < 2 {
        return Vec::new();
    }

    // Build per-repo filename lookup: stem -> (repo_name, rel_path)
    let mut global_lookup: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for repo in repos.values() {
        for f in &repo.all_files {
            let full_filename = f.rel_path.rsplit('/').next().unwrap_or(&f.rel_path);
            let stem = full_filename
                .rsplit_once('.')
                .map(|(s, _)| s)
                .unwrap_or(full_filename);
            global_lookup
                .entry(stem.to_string())
                .or_default()
                .push((repo.name.clone(), f.rel_path.clone()));
        }
    }

    let mut edges = Vec::new();

    for repo in repos.values() {
        // Find imports that didn't resolve within the repo
        // (files referenced in import directives but not in the repo's own import graph)
        let local_files: HashSet<&str> = repo.all_files.iter().map(|f| f.rel_path.as_str()).collect();

        for (file, imported_files) in &repo.import_graph.imports {
            for imported in imported_files {
                // Skip if it resolved within the repo
                if local_files.contains(imported.as_str()) {
                    continue;
                }
                // Try to find in other repos by filename stem
                let stem = imported
                    .rsplit('/')
                    .next()
                    .unwrap_or(imported)
                    .rsplit_once('.')
                    .map(|(s, _)| s)
                    .unwrap_or(imported);
                if let Some(candidates) = global_lookup.get(stem) {
                    for (target_repo, target_path) in candidates {
                        if target_repo != &repo.name {
                            edges.push(crate::types::CrossRepoEdge {
                                from_repo: repo.name.clone(),
                                from_file: file.clone(),
                                to_repo: target_repo.clone(),
                                to_file: target_path.clone(),
                            });
                        }
                    }
                }
            }
        }
    }

    if !edges.is_empty() {
        eprintln!("  Cross-repo: {} import edges resolved", edges.len());
    }

    edges
}
