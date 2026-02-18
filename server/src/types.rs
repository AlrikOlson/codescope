use dashmap::DashMap;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const MAX_FILE_READ: usize = 512 * 1024;

// ---------------------------------------------------------------------------
// Scan configuration — replaces hardcoded constants
// ---------------------------------------------------------------------------

/// Runtime configuration for scanning. Loaded from .codescope.toml or defaults.
#[derive(Clone)]
pub struct ScanConfig {
    pub root: PathBuf,
    /// Directories to scan (relative to root). Empty = scan root itself.
    pub scan_dirs: Vec<String>,
    /// Directory names to skip during walk.
    pub skip_dirs: HashSet<String>,
    /// File extensions to include. Empty = all text files.
    pub extensions: HashSet<String>,
    /// Directory names to collapse/strip from category paths.
    pub noise_dirs: HashSet<String>,
}

impl ScanConfig {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            scan_dirs: Vec::new(),
            extensions: HashSet::new(),
            skip_dirs: [
                ".git",
                "node_modules",
                "__pycache__",
                "target",
                "dist",
                "build",
                ".next",
                "vendor",
                "Intermediate",
                "Saved",
                "Binaries",
                "DerivedDataCache",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            noise_dirs: [
                "Private", "Public", "Classes", "Internal", "Inc", "Source", "Src", "Include",
                "src", "lib",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        }
    }
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self::new(PathBuf::from("."))
    }
}

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize)]
pub struct FileEntry {
    pub path: String,
    pub desc: String,
    pub size: u64,
}

#[derive(Clone, Serialize, Default)]
pub struct DepEntry {
    pub public: Vec<String>,
    pub private: Vec<String>,
    #[serde(rename = "categoryPath")]
    pub category_path: String,
}

#[derive(Clone)]
pub struct ScannedFile {
    pub rel_path: String,
    pub abs_path: PathBuf,
    pub desc: String,
    pub ext: String,
}

// ---------------------------------------------------------------------------
// Search index types
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize)]
pub struct SearchFileEntry {
    pub path: String,
    pub path_lower: String,
    pub filename: String,
    pub filename_lower: String,
    pub dir: String,
    pub ext: String,
    pub desc: String,
    pub desc_lower: String,
    pub category: String,
    #[serde(skip)]
    pub filename_mask: u64,
    #[serde(skip)]
    pub path_mask: u64,
    #[serde(skip)]
    pub desc_mask: u64,
}

#[derive(Clone, Serialize)]
pub struct SearchModuleEntry {
    pub id: String,
    pub id_lower: String,
    pub name: String,
    pub name_lower: String,
    pub file_count: usize,
    #[serde(skip)]
    pub name_mask: u64,
    #[serde(skip)]
    pub id_mask: u64,
}

// ---------------------------------------------------------------------------
// Import graph (built at startup from import/include directives)
// ---------------------------------------------------------------------------

pub struct ImportGraph {
    /// file -> files it imports (resolved to rel_paths)
    pub imports: BTreeMap<String, Vec<String>>,
    /// file -> files that import it
    pub imported_by: BTreeMap<String, Vec<String>>,
}

// ---------------------------------------------------------------------------
// Stub cache (lazy, populated on first context request per file)
// ---------------------------------------------------------------------------

/// Cached stub data for a single file. Shared via Arc to avoid clones.
pub struct CachedStub {
    pub raw: Arc<str>,
    pub tier1: Arc<str>,
    pub fast_tokens: usize,
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub struct AppState {
    pub project_root: PathBuf,
    pub tree_json: String,
    pub manifest_json: String,
    pub deps_json: String,
    pub deps: BTreeMap<String, DepEntry>,
    pub all_files: Vec<ScannedFile>,
    pub search_files: Vec<SearchFileEntry>,
    pub search_modules: Vec<SearchModuleEntry>,
    pub import_graph: ImportGraph,
    /// Lazy cache: rel_path -> (raw content, tier1 stubs, fast token estimate)
    pub stub_cache: DashMap<String, CachedStub>,
    pub tokenizer: Arc<dyn crate::tokenizer::Tokenizer>,
}

pub struct McpState {
    pub project_root: PathBuf,
    pub all_files: Vec<ScannedFile>,
    pub manifest: BTreeMap<String, Vec<FileEntry>>,
    pub deps: BTreeMap<String, DepEntry>,
    pub search_files: Vec<SearchFileEntry>,
    pub search_modules: Vec<SearchModuleEntry>,
    pub import_graph: ImportGraph,
    pub stub_cache: DashMap<String, CachedStub>,
    pub tokenizer: Arc<dyn crate::tokenizer::Tokenizer>,
}

// ---------------------------------------------------------------------------
// Path validation
// ---------------------------------------------------------------------------

pub fn validate_path(project_root: &Path, rel_path: &str) -> Result<PathBuf, &'static str> {
    if rel_path.is_empty() || rel_path.contains("..") || rel_path.starts_with('/') {
        return Err("Invalid path");
    }
    let full = project_root.join(rel_path);
    let canonical = full.canonicalize().map_err(|_| "File not found")?;
    let root_canonical = project_root.canonicalize().map_err(|_| "Root not found")?;
    if !canonical.starts_with(&root_canonical) {
        return Err("Path traversal detected");
    }
    Ok(canonical)
}
