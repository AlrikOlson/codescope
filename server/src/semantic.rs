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

/// Maximum character length per chunk (~512 tokens).
const MAX_CHUNK_CHARS: usize = 1500;

/// Model identifier on HuggingFace Hub.
const MODEL_ID: &str = "sentence-transformers/all-MiniLM-L6-v2";

/// Embedding dimensionality for all-MiniLM-L6-v2.
const EMBEDDING_DIM: usize = 384;

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
fn extract_chunks(files: &[ScannedFile]) -> Vec<Chunk> {
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
                if current_chunk.len() + line.len() + 1 > MAX_CHUNK_CHARS && !current_chunk.is_empty() {
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
fn load_model() -> Result<(BertModel, Tokenizer), String> {
    let api = Api::new().map_err(|e| format!("Failed to create HF API: {e}"))?;

    // Set cache dir to ~/.cache/codescope/models/ if possible
    let repo = api.repo(Repo::with_revision(
        MODEL_ID.to_string(),
        RepoType::Model,
        "main".to_string(),
    ));

    eprintln!("  [semantic] Downloading/loading model {MODEL_ID}...");

    let config_path = repo
        .get("config.json")
        .map_err(|e| format!("Failed to get config.json: {e}"))?;
    let tokenizer_path = repo
        .get("tokenizer.json")
        .map_err(|e| format!("Failed to get tokenizer.json: {e}"))?;
    let weights_path = repo
        .get("model.safetensors")
        .map_err(|e| format!("Failed to get model.safetensors: {e}"))?;

    let config_str =
        std::fs::read_to_string(&config_path).map_err(|e| format!("Failed to read config: {e}"))?;
    let config: BertConfig =
        serde_json::from_str(&config_str).map_err(|e| format!("Failed to parse config: {e}"))?;

    let tokenizer = Tokenizer::from_file(&tokenizer_path)
        .map_err(|e| format!("Failed to load tokenizer: {e}"))?;

    let device = Device::Cpu;
    let vb = unsafe {
        VarBuilder::from_mmaped_safetensors(&[weights_path], DType::F32, &device)
            .map_err(|e| format!("Failed to load weights: {e}"))?
    };

    let model =
        BertModel::load(vb, &config).map_err(|e| format!("Failed to load BERT model: {e}"))?;

    eprintln!("  [semantic] Model loaded successfully");
    Ok((model, tokenizer))
}

// ---------------------------------------------------------------------------
// Embedding generation
// ---------------------------------------------------------------------------

/// Encode a batch of texts into embeddings using mean pooling.
fn encode_batch(
    model: &BertModel,
    tokenizer: &Tokenizer,
    texts: &[&str],
) -> Result<Vec<Vec<f32>>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let device = Device::Cpu;

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
    let input_ids =
        Tensor::from_vec(all_ids, (batch_size, max_len), &device)
            .map_err(|e| format!("Tensor creation failed: {e}"))?;
    let attention_mask =
        Tensor::from_vec(all_mask.iter().map(|&x| x as f32).collect::<Vec<_>>(), (batch_size, max_len), &device)
            .map_err(|e| format!("Tensor creation failed: {e}"))?;
    let token_type_ids =
        Tensor::from_vec(all_type_ids, (batch_size, max_len), &device)
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

    let masked = output
        .mul(&mask_expanded)
        .map_err(|e| format!("mul failed: {e}"))?;

    let summed = masked
        .sum(1)
        .map_err(|e| format!("sum failed: {e}"))?;

    let mask_sum = mask_expanded
        .sum(1)
        .map_err(|e| format!("mask sum failed: {e}"))?
        .clamp(1e-9, f64::MAX)
        .map_err(|e| format!("clamp failed: {e}"))?;

    let mean_pooled = summed
        .div(&mask_sum)
        .map_err(|e| format!("div failed: {e}"))?;

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

    let normalized = mean_pooled
        .div(&norms)
        .map_err(|e| format!("div failed: {e}"))?;

    // Extract to Vec<Vec<f32>>
    let flat: Vec<f32> = normalized
        .flatten_all()
        .map_err(|e| format!("flatten failed: {e}"))?
        .to_vec1()
        .map_err(|e| format!("to_vec1 failed: {e}"))?;

    let mut result = Vec::with_capacity(batch_size);
    for i in 0..batch_size {
        let start = i * EMBEDDING_DIM;
        let end = start + EMBEDDING_DIM;
        result.push(flat[start..end].to_vec());
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Index building
// ---------------------------------------------------------------------------

/// Build a semantic index from scanned files.
/// Returns `None` if model loading fails or no chunks found.
pub fn build_semantic_index(files: &[ScannedFile]) -> Option<SemanticIndex> {
    let chunks = extract_chunks(files);
    if chunks.is_empty() {
        eprintln!("  [semantic] No chunks extracted, skipping semantic index");
        return None;
    }

    eprintln!("  [semantic] Extracted {} chunks from {} files", chunks.len(), files.len());

    let (model, tokenizer) = match load_model() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("  [semantic] Failed to load model: {e}");
            return None;
        }
    };

    // Embed in batches of 32
    let batch_size = 32;
    let mut all_embeddings: Vec<f32> = Vec::with_capacity(chunks.len() * EMBEDDING_DIM);
    let mut chunk_meta: Vec<ChunkMeta> = Vec::with_capacity(chunks.len());

    let total_batches = (chunks.len() + batch_size - 1) / batch_size;

    for (batch_idx, batch) in chunks.chunks(batch_size).enumerate() {
        let texts: Vec<&str> = batch.iter().map(|c| c.text.as_str()).collect();

        match encode_batch(&model, &tokenizer, &texts) {
            Ok(embeddings) => {
                for (i, emb) in embeddings.into_iter().enumerate() {
                    all_embeddings.extend_from_slice(&emb);
                    let chunk = &batch[i];
                    let snippet = if chunk.text.len() > 200 {
                        chunk.text[..200].to_string()
                    } else {
                        chunk.text.clone()
                    };
                    chunk_meta.push(ChunkMeta {
                        file_path: chunk.file_path.clone(),
                        start_line: chunk.start_line,
                        snippet,
                    });
                }
            }
            Err(e) => {
                eprintln!("  [semantic] Batch {}/{} failed: {e}", batch_idx + 1, total_batches);
                // Skip failed batch, continue with others
                continue;
            }
        }

        if (batch_idx + 1) % 10 == 0 || batch_idx + 1 == total_batches {
            eprintln!(
                "  [semantic] Embedded {}/{} batches ({} chunks)",
                batch_idx + 1,
                total_batches,
                chunk_meta.len()
            );
        }
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

    Some(SemanticIndex {
        embeddings: all_embeddings,
        chunk_meta,
        dim: EMBEDDING_DIM,
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

/// Search the semantic index for chunks similar to the query.
pub fn semantic_search(
    index: &SemanticIndex,
    query: &str,
    limit: usize,
) -> Result<Vec<SemanticSearchResult>, String> {
    let (model, tokenizer) = load_model()?;

    let query_embeddings = encode_batch(&model, &tokenizer, &[query])?;
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
        let dot: f32 = query_emb
            .iter()
            .zip(chunk_emb.iter())
            .map(|(a, b)| a * b)
            .sum();
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
