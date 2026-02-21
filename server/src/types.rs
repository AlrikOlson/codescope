//! Core types shared across the CodeScope server: scan configuration, file entries,
//! search indices, import graphs, stub caches, per-repo state, server state,
//! MCP transport types, and path validation utilities.

use dashmap::DashMap;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Session state (per MCP connection, tracks what the agent has already read)
// ---------------------------------------------------------------------------

/// Tracks files read during an MCP session to avoid wasting tokens re-reading,
/// plus exploration frontier and search history for smarter result ranking.
pub struct SessionState {
    pub files_read: HashMap<String, Instant>,
    pub total_tokens_served: usize,
    pub started_at: Instant,
    /// Recent search queries (capped at 50).
    pub search_queries: Vec<String>,
    /// Adjacent unread files — "frontier" of exploration.
    pub exploration_frontier: HashSet<String>,
}

/// Maximum number of recent search queries to track.
const MAX_SEARCH_QUERIES: usize = 50;

impl Default for SessionState {
    fn default() -> Self {
        Self {
            files_read: HashMap::new(),
            total_tokens_served: 0,
            started_at: Instant::now(),
            search_queries: Vec::new(),
            exploration_frontier: HashSet::new(),
        }
    }
}

impl SessionState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_read(&mut self, path: &str, tokens: usize) {
        self.files_read.insert(path.to_string(), Instant::now());
        self.total_tokens_served += tokens;
        // Remove from frontier since it's now been read
        self.exploration_frontier.remove(path);
    }

    pub fn seen_paths(&self) -> HashSet<String> {
        self.files_read.keys().cloned().collect()
    }

    /// Record a search query for session history.
    pub fn record_query(&mut self, query: &str) {
        if self.search_queries.len() >= MAX_SEARCH_QUERIES {
            self.search_queries.remove(0);
        }
        self.search_queries.push(query.to_string());
    }

    /// Update exploration frontier: add import neighbors of a read file,
    /// excluding files already read.
    pub fn update_frontier(&mut self, read_path: &str, neighbors: &[String]) {
        for neighbor in neighbors {
            if !self.files_read.contains_key(neighbor) {
                self.exploration_frontier.insert(neighbor.clone());
            }
        }
        self.exploration_frontier.remove(read_path);
    }

    /// Check if a path is on the exploration frontier.
    pub fn is_on_frontier(&self, path: &str) -> bool {
        self.exploration_frontier.contains(path)
    }
}

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
    /// Embedding model name for semantic search (e.g. "minilm", "codebert", or a HuggingFace ID).
    #[cfg(feature = "semantic")]
    pub semantic_model: Option<String>,
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
            #[cfg(feature = "semantic")]
            semantic_model: None,
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
pub struct SemanticProgress {
    pub status: std::sync::atomic::AtomicU8, // 0=idle, 1=extracting, 2=embedding, 3=ready, 4=failed
    pub total_chunks: std::sync::atomic::AtomicUsize,
    pub total_batches: std::sync::atomic::AtomicUsize,
    pub completed_batches: std::sync::atomic::AtomicUsize,
    pub device: std::sync::RwLock<String>,
}

#[cfg(feature = "semantic")]
impl Default for SemanticProgress {
    fn default() -> Self {
        Self {
            status: std::sync::atomic::AtomicU8::new(0),
            total_chunks: std::sync::atomic::AtomicUsize::new(0),
            total_batches: std::sync::atomic::AtomicUsize::new(0),
            completed_batches: std::sync::atomic::AtomicUsize::new(0),
            device: std::sync::RwLock::new(String::new()),
        }
    }
}

#[cfg(feature = "semantic")]
impl SemanticProgress {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn status_label(&self) -> &'static str {
        match self.status.load(std::sync::atomic::Ordering::Relaxed) {
            0 => "idle",
            1 => "extracting chunks",
            2 => "embedding",
            3 => "ready",
            4 => "failed",
            _ => "unknown",
        }
    }
}

#[cfg(feature = "semantic")]
pub struct SemanticIndex {
    /// Flat embedding storage: `n_chunks * dim` floats for SIMD-friendly access.
    pub embeddings: Vec<f32>,
    /// Metadata for each chunk (parallel to embeddings).
    pub chunk_meta: Vec<ChunkMeta>,
    /// Embedding dimensionality.
    pub dim: usize,
    /// Model name used for indexing (needed to load the same model at search time).
    pub model_name: String,
}

#[cfg(feature = "semantic")]
#[derive(Clone)]
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
#[derive(Default)]
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
    #[cfg(feature = "treesitter")]
    pub ast_index: std::sync::Arc<std::sync::RwLock<crate::ast::AstIndex>>,
    #[cfg(feature = "treesitter")]
    pub code_graph: std::sync::Arc<std::sync::RwLock<crate::graph::CodeGraph>>,
    #[cfg(feature = "semantic")]
    pub semantic_index: std::sync::Arc<std::sync::RwLock<Option<SemanticIndex>>>,
    #[cfg(feature = "semantic")]
    pub semantic_progress: std::sync::Arc<SemanticProgress>,
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
    #[cfg(feature = "semantic")]
    pub semantic_enabled: bool,
    #[cfg(feature = "semantic")]
    pub semantic_model: Option<String>,
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
// MCP transport types (session management, OAuth config)
// ---------------------------------------------------------------------------

/// Configuration for MCP HTTP transport and OAuth discovery.
pub struct McpConfig {
    /// Allowed Origin header values for DNS rebinding protection.
    pub allowed_origins: Vec<String>,
    /// OAuth authorization server URL. None = auth disabled.
    pub auth_issuer: Option<String>,
    /// The base URL of this server (for PRM `resource` field).
    pub server_url: String,
}

impl McpConfig {
    #[allow(dead_code)]
    pub fn auth_enabled(&self) -> bool {
        self.auth_issuer.is_some()
    }
}

/// State for a single MCP HTTP session.
pub struct McpSession {
    pub protocol_version: String,
    pub session_state: SessionState,
    pub last_activity: Instant,
}

impl McpSession {
    pub fn new(protocol_version: String) -> Self {
        Self { protocol_version, session_state: SessionState::new(), last_activity: Instant::now() }
    }
}

/// Thread-safe session store. Key = session ID (UUID string).
pub type SessionStore = DashMap<String, McpSession>;

/// Axum state for MCP HTTP transport routes.
#[derive(Clone)]
pub struct McpAppContext {
    pub state: Arc<std::sync::RwLock<ServerState>>,
    pub sessions: Arc<SessionStore>,
    pub config: Arc<McpConfig>,
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
    /// Server start time for uptime reporting via `/health`.
    pub start_time: std::time::Instant,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn validate_path_rejects_traversal() {
        let root = Path::new("/tmp");
        let result = validate_path(root, "../etc/passwd");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Invalid path");
    }

    #[test]
    fn validate_path_rejects_absolute_paths() {
        let root = Path::new("/tmp");
        let result = validate_path(root, "/etc/passwd");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Invalid path");
    }

    #[test]
    fn validate_path_rejects_empty() {
        let root = Path::new("/tmp");
        let result = validate_path(root, "");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Invalid path");
    }

    #[test]
    fn validate_path_accepts_valid_relative() {
        // Use a path that actually exists on the filesystem
        let root = Path::new("/tmp");
        // Create a temp file so canonicalize succeeds
        let test_file = root.join("codescope_test_validate.txt");
        std::fs::write(&test_file, "test").ok();
        let result = validate_path(root, "codescope_test_validate.txt");
        assert!(result.is_ok(), "valid relative path should succeed: {:?}", result);
        std::fs::remove_file(&test_file).ok();
    }

    #[test]
    fn grep_relevance_score_more_matches_higher() {
        let terms = vec!["foo".to_string()];
        let idf = vec![1.0];

        let score_low = grep_relevance_score(1, 100, "bar.rs", "rs", &terms, 1, 50, &idf);
        let score_high = grep_relevance_score(10, 100, "bar.rs", "rs", &terms, 1, 50, &idf);

        assert!(
            score_high > score_low,
            "10 matches ({score_high}) should score higher than 1 match ({score_low})"
        );
    }
}
