// ---------------------------------------------------------------------------
// Semantic search — embed code chunks via all-MiniLM-L6-v2, search by cosine similarity
// ---------------------------------------------------------------------------

use crate::stubs::extract_stubs;
use crate::types::{ChunkMeta, ScannedFile, SemanticIndex};

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config as BertConfig};
use hf_hub::{api::sync::Api, Repo, RepoType};
use tokenizers::Tokenizer;

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
                eprintln!("  [semantic] CUDA unavailable ({e}), falling back to CPU");
            }
        }
    }
    Device::Cpu
}

// ---------------------------------------------------------------------------
// Chunk extraction
// ---------------------------------------------------------------------------

/// A code chunk ready for embedding.
struct Chunk {
    file_path: String,
    start_line: usize,
    text: String,
}

/// Extract embeddable chunks from scanned files using structural stubs.
fn extract_chunks(files: &[ScannedFile], max_chunk_chars: usize) -> Vec<Chunk> {
    let mut chunks = Vec::new();

    for file in files {
        let content = match std::fs::read_to_string(&file.abs_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let stubs = extract_stubs(&content, &file.ext);
        if stubs.trim().is_empty() {
            continue;
        }

        // Split stubs into individual chunks at blank-line boundaries
        let mut current_chunk = String::new();
        let mut chunk_start_line = 1usize;
        let mut line_num = 1usize;

        for line in stubs.lines() {
            let trimmed = line.trim();

            // Split on blank lines or when chunk gets too large
            if trimmed.is_empty() && !current_chunk.trim().is_empty() {
                if current_chunk.len() >= 40 {
                    chunks.push(Chunk {
                        file_path: file.rel_path.clone(),
                        start_line: chunk_start_line,
                        text: current_chunk.clone(),
                    });
                }
                current_chunk.clear();
                chunk_start_line = line_num + 1;
            } else {
                if current_chunk.len() + line.len() + 1 > max_chunk_chars
                    && !current_chunk.is_empty()
                {
                    chunks.push(Chunk {
                        file_path: file.rel_path.clone(),
                        start_line: chunk_start_line,
                        text: current_chunk.clone(),
                    });
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

        // Flush remaining
        if current_chunk.len() >= 40 {
            chunks.push(Chunk {
                file_path: file.rel_path.clone(),
                start_line: chunk_start_line,
                text: current_chunk,
            });
        }
    }

    chunks
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

    eprintln!("  [semantic] Loading model {model_id} on {device_name}...");

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

    eprintln!("  [semantic] Model loaded on {device_name}");
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
        all_ids.extend(std::iter::repeat(0u32).take(pad_len));

        all_mask.extend_from_slice(mask);
        all_mask.extend(std::iter::repeat(0u32).take(pad_len));

        all_type_ids.extend_from_slice(type_ids);
        all_type_ids.extend(std::iter::repeat(0u32).take(pad_len));
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
// Index building
// ---------------------------------------------------------------------------

/// Build a semantic index from scanned files.
/// Uses parallel workers to saturate CPU cores during embedding.
/// Returns `None` if model loading fails or no chunks found.
pub fn build_semantic_index(
    files: &[ScannedFile],
    model_name: Option<&str>,
) -> Option<SemanticIndex> {
    let model_config = resolve_model(model_name);
    let chunks = extract_chunks(files, model_config.max_chunk_chars);
    if chunks.is_empty() {
        eprintln!("  [semantic] No chunks extracted, skipping semantic index");
        return None;
    }

    eprintln!("  [semantic] Extracted {} chunks from {} files", chunks.len(), files.len());

    // Pre-load model once to validate, warm the HF cache, and detect device
    let use_gpu = match load_model(&model_config) {
        Ok((_, _, ref dev)) => !matches!(dev, Device::Cpu),
        Err(e) => {
            eprintln!("  [semantic] Failed to load model: {e}");
            return None;
        }
    };

    // GPU: single worker with larger batches (GPU handles parallelism internally)
    // CPU: multiple workers to saturate cores
    let batch_size = if use_gpu { 128 } else { 32 };
    let total_batches = (chunks.len() + batch_size - 1) / batch_size;
    let n_workers = if use_gpu { 1 } else { num_cpus().min(total_batches).max(1) };

    let device_label = if use_gpu { "GPU" } else { "CPU" };
    eprintln!(
        "  [semantic] Embedding {} batches across {} worker(s) on {device_label}...",
        total_batches, n_workers
    );

    // Split chunks into per-worker groups
    let batches: Vec<&[Chunk]> = chunks.chunks(batch_size).collect();
    let group_size = (batches.len() + n_workers - 1) / n_workers;
    let groups: Vec<Vec<&[Chunk]>> = batches.chunks(group_size).map(|g| g.to_vec()).collect();

    let progress = std::sync::atomic::AtomicUsize::new(0);
    let model_config = &model_config;

    // Each worker loads its own model instance and processes its batch group
    let results: Vec<Option<(Vec<f32>, Vec<ChunkMeta>)>> = std::thread::scope(|s| {
        let handles: Vec<_> = groups
            .iter()
            .enumerate()
            .map(|(worker_id, group)| {
                let progress = &progress;
                s.spawn(move || {
                    let (model, tokenizer, device) = match load_model(&model_config) {
                        Ok(m) => m,
                        Err(e) => {
                            eprintln!("  [semantic] Worker {worker_id} failed to load model: {e}");
                            return None;
                        }
                    };

                    let mut embs: Vec<f32> = Vec::new();
                    let mut metas: Vec<ChunkMeta> = Vec::new();

                    for batch in group {
                        let texts: Vec<&str> = batch.iter().map(|c| c.text.as_str()).collect();
                        match encode_batch(&model, &tokenizer, &device, &texts, model_config.dim) {
                            Ok(embeddings) => {
                                for (i, emb) in embeddings.into_iter().enumerate() {
                                    embs.extend_from_slice(&emb);
                                    let chunk = &batch[i];
                                    let snippet = if chunk.text.len() > 200 {
                                        let mut end = 200;
                                        while !chunk.text.is_char_boundary(end) && end > 0 {
                                            end -= 1;
                                        }
                                        chunk.text[..end].to_string()
                                    } else {
                                        chunk.text.clone()
                                    };
                                    metas.push(ChunkMeta {
                                        file_path: chunk.file_path.clone(),
                                        start_line: chunk.start_line,
                                        snippet,
                                    });
                                }
                            }
                            Err(e) => {
                                eprintln!("  [semantic] Worker {worker_id} batch failed: {e}");
                                continue;
                            }
                        }

                        let done = progress.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        if done % 20 == 0 || done == total_batches {
                            eprintln!("  [semantic] Progress: {done}/{total_batches} batches");
                        }
                    }

                    Some((embs, metas))
                })
            })
            .collect();

        handles.into_iter().map(|h| h.join().unwrap_or(None)).collect()
    });

    // Merge worker results
    let mut all_embeddings: Vec<f32> = Vec::with_capacity(chunks.len() * model_config.dim);
    let mut chunk_meta: Vec<ChunkMeta> = Vec::with_capacity(chunks.len());
    for result in results.into_iter().flatten() {
        all_embeddings.extend(result.0);
        chunk_meta.extend(result.1);
    }

    if chunk_meta.is_empty() {
        eprintln!("  [semantic] No embeddings produced");
        return None;
    }

    eprintln!(
        "  [semantic] Index built: {} chunks, {} floats",
        chunk_meta.len(),
        all_embeddings.len()
    );

    let stored_model_name = model_name.unwrap_or("minilm").to_string();
    Some(SemanticIndex {
        embeddings: all_embeddings,
        chunk_meta,
        dim: model_config.dim,
        model_name: stored_model_name,
    })
}

/// Get number of available CPU cores (capped at 8 to avoid memory explosion).
fn num_cpus() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8)
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

/// Search the semantic index for chunks similar to the query.
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

    // Sort by score descending
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scores.truncate(limit);

    let results = scores
        .into_iter()
        .map(|(idx, score)| {
            let meta = &index.chunk_meta[idx];
            SemanticSearchResult {
                file_path: meta.file_path.clone(),
                start_line: meta.start_line,
                snippet: meta.snippet.clone(),
                score,
            }
        })
        .collect();

    Ok(results)
}
