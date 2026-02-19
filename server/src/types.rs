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
// Semantic search types (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "semantic")]
pub struct SemanticIndex {
    /// Flat embedding storage: `n_chunks * dim` floats for SIMD-friendly access.
    pub embeddings: Vec<f32>,
    /// Metadata for each chunk (parallel to embeddings).
    pub chunk_meta: Vec<ChunkMeta>,
    /// Embedding dimensionality (384 for all-MiniLM-L6-v2).
    pub dim: usize,
}

#[cfg(feature = "semantic")]
pub struct ChunkMeta {
    pub file_path: String,
    pub start_line: usize,
    /// First 200 chars of the chunk for display.
    pub snippet: String,
}

// ---------------------------------------------------------------------------
// Per-repo state (one instance per indexed repository)
// ---------------------------------------------------------------------------

pub struct RepoState {
    pub name: String,
    pub root: PathBuf,
    pub config: ScanConfig,
    pub all_files: Vec<ScannedFile>,
    pub manifest: BTreeMap<String, Vec<FileEntry>>,
    pub deps: BTreeMap<String, DepEntry>,
    pub search_files: Vec<SearchFileEntry>,
    pub search_modules: Vec<SearchModuleEntry>,
    pub import_graph: ImportGraph,
    pub stub_cache: DashMap<String, CachedStub>,
    pub scan_time_ms: u64,
    #[cfg(feature = "semantic")]
    pub semantic_index: Option<SemanticIndex>,
}

// ---------------------------------------------------------------------------
// Cross-repo import edges
// ---------------------------------------------------------------------------

pub struct CrossRepoEdge {
    pub from_repo: String,
    pub from_file: String,
    pub to_repo: String,
    pub to_file: String,
}

// ---------------------------------------------------------------------------
// Server state (unified — used by both MCP and HTTP modes)
// ---------------------------------------------------------------------------

pub struct ServerState {
    pub repos: BTreeMap<String, RepoState>,
    pub default_repo: Option<String>,
    pub cross_repo_edges: Vec<CrossRepoEdge>,
    pub tokenizer: Arc<dyn crate::tokenizer::Tokenizer>,
}

impl ServerState {
    /// Returns the default repo (single-repo mode) or the first repo.
    pub fn default_repo(&self) -> &RepoState {
        if let Some(ref name) = self.default_repo {
            &self.repos[name]
        } else {
            self.repos.values().next().expect("no repos in state")
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP-specific types (pre-computed JSON cache + Axum state)
// ---------------------------------------------------------------------------

pub struct HttpCache {
    pub tree_json: String,
    pub manifest_json: String,
    pub deps_json: String,
}

#[derive(Clone)]
pub struct AppContext {
    pub state: Arc<std::sync::RwLock<ServerState>>,
    pub cache: Arc<HttpCache>,
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Check if a file extension indicates a definition/header file.
pub fn is_definition_file(ext: &str) -> bool {
    matches!(ext, "h" | "hpp" | "hxx" | "d.ts" | "pyi")
}

/// BM25-lite relevance score for grep results.
/// Shared by HTTP API and MCP grep/find handlers.
pub fn grep_relevance_score(
    match_count: usize,
    total_lines: usize,
    filename_lower: &str,
    ext: &str,
    terms_lower: &[String],
) -> f64 {
    let tf = match_count as f64 / (match_count as f64 + 1.5);
    let filename_bonus = if terms_lower.iter().any(|t| filename_lower.contains(t.as_str())) {
        50.0
    } else {
        0.0
    };
    let def_bonus = if is_definition_file(ext) { 5.0 } else { 0.0 };
    let density = match_count as f64 / total_lines.max(1) as f64 * 10.0;
    tf * 20.0 + filename_bonus + def_bonus + density
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
