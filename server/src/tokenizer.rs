//! Pluggable token counting for budget allocation.
//!
//! Provides a `Tokenizer` trait with two implementations: `BytesEstimateTokenizer`
//! (fast bytes/3 heuristic, no dependencies) and `TiktokenTokenizer` (accurate BPE
//! counting, feature-gated behind `tiktoken`).

use std::sync::Arc;

pub trait Tokenizer: Send + Sync {
    fn count_tokens(&self, text: &str) -> usize;
    fn name(&self) -> &str;
}

/// Default: bytes/3 estimation (fast, no dependencies)
pub struct BytesEstimateTokenizer;

impl Tokenizer for BytesEstimateTokenizer {
    fn count_tokens(&self, text: &str) -> usize {
        text.len().div_ceil(3)
    }
    fn name(&self) -> &str {
        "bytes-estimate"
    }
}

/// Tiktoken-based tokenizer for Claude/GPT (requires `tiktoken` feature)
#[cfg(feature = "tiktoken")]
pub struct TiktokenTokenizer {
    bpe: tiktoken_rs::CoreBPE,
}

#[cfg(feature = "tiktoken")]
impl TiktokenTokenizer {
    pub fn new() -> Self {
        Self { bpe: tiktoken_rs::cl100k_base().unwrap() }
    }
}

#[cfg(feature = "tiktoken")]
impl Tokenizer for TiktokenTokenizer {
    fn count_tokens(&self, text: &str) -> usize {
        self.bpe.encode_with_special_tokens(text).len()
    }
    fn name(&self) -> &str {
        "tiktoken"
    }
}

/// Create a tokenizer by name. Falls back to bytes-estimate for unknown names.
pub fn create_tokenizer(name: &str) -> Arc<dyn Tokenizer> {
    match name {
        #[cfg(feature = "tiktoken")]
        "tiktoken" => Arc::new(TiktokenTokenizer::new()),
        _ => Arc::new(BytesEstimateTokenizer),
    }
}
