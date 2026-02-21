//! Semantic code search using BERT embeddings (all-MiniLM-L6-v2 by default).
//!
//! Chunks source files by logical boundaries, generates embeddings via candle,
//! and ranks results by cosine similarity. Supports CUDA GPU acceleration and
//! persistent caching to avoid re-embedding unchanged files.

use crate::stubs::extract_stubs;
use crate::types::{ChunkMeta, ScannedFile, SemanticIndex};

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config as BertConfig};
use hf_hub::{api::sync::Api, Repo, RepoType};
use tokenizers::Tokenizer;

use std::collections::HashMap;
use std::io::{Read as IoRead, Write as IoWrite};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Model configuration — presets and custom HuggingFace models
// ---------------------------------------------------------------------------

/// Configuration for an embedding model.
pub struct ModelConfig {
    pub model_id: String,
    pub dim: usize,
    pub max_chunk_chars: usize,
}

/// Resolve a model name to its configuration.
///
/// Accepts preset names ("minilm", "codebert", "starencoder"), returns the
/// default (minilm) for `None`, or treats any other string as a custom
/// HuggingFace model ID (defaults to dim=768, chunk=2000 — override dim
/// in .codescope.toml for non-768 models).
pub fn resolve_model(name: Option<&str>) -> ModelConfig {
    match name {
        None | Some("minilm") => ModelConfig {
            model_id: "sentence-transformers/all-MiniLM-L6-v2".into(),
            dim: 384,
            max_chunk_chars: 1500,
        },
        Some("codebert") => ModelConfig {
            model_id: "microsoft/codebert-base".into(),
            dim: 768,
            max_chunk_chars: 2000,
        },
        Some("starencoder") => {
            ModelConfig { model_id: "bigcode/starencoder".into(), dim: 768, max_chunk_chars: 2000 }
        }
        Some(custom) => {
            ModelConfig { model_id: custom.to_string(), dim: 768, max_chunk_chars: 2000 }
        }
    }
}

/// Select the best available device: CUDA GPU if available, otherwise CPU.
fn select_device() -> Device {
    #[cfg(feature = "cuda")]
    {
        match Device::new_cuda(0) {
            Ok(dev) => {
                return dev;
            }
            Err(e) => {
                tracing::warn!(error = %e, "CUDA unavailable, falling back to CPU");
            }
        }
    }
    Device::Cpu
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

/// Extract embeddable chunks grouped by file. Parallelized via rayon.
/// Pre-filters by extension and file size. Returns file metadata for cache keying.
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

fn extract_chunks_by_file(files: &[ScannedFile], max_chunk_chars: usize) -> Vec<FileChunks> {
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
            let stubs = extract_stubs(&content, &file.ext);
            if stubs.trim().is_empty() {
                return None;
            }

            let chunks = split_stubs_into_chunks(&stubs, max_chunk_chars);
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
// Model loading
// ---------------------------------------------------------------------------

/// Load the BERT model and tokenizer from HuggingFace Hub.
/// Models are cached in `~/.cache/codescope/models/` via hf-hub defaults.
/// Returns (model, tokenizer, device) so callers reuse the same device.
fn load_model(config: &ModelConfig) -> Result<(BertModel, Tokenizer, Device), String> {
    let model_id = &config.model_id;
    let device = select_device();
    let device_name = match &device {
        Device::Cpu => "CPU".to_string(),
        #[cfg(feature = "cuda")]
        Device::Cuda(_) => "CUDA GPU".to_string(),
        #[allow(unreachable_patterns)]
        _ => "unknown".to_string(),
    };

    let api = Api::new().map_err(|e| format!("Failed to create HF API: {e}"))?;

    let repo =
        api.repo(Repo::with_revision(model_id.to_string(), RepoType::Model, "main".to_string()));

    tracing::info!(model = model_id, device = device_name.as_str(), "Loading embedding model");

    let config_path =
        repo.get("config.json").map_err(|e| format!("Failed to get config.json: {e}"))?;
    let tokenizer_path =
        repo.get("tokenizer.json").map_err(|e| format!("Failed to get tokenizer.json: {e}"))?;
    let weights_path = repo
        .get("model.safetensors")
        .map_err(|e| format!("Failed to get model.safetensors: {e}"))?;

    let config_str =
        std::fs::read_to_string(&config_path).map_err(|e| format!("Failed to read config: {e}"))?;
    let config: BertConfig =
        serde_json::from_str(&config_str).map_err(|e| format!("Failed to parse config: {e}"))?;

    let tokenizer = Tokenizer::from_file(&tokenizer_path)
        .map_err(|e| format!("Failed to load tokenizer: {e}"))?;

    let vb = unsafe {
        VarBuilder::from_mmaped_safetensors(&[weights_path], DType::F32, &device)
            .map_err(|e| format!("Failed to load weights: {e}"))?
    };

    let model =
        BertModel::load(vb, &config).map_err(|e| format!("Failed to load BERT model: {e}"))?;

    tracing::info!(device = device_name.as_str(), "Embedding model loaded");
    Ok((model, tokenizer, device))
}

// ---------------------------------------------------------------------------
// Embedding generation
// ---------------------------------------------------------------------------

/// Encode a batch of texts into embeddings using mean pooling.
fn encode_batch(
    model: &BertModel,
    tokenizer: &Tokenizer,
    device: &Device,
    texts: &[&str],
    dim: usize,
) -> Result<Vec<Vec<f32>>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let encodings = tokenizer
        .encode_batch(texts.to_vec(), true)
        .map_err(|e| format!("Tokenization failed: {e}"))?;

    let max_len = encodings.iter().map(|e| e.get_ids().len()).max().unwrap_or(0);

    let mut all_ids: Vec<u32> = Vec::new();
    let mut all_mask: Vec<u32> = Vec::new();
    let mut all_type_ids: Vec<u32> = Vec::new();

    for enc in &encodings {
        let ids = enc.get_ids();
        let mask = enc.get_attention_mask();
        let type_ids = enc.get_type_ids();
        let pad_len = max_len - ids.len();

        all_ids.extend_from_slice(ids);
        all_ids.extend(std::iter::repeat_n(0u32, pad_len));

        all_mask.extend_from_slice(mask);
        all_mask.extend(std::iter::repeat_n(0u32, pad_len));

        all_type_ids.extend_from_slice(type_ids);
        all_type_ids.extend(std::iter::repeat_n(0u32, pad_len));
    }

    let batch_size = texts.len();
    let input_ids = Tensor::from_vec(all_ids, (batch_size, max_len), device)
        .map_err(|e| format!("Tensor creation failed: {e}"))?;
    let attention_mask = Tensor::from_vec(
        all_mask.iter().map(|&x| x as f32).collect::<Vec<_>>(),
        (batch_size, max_len),
        device,
    )
    .map_err(|e| format!("Tensor creation failed: {e}"))?;
    let token_type_ids = Tensor::from_vec(all_type_ids, (batch_size, max_len), device)
        .map_err(|e| format!("Tensor creation failed: {e}"))?;

    // Forward pass
    let output = model
        .forward(&input_ids, &token_type_ids, Some(&attention_mask))
        .map_err(|e| format!("Model forward pass failed: {e}"))?;

    // Mean pooling: sum(output * mask) / sum(mask)
    let mask_expanded = attention_mask
        .unsqueeze(2)
        .map_err(|e| format!("unsqueeze failed: {e}"))?
        .broadcast_as(output.shape())
        .map_err(|e| format!("broadcast failed: {e}"))?;

    let masked = output.mul(&mask_expanded).map_err(|e| format!("mul failed: {e}"))?;

    let summed = masked.sum(1).map_err(|e| format!("sum failed: {e}"))?;

    let mask_sum = mask_expanded
        .sum(1)
        .map_err(|e| format!("mask sum failed: {e}"))?
        .clamp(1e-9, f64::MAX)
        .map_err(|e| format!("clamp failed: {e}"))?;

    let mean_pooled = summed.div(&mask_sum).map_err(|e| format!("div failed: {e}"))?;

    // L2 normalize
    let norms = mean_pooled
        .sqr()
        .map_err(|e| format!("sqr failed: {e}"))?
        .sum(1)
        .map_err(|e| format!("norm sum failed: {e}"))?
        .sqrt()
        .map_err(|e| format!("sqrt failed: {e}"))?
        .unsqueeze(1)
        .map_err(|e| format!("unsqueeze failed: {e}"))?
        .broadcast_as(mean_pooled.shape())
        .map_err(|e| format!("broadcast failed: {e}"))?
        .clamp(1e-9, f64::MAX)
        .map_err(|e| format!("clamp failed: {e}"))?;

    let normalized = mean_pooled.div(&norms).map_err(|e| format!("div failed: {e}"))?;

    // Extract to Vec<Vec<f32>>
    let flat: Vec<f32> = normalized
        .flatten_all()
        .map_err(|e| format!("flatten failed: {e}"))?
        .to_vec1()
        .map_err(|e| format!("to_vec1 failed: {e}"))?;

    let mut result = Vec::with_capacity(batch_size);
    for i in 0..batch_size {
        let start = i * dim;
        let end = start + dim;
        result.push(flat[start..end].to_vec());
    }

    Ok(result)
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
const CACHE_VERSION: u16 = 2; // bumped: chunks now include path context + doc comments

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
/// Loads per-file cache from `.codescope/semantic.cache`. Files with matching
/// (size, mtime) use cached embeddings. Only changed/new files are embedded.
/// Cache entries are written progressively — if interrupted, completed files
/// survive for the next startup.
pub fn build_semantic_index(
    files: &[ScannedFile],
    model_name: Option<&str>,
    progress: &crate::types::SemanticProgress,
    repo_root: &Path,
) -> Option<SemanticIndex> {
    use std::sync::atomic::Ordering::Relaxed;

    // Phase 1: Extract chunks grouped by file
    progress.status.store(1, Relaxed);
    let model_config = resolve_model(model_name);
    let file_chunks = extract_chunks_by_file(files, model_config.max_chunk_chars);

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
    let stored_model = model_name.unwrap_or("minilm");
    let cp = cache_path(repo_root);
    let legacy_cp = legacy_cache_path(repo_root);
    let (cache, used_legacy) = {
        let central = load_cache(&cp, model_config.dim, stored_model);
        if !central.is_empty() {
            tracing::debug!(path = %cp.display(), "Loaded embedding cache");
            (central, false)
        } else if cp != legacy_cp {
            let legacy = load_cache(&legacy_cp, model_config.dim, stored_model);
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
    // Write header + all cache-hit entries first, then append as workers complete files.
    let cache_writer = {
        if let Some(parent) = cp.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::File::create(&cp) {
            Ok(f) => {
                let mut w = std::io::BufWriter::new(f);
                if write_cache_header(&mut w, model_config.dim, stored_model).is_err() {
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
            dim: model_config.dim,
            model_name: stored_model.to_string(),
        });
    }

    // Phase 4: Embed misses — distribute files across workers
    let use_gpu = !matches!(select_device(), Device::Cpu);
    // Dynamic batch sizing: aim for ~20 progress updates minimum so the UI
    // shows meaningful granularity, but clamp to hardware-reasonable bounds.
    let max_batch = if use_gpu { 512 } else { 64 };
    let min_batch = if use_gpu { 32 } else { 8 };
    let target_batches = 20;
    let batch_size = (miss_chunks / target_batches).clamp(min_batch, max_batch);
    let n_workers = if use_gpu { 1 } else { num_cpus().min(to_embed.len()).max(1) };

    let device_label = if use_gpu { "GPU" } else { "CPU" };
    *progress.device.write().unwrap() = device_label.to_string();
    let total_batches = miss_chunks.div_ceil(batch_size);
    progress.total_batches.store(total_batches, Relaxed);
    progress.completed_batches.store(0, Relaxed);
    progress.status.store(2, Relaxed);

    tracing::info!(
        batches = total_batches,
        workers = n_workers,
        device = %device_label,
        "Embedding chunks"
    );

    // Build a flat list of (file_index, chunk_index) pairs, then split into
    // batch_size batches. This packs small files together into full GPU batches
    // instead of sending tiny partial batches per file.
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

    // Split into batches, then distribute batches to workers
    let batches: Vec<&[ChunkRef]> = chunk_refs.chunks(batch_size).collect();
    let group_size = batches.len().div_ceil(n_workers);
    let batch_groups: Vec<Vec<&[ChunkRef]>> =
        batches.chunks(group_size).map(|g| g.to_vec()).collect();

    let batch_counter = std::sync::atomic::AtomicUsize::new(0);
    let model_config = &model_config;
    let cache_writer = &cache_writer;
    let to_embed_ref = &to_embed;

    // Per-file result accumulator: (embeddings, complete?)
    // Workers write results here; we flush to cache after all workers finish.
    type FileResult = Vec<(ChunkMeta, Vec<f32>)>;
    let file_results: Vec<std::sync::Mutex<FileResult>> = to_embed
        .iter()
        .map(|fc| std::sync::Mutex::new(Vec::with_capacity(fc.chunks.len())))
        .collect();
    let file_results = &file_results;

    // Each worker: load model, process packed batches, store results per-file
    let worker_results: Vec<Option<(Vec<f32>, Vec<ChunkMeta>)>> = std::thread::scope(|s| {
        let handles: Vec<_> = batch_groups
            .iter()
            .enumerate()
            .map(|(worker_id, group)| {
                let batch_counter = &batch_counter;
                s.spawn(move || {
                    let (model, tokenizer, device) = match load_model(model_config) {
                        Ok(m) => m,
                        Err(e) => {
                            tracing::error!(worker = worker_id, error = %e, "Worker failed to load model");
                            return None;
                        }
                    };

                    let mut all_embs: Vec<f32> = Vec::new();
                    let mut all_metas: Vec<ChunkMeta> = Vec::new();

                    for batch in group.iter() {
                        let texts: Vec<&str> = batch
                            .iter()
                            .map(|cr| to_embed_ref[cr.file_idx].chunks[cr.chunk_idx].text.as_str())
                            .collect();

                        match encode_batch(&model, &tokenizer, &device, &texts, model_config.dim) {
                            Ok(embeddings) => {
                                for (i, emb) in embeddings.into_iter().enumerate() {
                                    let cr = &batch[i];
                                    let fc = &to_embed_ref[cr.file_idx];
                                    let chunk = &fc.chunks[cr.chunk_idx];
                                    let meta = ChunkMeta {
                                        file_path: fc.rel_path.clone(),
                                        start_line: chunk.start_line,
                                        snippet: make_snippet(&chunk.text),
                                    };

                                    // Store per-file for cache writes
                                    file_results[cr.file_idx]
                                        .lock()
                                        .unwrap()
                                        .push((meta.clone(), emb.clone()));

                                    all_embs.extend_from_slice(&emb);
                                    all_metas.push(meta);
                                }
                            }
                            Err(e) => {
                                tracing::warn!(worker = worker_id, error = %e, "Batch encode failed");
                                continue;
                            }
                        }

                        let done =
                            batch_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        progress.completed_batches.store(done, Relaxed);
                        if done.is_multiple_of(20) || done == total_batches {
                            tracing::info!(done = done, total = total_batches, "Embedding progress");
                        }
                    }

                    Some((all_embs, all_metas))
                })
            })
            .collect();

        handles.into_iter().map(|h| h.join().unwrap_or(None)).collect()
    });

    // Write cache entries for all embedded files
    if let Some(ref writer) = cache_writer {
        if let Ok(mut w) = writer.lock() {
            for (fi, fc) in to_embed.iter().enumerate() {
                let results = file_results[fi].lock().unwrap();
                if results.len() == fc.chunks.len() {
                    let _ = write_cache_entry(
                        &mut *w,
                        &fc.rel_path,
                        fc.file_size,
                        fc.mtime_secs,
                        &results,
                    );
                }
            }
            let _ = w.flush();
        }
    }

    let results = worker_results;

    // Merge cached + freshly embedded
    let mut all_embeddings = cached_embs;
    let mut chunk_meta = cached_meta;
    for result in results.into_iter().flatten() {
        all_embeddings.extend(result.0);
        chunk_meta.extend(result.1);
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
        dim: model_config.dim,
        model_name: stored_model.to_string(),
    })
}

/// Get number of available CPU cores.
fn num_cpus() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
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
    let model_config = resolve_model(Some(&index.model_name));
    let (model, tokenizer, device) = load_model(&model_config)?;

    let query_embeddings = encode_batch(&model, &tokenizer, &device, &[query], model_config.dim)?;
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
    // for queries with no meaningful matches. Threshold tuned for MiniLM on code;
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
