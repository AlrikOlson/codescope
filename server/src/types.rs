use dashmap::DashMap;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum file size (in bytes) that will be read into memory.
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
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            noise_dirs: ["Private", "Public", "Internal", "Source", "Src", "Include", "src", "lib"]
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

/// A single file within a category, carrying its path, description, and size.
#[derive(Clone, Serialize)]
pub struct FileEntry {
    pub path: String,
    pub desc: String,
    pub size: u64,
}

/// Dependency entry for a module, split into public and private (dev) dependencies.
#[derive(Clone, Serialize, Default)]
pub struct DepEntry {
    pub public: Vec<String>,
    pub private: Vec<String>,
    #[serde(rename = "categoryPath")]
    pub category_path: String,
}

/// Metadata for a file discovered during the directory scan.
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

/// Pre-computed search index entry for a file, with lowercased fields and bitmasks for fast fuzzy matching.
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

/// Pre-computed search index entry for a module (category), with bitmasks for fast fuzzy matching.
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

/// Bidirectional import/include graph mapping files to their dependencies and dependents.
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

/// Per-term document frequency index for IDF-weighted search scoring.
pub struct TermDocFreq {
    pub total_docs: usize,
    pub freq: HashMap<String, usize>,
}

impl TermDocFreq {
    pub fn new() -> Self {
        Self { total_docs: 0, freq: HashMap::new() }
    }

    /// IDF with Laplace smoothing: ln((N+1)/(df+1)) + 1.
    /// Unknown terms default to df=total_docs (IDF ~1.0).
    pub fn idf(&self, term: &str) -> f64 {
        let df = self.freq.get(term).copied().unwrap_or(self.total_docs);
        (((self.total_docs as f64 + 1.0) / (df as f64 + 1.0)).ln() + 1.0).max(1.0)
    }
}

/// Complete indexed state for a single repository, including files, deps, search index, and caches.
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
    pub term_doc_freq: TermDocFreq,
    pub scan_time_ms: u64,
    #[cfg(feature = "semantic")]
    pub semantic_index: std::sync::Arc<std::sync::RwLock<Option<SemanticIndex>>>,
}

// ---------------------------------------------------------------------------
// Cross-repo import edges
// ---------------------------------------------------------------------------

/// An import edge that crosses repository boundaries (file in repo A imports file in repo B).
pub struct CrossRepoEdge {
    pub from_repo: String,
    pub from_file: String,
    pub to_repo: String,
    pub to_file: String,
}

// ---------------------------------------------------------------------------
// Server state (unified — used by both MCP and HTTP modes)
// ---------------------------------------------------------------------------

/// Unified server state holding all indexed repos, shared by both MCP and HTTP modes.
pub struct ServerState {
    pub repos: BTreeMap<String, RepoState>,
    pub default_repo: Option<String>,
    pub cross_repo_edges: Vec<CrossRepoEdge>,
    pub tokenizer: Arc<dyn crate::tokenizer::Tokenizer>,
}

impl ServerState {
    /// Returns the default repo (single-repo mode) or the first repo.
    ///
    /// # Panics
    /// Panics if no repositories have been indexed.
    pub fn default_repo(&self) -> &RepoState {
        if let Some(ref name) = self.default_repo {
            &self.repos[name]
        } else {
            self.repos.values().next().expect("ServerState must have at least one repo")
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP-specific types (pre-computed JSON cache + Axum state)
// ---------------------------------------------------------------------------

/// Pre-serialized JSON responses for the HTTP API, computed once at startup.
pub struct HttpCache {
    pub tree_json: String,
    pub manifest_json: String,
    pub deps_json: String,
}

/// Axum application state combining the shared server state with the HTTP JSON cache.
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

/// BM25-lite relevance score for grep results with IDF weighting.
/// Shared by HTTP API and MCP grep/find handlers.
#[allow(clippy::too_many_arguments)]
pub fn grep_relevance_score(
    match_count: usize,
    total_lines: usize,
    filename_lower: &str,
    ext: &str,
    terms_lower: &[String],
    terms_matched: usize,
    first_match_line: usize,
    idf_weights: &[f64],
) -> f64 {
    let tf = match_count as f64 / (match_count as f64 + 1.5);

    // Average IDF across query terms — rare terms score higher
    let avg_idf = if idf_weights.is_empty() {
        1.0
    } else {
        idf_weights.iter().sum::<f64>() / idf_weights.len() as f64
    };

    // Density: sqrt-normalized to reduce large-file penalty
    let density = match_count as f64 / (total_lines as f64).sqrt().max(1.0);

    // Filename bonus: reduced from 50 to 15 so content can compete
    let filename_bonus =
        if terms_lower.iter().any(|t| filename_lower.contains(t.as_str())) { 15.0 } else { 0.0 };

    let def_bonus = if is_definition_file(ext) { 5.0 } else { 0.0 };

    // Position bonus: matches in first 30 lines (declarations) score higher
    let position_bonus = if total_lines > 30 && first_match_line < 30 {
        3.0 * (1.0 - first_match_line as f64 / 30.0)
    } else {
        0.0
    };

    let base = tf * 15.0 * avg_idf + filename_bonus + def_bonus + density + position_bonus;

    // IDF-weighted coverage: missing a rare term is a massive penalty.
    // For single-term queries, coverage is trivially 1.0 (no penalty).
    let term_count = terms_lower.len();
    if term_count <= 1 || idf_weights.is_empty() {
        return base;
    }

    // Assume matched terms are the lowest-IDF (most common) ones.
    // This penalizes missing rare terms hardest.
    let mut sorted_idfs: Vec<f64> = idf_weights.to_vec();
    sorted_idfs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let matched_idf_sum: f64 = sorted_idfs.iter().take(terms_matched).sum();
    let total_idf_sum: f64 = sorted_idfs.iter().sum();

    let coverage = if total_idf_sum > 0.0 { matched_idf_sum / total_idf_sum } else { 1.0 };
    let coverage_factor = coverage * coverage;

    // Floor of 0.3 keeps partial matches visible but far below full matches
    base * (0.3 + 0.7 * coverage_factor)
}

// ---------------------------------------------------------------------------
// Path validation
// ---------------------------------------------------------------------------

/// Validate and canonicalize a relative path, rejecting traversal attacks and paths outside the root.
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
