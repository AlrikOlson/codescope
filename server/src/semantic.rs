//! Semantic code search using sentence embeddings (nomic-embed-text-v1.5 by default).
//!
//! Chunks source files by logical boundaries, generates embeddings via fastembed
//! (ONNX Runtime), and ranks results by cosine similarity. Supports GPU acceleration
//! via CUDA, CoreML, DirectML, Metal, and Accelerate execution providers.
//! Persistent caching avoids re-embedding unchanged files.

use crate::stubs::extract_stubs;
use crate::types::{ChunkMeta, ScannedFile, SemanticIndex};

#[cfg(feature = "treesitter")]
use crate::ast::{AstIndex, FileAst};

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

use std::collections::HashMap;
use std::io::{Read as IoRead, Write as IoWrite};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Model tiers — built-in presets with auto-selection
// ---------------------------------------------------------------------------

/// Configuration for an embedding model tier.
pub struct TierConfig {
    pub fastembed_model: EmbeddingModel,
    pub dim: usize,
    pub max_chunk_chars: usize,
    /// Prefix prepended to queries during search (e.g. "search_query: ").
    pub query_prefix: Option<&'static str>,
    /// Prefix prepended to documents during embedding (e.g. "search_document: ").
    pub document_prefix: Option<&'static str>,
    /// Name stored in cache file for validation.
    pub cache_name: &'static str,
}

/// Resolve a model name to its tier configuration.
///
/// Accepts tier names ("standard", "lightweight", "quality", "code"),
/// legacy names ("modernbert" → standard, "minilm" → lightweight),
/// or defaults to "standard" (nomic-embed-text-v1.5 quantized) for `None`/`"auto"`.
pub fn resolve_model(name: Option<&str>) -> TierConfig {
    match name {
        None | Some("auto") | Some("standard") | Some("nomic") | Some("modernbert") => {
            TierConfig {
                fastembed_model: EmbeddingModel::NomicEmbedTextV15Q,
                dim: 768,
                max_chunk_chars: 2000,
                query_prefix: Some("search_query: "),
                document_prefix: Some("search_document: "),
                cache_name: "standard",
            }
        }
        Some("lightweight") | Some("minilm") => TierConfig {
            fastembed_model: EmbeddingModel::AllMiniLML6V2Q,
            dim: 384,
            max_chunk_chars: 1500,
            query_prefix: None,
            document_prefix: None,
            cache_name: "lightweight",
        },
        Some("quality") => TierConfig {
            fastembed_model: EmbeddingModel::NomicEmbedTextV15,
            dim: 768,
            max_chunk_chars: 2000,
            query_prefix: Some("search_query: "),
            document_prefix: Some("search_document: "),
            cache_name: "quality",
        },
        Some("code") | Some("jina-code") => TierConfig {
            fastembed_model: EmbeddingModel::JinaEmbeddingsV2BaseCode,
            dim: 768,
            max_chunk_chars: 2000,
            query_prefix: None,
            document_prefix: None,
            cache_name: "code",
        },
        Some(unknown) => {
            tracing::warn!(model = unknown, "Unknown model tier, falling back to standard");
            TierConfig {
                fastembed_model: EmbeddingModel::NomicEmbedTextV15Q,
                dim: 768,
                max_chunk_chars: 2000,
                query_prefix: Some("search_query: "),
                document_prefix: Some("search_document: "),
                cache_name: "standard",
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Chunk extraction — per-file grouping for cache keying
// ---------------------------------------------------------------------------

/// A code chunk ready for embedding.
struct Chunk {
    start_line: usize,
    text: String,
}

/// All chunks from a single source file, with metadata for cache invalidation.
struct FileChunks {
    rel_path: String,
    file_size: u64,
    mtime_secs: i64,
    chunks: Vec<Chunk>,
}

/// File extensions worth embedding — source code that produces meaningful stubs.
fn is_embeddable_ext(ext: &str) -> bool {
    matches!(
        ext,
        "h" | "hpp"
            | "hxx"
            | "cpp"
            | "cxx"
            | "cc"
            | "c"
            | "cs"
            | "java"
            | "kt"
            | "scala"
            | "rs"
            | "go"
            | "js"
            | "ts"
            | "jsx"
            | "tsx"
            | "mjs"
            | "cjs"
            | "swift"
            | "usf"
            | "ush"
            | "hlsl"
            | "glsl"
            | "vert"
            | "frag"
            | "comp"
            | "wgsl"
            | "py"
            | "rb"
            | "d"
            | "ps1"
            | "psm1"
            | "psd1"
    )
}

/// Directories to skip during semantic indexing.
/// These are still indexed for keyword search (cs_grep, cs_find) but excluded
/// from embedding because they produce noise and bloat the vector index.
const SEMANTIC_SKIP_DIRS: &[&str] = &["ThirdParty", "External", "Intermediate", "Deploy"];

/// Check if a file path should be skipped for semantic indexing.
fn skip_for_semantic(rel_path: &str) -> bool {
    let path_lower = rel_path.to_lowercase();
    SEMANTIC_SKIP_DIRS.iter().any(|d| {
        let needle = format!("/{}/", d.to_lowercase());
        path_lower.contains(&needle) || path_lower.starts_with(&needle[1..])
    })
}

/// Max file size to read for embedding (512 KB). Larger files are typically
/// generated code that produces low-quality chunks.
const MAX_FILE_SIZE: u64 = 512 * 1024;

/// Split stub text into chunks at blank-line boundaries.
fn split_stubs_into_chunks(stubs: &str, max_chunk_chars: usize) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut current_chunk = String::new();
    let mut chunk_start_line = 1usize;
    let mut line_num = 1usize;

    for line in stubs.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() && !current_chunk.trim().is_empty() {
            if current_chunk.len() >= 40 {
                chunks.push(Chunk { start_line: chunk_start_line, text: current_chunk.clone() });
            }
            current_chunk.clear();
            chunk_start_line = line_num + 1;
        } else {
            if current_chunk.len() + line.len() + 1 > max_chunk_chars && !current_chunk.is_empty() {
                chunks.push(Chunk { start_line: chunk_start_line, text: current_chunk.clone() });
                current_chunk.clear();
                chunk_start_line = line_num;
            }
            if !current_chunk.is_empty() {
                current_chunk.push('\n');
            }
            current_chunk.push_str(line);
        }
        line_num += 1;
    }

    if current_chunk.len() >= 40 {
        chunks.push(Chunk { start_line: chunk_start_line, text: current_chunk });
    }

    chunks
}

/// Split source content into chunks at AST symbol boundaries.
///
/// For each top-level symbol (parent_idx == None):
///   1. Include doc comment lines above the symbol
///   2. Extract content from doc_start..symbol.end_line
///   3. If oversized, truncate to signature + first max_chars
///   4. Drop chunks < 40 chars
///
/// Falls back to blank-line chunking if AST produces no usable chunks.
#[cfg(feature = "treesitter")]
fn split_by_ast_boundaries(
    content: &str,
    file_ast: &FileAst,
    max_chunk_chars: usize,
) -> Vec<Chunk> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() || file_ast.symbols.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();

    for sym in &file_ast.symbols {
        // Only chunk top-level symbols (methods are included in their parent's chunk)
        if sym.parent_idx.is_some() {
            continue;
        }

        let sym_start = sym.start_line.saturating_sub(1); // 0-based index
        let sym_end = sym.end_line.min(lines.len()); // 1-based inclusive -> exclusive

        if sym_start >= lines.len() {
            continue;
        }

        // Scan upward for doc comments above the symbol
        let mut doc_start = sym_start;
        while doc_start > 0 {
            let prev = lines[doc_start - 1].trim();
            if prev.starts_with("///")
                || prev.starts_with("//!")
                || prev.starts_with("/**")
                || prev.starts_with("* ")
                || prev.starts_with("*/")
                || prev.starts_with("#[")     // Rust attributes
                || prev.starts_with("@")      // Java/TS decorators
                || prev.starts_with("#")       // Python decorators (# comments too)
                || prev.starts_with("\"\"\"")  // Python docstrings
            {
                doc_start -= 1;
            } else {
                break;
            }
        }

        // Extract the chunk text
        let chunk_lines: Vec<&str> = lines[doc_start..sym_end].to_vec();
        let mut text = chunk_lines.join("\n");

        // Truncate oversized chunks (keep signature + beginning)
        if text.len() > max_chunk_chars {
            // Try to keep at least the signature line
            let mut end = max_chunk_chars;
            while !text.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            text = text[..end].to_string();
            text.push_str("\n// ...");
        }

        if text.len() >= 40 {
            chunks.push(Chunk {
                start_line: doc_start + 1, // 1-based
                text,
            });
        }
    }

    chunks
}

/// Convert a file path into a context line for embedding.
/// Strips noise directories and creates a breadcrumb like:
///   "// File: SaveGame.h (Engine > GameFramework)"
fn path_context(rel_path: &str) -> String {
    let filename = rel_path.rsplit('/').next().unwrap_or(rel_path);

    const NOISE: &[&str] =
        &["Source", "Private", "Public", "Internal", "Include", "src", "lib", "Classes", "Src"];
    let parts: Vec<&str> =
        rel_path.split('/').filter(|p| *p != filename && !NOISE.contains(p)).collect();

    // Take last 3 meaningful components as context breadcrumb
    let context: Vec<&str> = parts.iter().rev().take(3).rev().cloned().collect();

    if context.is_empty() {
        format!("// File: {filename}")
    } else {
        format!("// File: {filename} ({})", context.join(" > "))
    }
}

/// Extract embeddable chunks grouped by file. Parallelized via rayon.
/// Pre-filters by extension and file size. Returns file metadata for cache keying.
///
/// When `ast_index` is provided (treesitter feature), uses AST-boundary chunking
/// for files with parsed ASTs, falling back to blank-line stub chunking otherwise.
fn extract_chunks_by_file(
    files: &[ScannedFile],
    max_chunk_chars: usize,
    #[cfg(feature = "treesitter")] ast_index: Option<&AstIndex>,
) -> Vec<FileChunks> {
    use rayon::prelude::*;

    files
        .par_iter()
        .filter(|file| is_embeddable_ext(&file.ext))
        .filter(|file| !skip_for_semantic(&file.rel_path))
        .filter_map(|file| {
            let meta = std::fs::metadata(&file.abs_path).ok()?;
            if meta.len() > MAX_FILE_SIZE {
                return None;
            }
            let mtime_secs = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            let content = std::fs::read_to_string(&file.abs_path).ok()?;

            // Try AST-boundary chunking first (when treesitter feature is enabled)
            #[cfg(feature = "treesitter")]
            let chunks = {
                let ast_chunks = ast_index
                    .and_then(|idx| idx.get(&file.rel_path))
                    .map(|file_ast| split_by_ast_boundaries(&content, file_ast, max_chunk_chars))
                    .unwrap_or_default();

                if !ast_chunks.is_empty() {
                    ast_chunks
                } else {
                    // Fallback to blank-line stub chunking
                    let stubs = extract_stubs(&content, &file.ext);
                    if stubs.trim().is_empty() {
                        return None;
                    }
                    split_stubs_into_chunks(&stubs, max_chunk_chars)
                }
            };

            #[cfg(not(feature = "treesitter"))]
            let chunks = {
                let stubs = extract_stubs(&content, &file.ext);
                if stubs.trim().is_empty() {
                    return None;
                }
                split_stubs_into_chunks(&stubs, max_chunk_chars)
            };

            if chunks.is_empty() {
                return None;
            }

            // Prepend file path context to each chunk for better embedding relevance
            let header = path_context(&file.rel_path);
            let chunks = chunks
                .into_iter()
                .map(|mut c| {
                    c.text = format!("{header}\n{}", c.text);
                    c
                })
                .collect();

            Some(FileChunks {
                rel_path: file.rel_path.clone(),
                file_size: meta.len(),
                mtime_secs,
                chunks,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Embedding model — fastembed with execution provider auto-detection
// ---------------------------------------------------------------------------

/// Determine the device label from compiled features.
fn device_label() -> &'static str {
    if cfg!(feature = "cuda") {
        "CUDA"
    } else if cfg!(feature = "coreml") {
        "CoreML"
    } else if cfg!(feature = "directml") {
        "DirectML"
    } else if cfg!(feature = "metal") {
        "Metal"
    } else if cfg!(feature = "accelerate") {
        "Accelerate"
    } else {
        "CPU"
    }
}

/// Create a fastembed TextEmbedding model with appropriate execution providers.
/// Registers GPU EPs based on compiled features (CUDA, CoreML, DirectML).
/// ORT falls back to CPU automatically if a GPU EP is unavailable at runtime.
fn create_embedding_model(config: &TierConfig) -> Result<TextEmbedding, String> {
    tracing::info!(
        model = ?config.fastembed_model,
        device = device_label(),
        "Loading embedding model"
    );

    let mut opts = InitOptions::new(config.fastembed_model.clone())
        .with_show_download_progress(true);

    // Register execution providers based on compiled features.
    // ORT tries EPs in order and falls back to CPU for unsupported ops.
    #[allow(unused_mut)]
    let mut eps: Vec<ort::execution_providers::ExecutionProviderDispatch> = Vec::new();

    #[cfg(feature = "coreml")]
    {
        eps.push(
            ort::ep::CoreML::default()
                .with_subgraphs(true)
                .build(),
        );
    }

    #[cfg(feature = "cuda")]
    {
        eps.push(ort::ep::CUDA::default().build());
    }

    #[cfg(feature = "directml")]
    {
        eps.push(ort::ep::DirectML::default().build());
    }

    if !eps.is_empty() {
        opts = opts.with_execution_providers(eps);
    }

    TextEmbedding::try_new(opts)
        .map_err(|e| format!("Failed to create embedding model: {e}"))
}

// ---------------------------------------------------------------------------
// Embedding cache — per-file, progressive, survives interrupts
// ---------------------------------------------------------------------------
//
// Binary format (append-safe):
//   Header:  magic[4] + version[2] + dim[2] + model_name_len[2] + model_name
//   Entry*:  path_len[4] + path + file_size[8] + mtime[8] + n_chunks[4]
//            + per chunk: start_line[4] + snippet_len[2] + snippet + embedding[dim*4]
//
// Later entries for the same path supersede earlier ones (HashMap insert).

const CACHE_MAGIC: &[u8; 4] = b"CSEM";
const CACHE_VERSION: u16 = 5; // bumped: AST-boundary chunking produces different chunks

/// Cached embeddings for one source file.
struct CachedFile {
    file_size: u64,
    mtime_secs: i64,
    chunks: Vec<(ChunkMeta, Vec<f32>)>,
}

/// Derive a stable identity string for a repo, used as the cache directory name.
///
/// For git repos with a remote: normalizes the origin URL into a filesystem-safe string.
/// For git repos without a remote or non-git dirs: sanitizes the canonical path.
fn repo_identity(repo_root: &Path) -> String {
    // Try to get git remote origin URL via git2
    if let Ok(repo) = git2::Repository::open(repo_root) {
        if let Ok(remote) = repo.find_remote("origin") {
            if let Some(url) = remote.url() {
                return normalize_remote_url(url);
            }
        }
    }
    // Fallback: sanitize the canonical path
    let canonical = repo_root.canonicalize().unwrap_or_else(|_| repo_root.to_path_buf());
    sanitize_path_to_identity(&canonical)
}

/// Normalize a git remote URL into a filesystem-safe directory name.
///
/// Examples:
///   https://github.com/User/Repo.git → github.com_user_repo
///   git@github.com:User/Repo.git     → github.com_user_repo
///   ssh://git@host.com/org/repo      → host.com_org_repo
fn normalize_remote_url(url: &str) -> String {
    let mut s = url.to_string();
    // Strip common schemes
    for prefix in &["https://", "http://", "ssh://", "git://"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.to_string();
            break;
        }
    }
    // Strip git@ prefix (SSH shorthand)
    if let Some(rest) = s.strip_prefix("git@") {
        s = rest.to_string();
    }
    // Strip .git suffix
    if let Some(rest) = s.strip_suffix(".git") {
        s = rest.to_string();
    }
    // Replace : with / (git@github.com:user/repo → github.com/user/repo)
    s = s.replace(':', "/");
    // Remove double slashes
    while s.contains("//") {
        s = s.replace("//", "/");
    }
    // Strip leading/trailing slashes
    s = s.trim_matches('/').to_string();
    // Lowercase and replace / with _
    s.to_lowercase().replace('/', "_")
}

/// Sanitize an absolute path into a filesystem-safe identity string.
fn sanitize_path_to_identity(path: &Path) -> String {
    let s = path.to_string_lossy();
    let cleaned = s.replace(['/', '\\'], "_").replace(':', "_");
    let trimmed = cleaned.trim_matches('_');
    trimmed.to_lowercase()
}

/// Resolve the centralized cache path for a repo.
///
/// Returns `~/.cache/codescope/semantic/{identity}/semantic.cache` (or platform equivalent).
/// Falls back to legacy `{repo_root}/.codescope/semantic.cache` if central cache dir unavailable.
fn cache_path(repo_root: &Path) -> PathBuf {
    if let Some(base) = crate::cache_dir() {
        let identity = repo_identity(repo_root);
        base.join("semantic").join(&identity).join("semantic.cache")
    } else {
        // Fallback to legacy in-repo location
        repo_root.join(".codescope").join("semantic.cache")
    }
}

/// Legacy in-repo cache path for migration.
fn legacy_cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".codescope").join("semantic.cache")
}

/// Info about a repo's semantic cache state.
pub struct CacheInfo {
    pub model: String,
    pub chunks: usize,
    /// Whether the cache version matches the current binary (false = needs rebuild)
    pub current: bool,
}

/// Check if a repo has an existing semantic cache.
/// Returns `Some(CacheInfo)` if meta.json exists with chunks > 0, `None` otherwise.
/// `current` is true only if the stored cache_version matches `CACHE_VERSION`.
pub fn check_semantic_cache(repo_root: &Path) -> Option<CacheInfo> {
    let cp = cache_path(repo_root);
    let meta_path = cp.parent()?.join("meta.json");
    let content = std::fs::read_to_string(&meta_path).ok()?;
    let meta: serde_json::Value = serde_json::from_str(&content).ok()?;
    let model = meta["model"].as_str().unwrap_or("unknown").to_string();
    let chunks = meta["chunks"].as_u64().unwrap_or(0) as usize;
    if chunks == 0 {
        return None;
    }
    let stored_version = meta["cache_version"].as_u64().unwrap_or(0) as u16;
    Some(CacheInfo {
        model,
        chunks,
        current: stored_version == CACHE_VERSION,
    })
}

/// Write a meta.json alongside the cache for debugging/discovery.
fn write_cache_meta(repo_root: &Path, model: &str, chunks: usize) {
    let cp = cache_path(repo_root);
    if let Some(dir) = cp.parent() {
        let meta_path = dir.join("meta.json");
        let remote = git2::Repository::open(repo_root)
            .ok()
            .and_then(|r| r.find_remote("origin").ok().and_then(|rem| rem.url().map(String::from)));
        let meta = serde_json::json!({
            "remote_url": remote,
            "last_path": repo_root.to_string_lossy(),
            "model": model,
            "chunks": chunks,
            "cache_version": CACHE_VERSION,
            "built": chrono_now_iso(),
        });
        let _ = std::fs::write(&meta_path, serde_json::to_string_pretty(&meta).unwrap_or_default());
    }
}

/// Simple ISO timestamp without pulling in chrono.
fn chrono_now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // Good enough for a debug file — seconds since epoch
    format!("{secs}")
}

fn load_cache(
    path: &Path,
    expected_dim: usize,
    expected_model: &str,
) -> HashMap<String, CachedFile> {
    let mut map = HashMap::new();
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return map,
    };
    let mut r = std::io::BufReader::new(file);

    // Header
    let mut magic = [0u8; 4];
    if r.read_exact(&mut magic).is_err() || &magic != CACHE_MAGIC {
        return map;
    }
    let mut buf2 = [0u8; 2];
    if r.read_exact(&mut buf2).is_err() {
        return map;
    }
    if u16::from_le_bytes(buf2) != CACHE_VERSION {
        return map;
    }
    if r.read_exact(&mut buf2).is_err() {
        return map;
    }
    let dim = u16::from_le_bytes(buf2) as usize;
    if dim != expected_dim {
        return map;
    }
    if r.read_exact(&mut buf2).is_err() {
        return map;
    }
    let model_len = u16::from_le_bytes(buf2) as usize;
    let mut model_buf = vec![0u8; model_len];
    if r.read_exact(&mut model_buf).is_err() {
        return map;
    }
    if String::from_utf8_lossy(&model_buf) != expected_model {
        return map;
    }

    // Entries — read until EOF or corrupt data
    loop {
        let mut buf4 = [0u8; 4];
        if r.read_exact(&mut buf4).is_err() {
            break;
        }
        let path_len = u32::from_le_bytes(buf4) as usize;
        let mut path_buf = vec![0u8; path_len];
        if r.read_exact(&mut path_buf).is_err() {
            break;
        }
        let rel_path = String::from_utf8_lossy(&path_buf).into_owned();

        let mut buf8 = [0u8; 8];
        if r.read_exact(&mut buf8).is_err() {
            break;
        }
        let file_size = u64::from_le_bytes(buf8);
        if r.read_exact(&mut buf8).is_err() {
            break;
        }
        let mtime_secs = i64::from_le_bytes(buf8);

        if r.read_exact(&mut buf4).is_err() {
            break;
        }
        let n_chunks = u32::from_le_bytes(buf4) as usize;

        let mut chunks = Vec::with_capacity(n_chunks);
        let mut valid = true;
        for _ in 0..n_chunks {
            if r.read_exact(&mut buf4).is_err() {
                valid = false;
                break;
            }
            let start_line = u32::from_le_bytes(buf4) as usize;

            if r.read_exact(&mut buf2).is_err() {
                valid = false;
                break;
            }
            let snippet_len = u16::from_le_bytes(buf2) as usize;
            let mut snippet_buf = vec![0u8; snippet_len];
            if r.read_exact(&mut snippet_buf).is_err() {
                valid = false;
                break;
            }
            let snippet = String::from_utf8_lossy(&snippet_buf).into_owned();

            let mut emb_buf = vec![0u8; dim * 4];
            if r.read_exact(&mut emb_buf).is_err() {
                valid = false;
                break;
            }
            let embedding: Vec<f32> = emb_buf
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect();

            chunks
                .push((ChunkMeta { file_path: rel_path.clone(), start_line, snippet }, embedding));
        }
        if !valid {
            break; // truncated entry — stop reading, keep what we have
        }
        // Later entries for same path overwrite earlier ones
        map.insert(rel_path, CachedFile { file_size, mtime_secs, chunks });
    }

    map
}

fn write_cache_header(w: &mut impl IoWrite, dim: usize, model_name: &str) -> std::io::Result<()> {
    w.write_all(CACHE_MAGIC)?;
    w.write_all(&CACHE_VERSION.to_le_bytes())?;
    w.write_all(&(dim as u16).to_le_bytes())?;
    let model_bytes = model_name.as_bytes();
    w.write_all(&(model_bytes.len() as u16).to_le_bytes())?;
    w.write_all(model_bytes)?;
    Ok(())
}

fn write_cache_entry(
    w: &mut impl IoWrite,
    rel_path: &str,
    file_size: u64,
    mtime_secs: i64,
    chunks: &[(ChunkMeta, Vec<f32>)],
) -> std::io::Result<()> {
    let path_bytes = rel_path.as_bytes();
    w.write_all(&(path_bytes.len() as u32).to_le_bytes())?;
    w.write_all(path_bytes)?;
    w.write_all(&file_size.to_le_bytes())?;
    w.write_all(&mtime_secs.to_le_bytes())?;
    w.write_all(&(chunks.len() as u32).to_le_bytes())?;
    for (meta, emb) in chunks {
        w.write_all(&(meta.start_line as u32).to_le_bytes())?;
        let snippet_bytes = meta.snippet.as_bytes();
        w.write_all(&(snippet_bytes.len() as u16).to_le_bytes())?;
        w.write_all(snippet_bytes)?;
        for &f in emb {
            w.write_all(&f.to_le_bytes())?;
        }
    }
    Ok(())
}

/// Make a snippet from chunk text (first 200 chars, respecting char boundaries).
fn make_snippet(text: &str) -> String {
    if text.len() <= 200 {
        text.to_string()
    } else {
        let mut end = 200;
        while !text.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        text[..end].to_string()
    }
}

// ---------------------------------------------------------------------------
// Index building — incremental with progressive cache writes
// ---------------------------------------------------------------------------

/// Build a semantic index from scanned files.
///
/// Loads per-file cache from the centralized cache directory. Files with matching
/// (size, mtime) use cached embeddings. Only changed/new files are embedded.
/// Cache entries are written progressively — if interrupted, completed files
/// survive for the next startup.
pub fn build_semantic_index(
    files: &[ScannedFile],
    model_name: Option<&str>,
    progress: &crate::types::SemanticProgress,
    repo_root: &Path,
    #[cfg(feature = "treesitter")] ast_index: Option<&AstIndex>,
) -> Option<SemanticIndex> {
    use std::sync::atomic::Ordering::Relaxed;

    // Phase 1: Extract chunks grouped by file
    progress.status.store(1, Relaxed);
    let tier = resolve_model(model_name);
    let file_chunks = extract_chunks_by_file(
        files,
        tier.max_chunk_chars,
        #[cfg(feature = "treesitter")]
        ast_index,
    );

    let total_chunks: usize = file_chunks.iter().map(|fc| fc.chunks.len()).sum();
    if total_chunks == 0 {
        tracing::warn!("No chunks extracted, skipping semantic index");
        progress.status.store(4, Relaxed);
        return None;
    }

    progress.total_chunks.store(total_chunks, Relaxed);
    tracing::info!(
        chunks = total_chunks,
        files = file_chunks.len(),
        "Extracted chunks for embedding"
    );

    // Phase 2: Load cache, separate hits from misses
    // Try central cache first, fall back to legacy in-repo location
    let stored_model = tier.cache_name;
    let cp = cache_path(repo_root);
    let legacy_cp = legacy_cache_path(repo_root);
    let (cache, used_legacy) = {
        let central = load_cache(&cp, tier.dim, stored_model);
        if !central.is_empty() {
            tracing::debug!(path = %cp.display(), "Loaded embedding cache");
            (central, false)
        } else if cp != legacy_cp {
            let legacy = load_cache(&legacy_cp, tier.dim, stored_model);
            if !legacy.is_empty() {
                tracing::debug!(path = %legacy_cp.display(), "Migrating legacy embedding cache");
                (legacy, true)
            } else {
                (HashMap::new(), false)
            }
        } else {
            (HashMap::new(), false)
        }
    };
    let _ = used_legacy; // used below when writing meta.json

    let mut cached_embs: Vec<f32> = Vec::new();
    let mut cached_meta: Vec<ChunkMeta> = Vec::new();
    let mut to_embed: Vec<&FileChunks> = Vec::new();

    for fc in &file_chunks {
        if let Some(entry) = cache.get(&fc.rel_path) {
            if entry.file_size == fc.file_size && entry.mtime_secs == fc.mtime_secs {
                for (meta, emb) in &entry.chunks {
                    cached_embs.extend_from_slice(emb);
                    cached_meta.push(ChunkMeta {
                        file_path: meta.file_path.clone(),
                        start_line: meta.start_line,
                        snippet: meta.snippet.clone(),
                    });
                }
                continue;
            }
        }
        to_embed.push(fc);
    }

    let cache_hits = cached_meta.len();
    let miss_chunks: usize = to_embed.iter().map(|fc| fc.chunks.len()).sum();
    tracing::info!(
        cache_hits = cache_hits,
        to_embed = miss_chunks,
        changed_files = to_embed.len(),
        "Embedding cache status"
    );

    // Phase 3: Open cache file for progressive writes
    // Write header + all cache-hit entries first, then append as batches complete.
    let cache_writer = {
        if let Some(parent) = cp.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::File::create(&cp) {
            Ok(f) => {
                let mut w = std::io::BufWriter::new(f);
                if write_cache_header(&mut w, tier.dim, stored_model).is_err() {
                    tracing::warn!("Failed to write embedding cache header");
                }
                // Write cache-hit entries
                for fc in &file_chunks {
                    if let Some(entry) = cache.get(&fc.rel_path) {
                        if entry.file_size == fc.file_size && entry.mtime_secs == fc.mtime_secs {
                            let _ = write_cache_entry(
                                &mut w,
                                &fc.rel_path,
                                entry.file_size,
                                entry.mtime_secs,
                                &entry.chunks,
                            );
                        }
                    }
                }
                let _ = w.flush();
                Some(std::sync::Mutex::new(w))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Cannot write embedding cache");
                None
            }
        }
    };
    drop(cache); // free memory from old cache

    // Fast path: everything cached
    if to_embed.is_empty() {
        progress.status.store(3, Relaxed);
        tracing::info!(chunks = cache_hits, "Semantic index fully cached, loaded instantly");
        write_cache_meta(repo_root, stored_model, cached_meta.len());
        return Some(SemanticIndex {
            embeddings: cached_embs,
            chunk_meta: cached_meta,
            dim: tier.dim,
            model_name: stored_model.to_string(),
        });
    }

    // Phase 4: Create embedding model (fastembed handles ORT session, threading)
    let mut model = match create_embedding_model(&tier) {
        Ok(m) => m,
        Err(e) => {
            tracing::error!(error = %e, "Failed to create embedding model");
            progress.status.store(4, Relaxed);
            return None;
        }
    };

    // Dynamic batch sizing for ~20-40 progress updates.
    let batch_size = (miss_chunks / 30).clamp(16, 64);
    *progress.device.write().unwrap() = device_label().to_string();
    let total_batches = miss_chunks.div_ceil(batch_size);
    progress.total_batches.store(total_batches, Relaxed);
    progress.completed_batches.store(0, Relaxed);
    progress.status.store(2, Relaxed);

    tracing::info!(
        batches = total_batches,
        device = device_label(),
        "Embedding chunks"
    );

    // Build flat chunk list, then process in sequential batches
    struct ChunkRef {
        file_idx: usize,
        chunk_idx: usize,
    }

    let mut chunk_refs: Vec<ChunkRef> = Vec::with_capacity(miss_chunks);
    for (fi, fc) in to_embed.iter().enumerate() {
        for ci in 0..fc.chunks.len() {
            chunk_refs.push(ChunkRef { file_idx: fi, chunk_idx: ci });
        }
    }

    // Per-file result accumulator for cache writes
    type FileResult = Vec<(ChunkMeta, Vec<f32>)>;
    let mut file_results: Vec<FileResult> = to_embed
        .iter()
        .map(|fc| Vec::with_capacity(fc.chunks.len()))
        .collect();

    let mut all_embeddings = cached_embs;
    let mut chunk_meta = cached_meta;
    let mut completed_batches = 0usize;

    for batch in chunk_refs.chunks(batch_size) {
        let texts: Vec<String> = batch
            .iter()
            .map(|cr| {
                let text = &to_embed[cr.file_idx].chunks[cr.chunk_idx].text;
                match tier.document_prefix {
                    Some(prefix) => format!("{prefix}{text}"),
                    None => text.clone(),
                }
            })
            .collect();

        match model.embed(texts, Some(batch.len())) {
            Ok(embeddings) => {
                for (i, emb) in embeddings.into_iter().enumerate() {
                    let cr = &batch[i];
                    let fc = &to_embed[cr.file_idx];
                    let chunk = &fc.chunks[cr.chunk_idx];
                    let meta = ChunkMeta {
                        file_path: fc.rel_path.clone(),
                        start_line: chunk.start_line,
                        snippet: make_snippet(&chunk.text),
                    };

                    file_results[cr.file_idx].push((meta.clone(), emb.clone()));
                    all_embeddings.extend_from_slice(&emb);
                    chunk_meta.push(meta);
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Batch embed failed");
                continue;
            }
        }

        completed_batches += 1;
        progress.completed_batches.store(completed_batches, Relaxed);
        tracing::info!(done = completed_batches, total = total_batches, "Embedding progress");
    }

    // Write cache entries for all embedded files
    if let Some(ref writer) = cache_writer {
        if let Ok(mut w) = writer.lock() {
            for (fi, fc) in to_embed.iter().enumerate() {
                if file_results[fi].len() == fc.chunks.len() {
                    let _ = write_cache_entry(
                        &mut *w,
                        &fc.rel_path,
                        fc.file_size,
                        fc.mtime_secs,
                        &file_results[fi],
                    );
                }
            }
            let _ = w.flush();
        }
    }

    if chunk_meta.is_empty() {
        tracing::warn!("No embeddings produced");
        progress.status.store(4, Relaxed);
        return None;
    }

    progress.status.store(3, Relaxed);
    tracing::info!(
        total = chunk_meta.len(),
        cached = cache_hits,
        embedded = chunk_meta.len() - cache_hits,
        "Semantic index ready"
    );

    // Write meta.json alongside the cache for debugging
    write_cache_meta(repo_root, stored_model, chunk_meta.len());

    Some(SemanticIndex {
        embeddings: all_embeddings,
        chunk_meta,
        dim: tier.dim,
        model_name: stored_model.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Result of a semantic search query.
pub struct SemanticSearchResult {
    pub file_path: String,
    pub start_line: usize,
    pub snippet: String,
    pub score: f32,
}

/// Adjust cosine similarity score using path-based signals.
/// Penalizes third-party code, boosts first-party engine code and header files,
/// and rewards path components that match query terms.
fn adjusted_score(cosine: f32, file_path: &str, query_terms: &[&str]) -> f32 {
    let mut score = cosine;
    let path_lower = file_path.to_lowercase();

    // Penalties: third-party and known noisy libraries
    if path_lower.contains("/thirdparty/")
        || path_lower.contains("/external/")
        || path_lower.contains("/intermediate/")
    {
        score *= 0.3;
    }
    const NOISY_LIBS: &[&str] = &[
        "boost",
        "icu4c",
        "imgui",
        "embree",
        "directshow",
        "vulkan",
        "zlib",
        "freetype",
        "openssl",
        "libpng",
        "metricsdiscovery",
    ];
    if NOISY_LIBS.iter().any(|lib| path_lower.contains(lib)) {
        score *= 0.2;
    }

    // Boosts: header files are more useful API surfaces
    let ext = file_path.rsplit('.').next().unwrap_or("");
    if matches!(ext, "h" | "hpp" | "hxx") {
        score *= 1.15;
    }

    // Boost: query terms found in path components
    let path_parts: Vec<&str> = file_path.split('/').collect();
    let hits = query_terms
        .iter()
        .filter(|t| t.len() >= 4)
        .filter(|t| {
            let tl = t.to_lowercase();
            path_parts.iter().any(|p| p.to_lowercase().contains(&tl))
        })
        .count();
    if hits > 0 {
        score *= 1.0 + 0.15 * hits as f32;
    }

    // Boost: first-party engine source
    if path_lower.contains("/engine/source/runtime/")
        || path_lower.contains("/engine/source/editor/")
        || path_lower.contains("/engine/plugins/")
    {
        score *= 1.1;
    }

    score
}

/// Search the semantic index for chunks similar to the query.
/// Retrieves oversample candidates, reranks with path-based signals,
/// deduplicates to one result per file, and returns top-K.
pub fn semantic_search(
    index: &SemanticIndex,
    query: &str,
    limit: usize,
) -> Result<Vec<SemanticSearchResult>, String> {
    let tier = resolve_model(Some(&index.model_name));
    let mut model = create_embedding_model(&tier)?;

    // Prepend query prefix if the model requires it
    let query_text = match tier.query_prefix {
        Some(prefix) => format!("{prefix}{query}"),
        None => query.to_string(),
    };

    let query_embeddings = model
        .embed(vec![query_text], None)
        .map_err(|e| format!("Query embedding failed: {e}"))?;
    if query_embeddings.is_empty() {
        return Ok(Vec::new());
    }
    let query_emb = &query_embeddings[0];

    let n_chunks = index.chunk_meta.len();
    let dim = index.dim;

    // Cosine similarity (embeddings are already L2-normalized, so dot product = cosine sim)
    let mut scores: Vec<(usize, f32)> = Vec::with_capacity(n_chunks);
    for i in 0..n_chunks {
        let offset = i * dim;
        let chunk_emb = &index.embeddings[offset..offset + dim];
        let dot: f32 = query_emb.iter().zip(chunk_emb.iter()).map(|(a, b)| a * b).sum();
        scores.push((i, dot));
    }

    // Filter out low-relevance results before ranking — prevents garbage results
    // for queries with no meaningful matches. Threshold tuned for code search;
    // code vocabulary mismatch requires a lower cutoff than general text (~0.3-0.5).
    const MIN_SEMANTIC_SCORE: f32 = 0.25;
    scores.retain(|(_, dot)| *dot >= MIN_SEMANTIC_SCORE);
    if scores.is_empty() {
        return Ok(Vec::new());
    }

    // Oversample: retrieve more candidates for reranking
    let oversample = limit * 6;
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scores.truncate(oversample);

    // Extract query terms for path matching
    let query_terms: Vec<&str> = query.split_whitespace().filter(|t| t.len() >= 4).collect();

    // Rerank with path-based signals
    let mut results: Vec<SemanticSearchResult> = scores
        .into_iter()
        .map(|(idx, cosine)| {
            let meta = &index.chunk_meta[idx];
            let score = adjusted_score(cosine, &meta.file_path, &query_terms);
            SemanticSearchResult {
                file_path: meta.file_path.clone(),
                start_line: meta.start_line,
                snippet: meta.snippet.clone(),
                score,
            }
        })
        .collect();

    // Re-sort by adjusted score
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    // Deduplicate: keep only the best-scoring chunk per file
    let mut seen_files = std::collections::HashSet::new();
    let mut deduped: Vec<SemanticSearchResult> = Vec::with_capacity(limit);
    for r in results {
        if seen_files.insert(r.file_path.clone()) {
            deduped.push(r);
            if deduped.len() >= limit {
                break;
            }
        }
    }

    Ok(deduped)
}
