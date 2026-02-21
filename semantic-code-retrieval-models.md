# Semantic Code Retrieval Models for Rust — Hardware-Tiered Guide

> **Goal:** Natural language → code search (e.g., "find the function that parses JSON config files")  
> **Runtime:** Rust, via ONNX Runtime (`ort` crate) or `fastembed-rs`  
> **Last updated:** Feb 2026

---

## TL;DR — The Pick List

| Tier | Hardware | Model | Params | Embed Dim | ONNX Size (approx) | Code-Specific? |
|------|----------|-------|--------|-----------|---------------------|----------------|
| 0 | Raspberry Pi / old laptop (<4GB) | **all-MiniLM-L6-v2** (quantized) | 22M | 384 | ~23 MB | ❌ General |
| 1 | Laptop CPU (8GB) | **nomic-embed-text-v1.5** | 137M | 768 (MRL→256) | ~260 MB | ⚠️ Decent |
| 1+ | Laptop CPU (8GB), code-focused | **CodeSage-Small-v2** | 130M | 1024 (MRL) | ~500 MB | ✅ Yes |
| 2 | M2 MacBook / workstation CPU (16GB) | **CodeSage-Base-v2** | 356M | 1024 (MRL) | ~1.3 GB | ✅ Yes |
| 2+ | M2 MacBook / workstation, general | **EmbeddingGemma-300M** | 300M | 768 (MRL→128) | ~200 MB (q8) | ⚠️ Decent |
| 3 | M2 Max (32GB+) or RTX 4090 | **Qodo-Embed-1-1.5B** | 1.5B | — | ~3 GB fp16 | ✅ Yes |
| 3+ | RTX 4090 (24GB VRAM) | **CodeSage-Large-v2** | 1.3B | 2048 (MRL) | ~5 GB | ✅ Yes |
| 4 | Multi-GPU / 48GB+ VRAM | **Qodo-Embed-1-7B** | 7B | — | ~14 GB fp16 | ✅ Yes |
| 4+ | Multi-GPU, fully open | **Nomic-Embed-Code** | 7B | — | ~14 GB fp16 | ✅ Yes |

---

## Rust Integration Stack

### Primary: `fastembed-rs` (v5+)
The easiest path. Pre-packaged ONNX models with auto-download, tokenization, and quantized variants built in.

```toml
[dependencies]
fastembed = "5"
```

```rust
use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};

let model = TextEmbedding::try_new(
    InitOptions::new(EmbeddingModel::AllMiniLML6V2)
        .with_show_download_progress(true),
)?;

let embeddings = model.embed(vec![
    "search_query: function that parses JSON config",
    "search_document: fn parse_config(path: &Path) -> Result<Config> { ... }",
], None)?;
```

**Models already in fastembed-rs:**
- `AllMiniLML6V2` / `AllMiniLML6V2Q` (quantized)
- `BGESmallENV15` / `BGESmallENV15Q`
- `BGEBaseENV15` / `BGEBaseENV15Q`
- `BGELargeENV15` / `BGELargeENV15Q`
- `NomicEmbedTextV1` / `NomicEmbedTextV15`
- `NomicEmbedTextV2Moe` (via `nomic-v2-moe` feature, uses candle)
- `MLE5Large`
- And more — check `TextEmbedding::list_supported_models()`

### Secondary: `ort` crate directly (v2+)
For models not in fastembed-rs (CodeSage, Qodo-Embed, etc.), use `ort` with ONNX files.

```toml
[dependencies]
ort = { version = "2", features = ["cuda"] }  # or "coreml" for macOS
tokenizers = "0.20"
ndarray = "0.16"
```

```rust
use ort::Session;

let session = Session::builder()?
    .with_optimization_level(ort::GraphOptimizationLevel::Level3)?
    .with_intra_threads(4)?
    .commit_from_file("codesage-small-v2.onnx")?;

// Tokenize with HuggingFace tokenizers crate, then run inference
let outputs = session.run(ort::inputs!["input_ids" => input_ids, "attention_mask" => mask]?)?;
```

### Execution Providers by Hardware
| Hardware | `ort` Feature Flag | Notes |
|----------|-------------------|-------|
| CPU (any) | default | Always works, AVX2/AVX-512 auto-detected |
| NVIDIA GPU | `cuda` or `tensorrt` | Requires CUDA toolkit |
| Apple Silicon | `coreml` | M1/M2/M3, uses Neural Engine |
| AMD GPU | `rocm` | Linux only |
| Intel GPU | `openvino` | Integrated + Arc GPUs |
| Windows GPU | `directml` | Universal Windows GPU backend |

---

## Model Deep Dives

### Tier 0 — all-MiniLM-L6-v2 (~23 MB quantized)
- **Why:** Runs anywhere. 22M params, 384-dim output. Sub-millisecond on modern CPUs.
- **Code quality:** Adequate for keyword-heavy queries ("parse JSON", "sort array"). Struggles with semantic intent ("ensure reliability of operations").
- **License:** Apache 2.0
- **Quantized:** Yes, INT8 variant in fastembed-rs (`AllMiniLML6V2Q`)
- **Context window:** ~256 tokens

### Tier 1 — nomic-embed-text-v1.5 (137M, ~260 MB)
- **Why:** Best general-purpose small model. Matryoshka dims (768→512→256→128) let you trade accuracy for speed. 8192-token context window.
- **Code quality:** Good when using task prefixes: `search_query: ...` and `search_document: ...`
- **License:** Apache 2.0
- **ONNX:** Pre-exported on HuggingFace, already in fastembed-rs
- **Tip:** Use 512d for a good speed/quality balance

### Tier 1+ — CodeSage-Small-v2 (130M, ~500 MB)
- **Why:** Purpose-built for code. Trained on The Stack V2 with MLM + deobfuscation + contrastive learning. 9 languages: C, C#, Go, Java, JS, TS, PHP, Python, Ruby.
- **Code quality:** Substantially better than general models at NL→code. Consistency filtering removes noisy training pairs.
- **License:** Apache 2.0
- **ONNX:** Need to export yourself (one-time Python step)
- **Context window:** 1024 tokens
- **MRL:** Yes, flexible embedding dims

### Tier 2 — CodeSage-Base-v2 (356M, ~1.3 GB)
- **Why:** The sweet spot. Meaningful jump in NL→code quality over Small while staying CPU-friendly on a modern laptop.
- **Code quality:** Strong across code2code and NL2code benchmarks. Outperforms OpenAI text-embedding-3-large on code tasks.
- **License:** Apache 2.0
- **ONNX:** Export yourself
- **Languages:** Same 9 as Small

### Tier 2+ — EmbeddingGemma-300M (300M, ~200 MB quantized)
- **Why:** Google DeepMind model, extremely compact with quantization. 100+ language support. MRL (768→128 dims). Can run in <200MB RAM on EdgeTPU.
- **Code quality:** Decent — general-purpose but trained on diverse data including code
- **License:** Apache 2.0
- **Note:** Some community reports suggest it outperforms Qwen3-0.6B on retrieval benchmarks

### Tier 3 — Qodo-Embed-1-1.5B (1.5B)
- **Why:** The efficiency king. CoIR benchmark score of 68.53 — this **beats most 7B models**. Trained with progressive hard negative mining.
- **Code quality:** State-of-the-art for its size. Understands semantic intent, not just keyword matching. Correctly distinguishes "analyzing failures" from "handling failures."
- **License:** Check Qodo's terms (open weights)
- **ONNX:** Would need export; may be better served via candle or torch2ort
- **Fits:** RTX 4090 in fp16, M2 Max with 32GB+

### Tier 3+ — CodeSage-Large-v2 (1.3B, ~5 GB)
- **Why:** Proven architecture, Apache 2.0, 2048-dim embeddings capture fine-grained semantics.
- **Code quality:** Outperforms OpenAI ada-002, text-embedding-3-small, and matches text-embedding-3-large on code tasks.
- **License:** Apache 2.0
- **ONNX:** Export yourself

### Tier 4 — Qodo-Embed-1-7B / Nomic-Embed-Code 7B
- **Qodo-Embed-1-7B:** CoIR 71.5. Current top performer. 
- **Nomic-Embed-Code:** Fully open-source (weights, training data, eval code). Apache 2.0. Trained on CoRNStack.
- **Code quality:** Best available. Understands complex multi-step queries.
- **Practical:** Needs quantization (GPTQ/AWQ) for a single 4090, or split across GPUs.

---

## ONNX Export Cheat Sheet

For models not pre-exported, here's the one-time Python setup:

```bash
pip install optimum[onnxruntime] sentence-transformers
```

```python
# For encoder models (CodeSage, BGE, etc.)
from optimum.onnxruntime import ORTModelForFeatureExtraction
from transformers import AutoTokenizer

model_id = "codesage/codesage-small-v2"
model = ORTModelForFeatureExtraction.from_pretrained(model_id, export=True)
tokenizer = AutoTokenizer.from_pretrained(model_id)

# Save ONNX
model.save_pretrained("codesage-small-v2-onnx")
tokenizer.save_pretrained("codesage-small-v2-onnx")
```

```python
# For quantization (INT8, ~2x smaller, minimal quality loss)
from optimum.onnxruntime import ORTQuantizer
from optimum.onnxruntime.configuration import AutoQuantizationConfig

quantizer = ORTQuantizer.from_pretrained("codesage-small-v2-onnx")
qconfig = AutoQuantizationConfig.avx512_vnni(is_static=False)  # or arm64
quantizer.quantize(save_dir="codesage-small-v2-onnx-q8", quantization_config=qconfig)
```

---

## Architecture Considerations

### Encoder-only (recommended for most tiers)
Models like CodeSage, BGE, MiniLM, and nomic-embed-text use **encoder-only** transformers (BERT-like). These are ideal for ONNX:
- Single forward pass → embedding
- Easy batching
- Well-supported by `ort`

### Decoder-based (Tiers 3-4)
Models like Qwen3-Embedding, Nomic-Embed-Code, and CodeXEmbed-2B/7B use **decoder** architectures with last-token pooling. These work but:
- Require KV-cache management for efficiency
- Higher memory per parameter
- More complex ONNX graphs
- Consider using `candle` or `llama.cpp` bindings instead of ONNX for these

### Query Prefixes
Most models require specific prefixes for optimal performance:

| Model | Query Prefix | Document Prefix |
|-------|-------------|-----------------|
| nomic-embed-text | `search_query: ` | `search_document: ` |
| CodeSage | (none, but add EOS token) | (none, but add EOS token) |
| BGE | `Represent this sentence: ` | (none) |
| Qwen3-Embedding | `Instruct: ...\nQuery:` | (none) |
| Qodo-Embed | `Represent this query for searching relevant code: ` | (none) |

---

## Recommended Setup for Auto-Selection

```rust
enum ModelTier {
    UltraLight,  // all-MiniLM-L6-v2 Q
    Light,       // nomic-embed-text-v1.5 or CodeSage-Small-v2
    Medium,      // CodeSage-Base-v2
    High,        // Qodo-Embed-1-1.5B or CodeSage-Large-v2
    Extreme,     // Qodo-Embed-1-7B
}

fn select_tier(available_ram_gb: f64, has_gpu: bool, gpu_vram_gb: Option<f64>) -> ModelTier {
    match (available_ram_gb, has_gpu, gpu_vram_gb) {
        (ram, _, _) if ram < 4.0 => ModelTier::UltraLight,
        (ram, false, _) if ram < 12.0 => ModelTier::Light,
        (ram, false, _) if ram < 24.0 => ModelTier::Medium,
        (_, true, Some(vram)) if vram >= 16.0 => ModelTier::High,
        (_, true, Some(vram)) if vram >= 40.0 => ModelTier::Extreme,
        _ => ModelTier::Medium,
    }
}
```

---

## Key Benchmarks Reference

**CoIR (Code Information Retrieval) — NDCG@10 average across 10 datasets:**

| Model | Size | CoIR Avg | Notes |
|-------|------|----------|-------|
| Qodo-Embed-1-7B | 7B | 71.5 | Current SOTA |
| CodeXEmbed-7B (SFR) | 7B | ~70+ | #1 on CoIR leaderboard |
| Qodo-Embed-1-1.5B | 1.5B | 68.53 | Beats most 7B models! |
| CodeXEmbed-2B (SFR) | 2B | ~65 | Good value |
| CodeXEmbed-400M (SFR) | 400M | ~58 | Research license ⚠️ |
| CodeSage-Large-v2 | 1.3B | ~55-60 | Apache 2.0 |
| Voyage-Code-002 | ? | ~50 | Proprietary API |
| E5-Mistral | 7B | ~45 | Best general on CoIR |
| CodeSage-Small-v2 | 130M | ~40-45 | Great for size |

*Note: Exact numbers vary by sub-task. The overall ranking is consistent.*

---

## License Summary

| Model | License | Commercial Use? |
|-------|---------|----------------|
| all-MiniLM-L6-v2 | Apache 2.0 | ✅ |
| nomic-embed-text-v1.5 | Apache 2.0 | ✅ |
| CodeSage (all sizes) v2 | Apache 2.0 | ✅ |
| EmbeddingGemma-300M | Apache 2.0 | ✅ |
| Nomic-Embed-Code | Apache 2.0 | ✅ |
| Qwen3-Embedding (all) | Apache 2.0 | ✅ |
| CodeXEmbed (SFR) | Research only | ❌ |
| Qodo-Embed-1 | Check terms | ⚠️ Verify |

---

## My Top 3 Recommendations

1. **Default / fallback:** `nomic-embed-text-v1.5` — runs everywhere, ONNX ready, in fastembed-rs, Apache 2.0, 8K context
2. **Code-focused, resource-constrained:** `CodeSage-Small-v2` — purpose-built for code, 130M params, Apache 2.0
3. **When you have a GPU:** `Qodo-Embed-1-1.5B` — absurd quality/size ratio, beats 7B models on code retrieval
