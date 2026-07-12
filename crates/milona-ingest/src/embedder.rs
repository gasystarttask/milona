//! `Embedder` implementation for Phase 1.
//!
//! ROADMAP.md recommends `fastembed-rs` (local ONNX inference via `ort`).
//! **That was tried and does not build in this sandbox**: `fastembed` v5
//! depends on `ort` 2.0.0-rc.12, which requires rustc >= 1.88, while this
//! workspace pins `rust-version = 1.87` (`cargo build` fails with
//! `ort-sys@2.0.0-rc.12 requires rustc 1.88`). This is the same MSRV
//! conflict already documented for `genai`/`darling` in `milona-adapter`.
//! `fastembed` also downloads an ONNX model + tokenizer from Hugging Face
//! on first use, which is a separate concern (network egress at runtime)
//! independent of the MSRV blocker.
//!
//! Pending either a workspace rustc bump or a lighter pure-Rust ONNX-free
//! embedding option, this module ships [`MockEmbedder`]: a clearly-labeled
//! placeholder that hashes input text into a deterministic, fixed-dimension
//! pseudo-embedding. It satisfies the `Embedder` trait shape (stable
//! dimensionality, deterministic output) so the rest of the pipeline
//! (chunking → embedding → storage) can be built, tested, and swapped later
//! without touching call sites — but it has **no semantic meaning**: two
//! unrelated chunks of text are exactly as "similar" under this embedder as
//! two paraphrases of the same sentence. Do not use it for anything but
//! wiring/tests.

use async_trait::async_trait;
use milona_core::error::CoreError;
use milona_core::traits::Embedder;
use std::hash::Hasher;

/// Placeholder embedder producing a deterministic hash-based pseudo-vector.
///
/// # How it works
/// The input text is tokenized on whitespace. Each token is hashed with
/// `std::hash::Hash` (via a FNV-style `DefaultHasher`) combined with each
/// output dimension's index, producing a value that's folded into
/// `[-1.0, 1.0]` and accumulated per-dimension. The resulting vector is then
/// L2-normalized. This is deterministic (same text always yields the same
/// vector), fast, and dependency-free, but it is **not** a learned
/// embedding: it captures no semantics, only a text-derived pseudo-random
/// fingerprint. Replace with a real embedding model before relying on
/// vector search quality.
#[derive(Debug, Clone, Copy)]
pub struct MockEmbedder {
    dimensions: usize,
}

impl MockEmbedder {
    /// Common small-model dimensionality (e.g. `all-MiniLM-L6-v2`), used as
    /// the default so a future swap to a real embedder needs no dimension
    /// migration for the common case.
    pub const DEFAULT_DIMENSIONS: usize = 384;

    pub fn new(dimensions: usize) -> Self {
        assert!(dimensions > 0, "embedding dimensionality must be non-zero");
        Self { dimensions }
    }
}

impl Default for MockEmbedder {
    fn default() -> Self {
        Self::new(Self::DEFAULT_DIMENSIONS)
    }
}

fn hash_token_dim(token: &str, dim: usize) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    hasher.write(token.as_bytes());
    hasher.write_usize(dim);
    hasher.finish()
}

#[async_trait]
impl Embedder for MockEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, CoreError> {
        let mut vector = vec![0f32; self.dimensions];

        let tokens: Vec<&str> = text.split_whitespace().collect();
        if tokens.is_empty() {
            // Deterministic zero-ish vector for empty input, still the
            // declared dimensionality.
            return Ok(vector);
        }

        for token in &tokens {
            for (dim, slot) in vector.iter_mut().enumerate() {
                let h = hash_token_dim(token, dim);
                // Fold the 64-bit hash into [-1.0, 1.0].
                let signed = (h as i64 as f64) / (u64::MAX as f64 / 2.0);
                *slot += signed as f32;
            }
        }

        // L2-normalize so cosine-similarity-based vector stores behave
        // sensibly (unit-length vectors) even though the content is
        // meaningless.
        let norm: f32 = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > f32::EPSILON {
            for v in &mut vector {
                *v /= norm;
            }
        }

        Ok(vector)
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn embed_returns_declared_dimensionality() {
        let embedder = MockEmbedder::new(128);
        let v = embedder.embed("hello world").await.unwrap();
        assert_eq!(v.len(), 128);
        assert_eq!(embedder.dimensions(), 128);
    }

    #[tokio::test]
    async fn embed_is_deterministic() {
        let embedder = MockEmbedder::default();
        let a = embedder.embed("the quick brown fox").await.unwrap();
        let b = embedder.embed("the quick brown fox").await.unwrap();
        assert_eq!(a, b);
    }

    #[tokio::test]
    async fn different_text_yields_different_vector() {
        let embedder = MockEmbedder::default();
        let a = embedder.embed("alpha beta gamma").await.unwrap();
        let b = embedder
            .embed("completely unrelated content here")
            .await
            .unwrap();
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn embed_is_l2_normalized() {
        let embedder = MockEmbedder::default();
        let v = embedder.embed("normalize me please").await.unwrap();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "norm was {norm}");
    }

    #[tokio::test]
    async fn empty_text_yields_zero_vector_of_correct_dimension() {
        let embedder = MockEmbedder::new(16);
        let v = embedder.embed("").await.unwrap();
        assert_eq!(v.len(), 16);
        assert!(v.iter().all(|x| *x == 0.0));
    }

    #[tokio::test]
    async fn default_dimensions_match_documented_constant() {
        let embedder = MockEmbedder::default();
        assert_eq!(embedder.dimensions(), MockEmbedder::DEFAULT_DIMENSIONS);
    }
}
