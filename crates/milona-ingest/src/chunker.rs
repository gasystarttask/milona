//! `Chunker` implementation using the `text-splitter` crate's recursive
//! semantic splitter (paragraph/sentence/word/character fallback levels).
//!
//! ROADMAP.md suggests token-aware sizing via `tiktoken-rs`. That feature
//! pulls a BPE vocabulary file and, depending on the `tiktoken-rs` version,
//! may fetch it from the network at build/first-run time — undesirable in
//! this sandbox. Instead this chunker sizes chunks by character count (the
//! `text-splitter` default `Characters` sizer), and approximates a token
//! count on the resulting `Chunk` with the common `chars / 4` heuristic
//! (documented on `approx_token_count`) purely for the `Chunk::token_count`
//! field milona-core already has — chunk *boundaries* are decided by
//! character count, not by this approximation.

use milona_core::document::{Chunk, RawDocument};
use milona_core::error::CoreError;
use milona_core::traits::Chunker;
use text_splitter::{ChunkConfig, TextSplitter};

use crate::error::IngestError;

/// Default target chunk size in characters. ROADMAP.md's ~400-512 *token*
/// guidance maps to roughly 1600-2000 characters under the `chars/4`
/// approximation used here.
pub const DEFAULT_CHUNK_CHARACTERS: usize = 1800;

/// Default overlap in characters, ~15% of the default chunk size, within
/// ROADMAP.md's suggested 10-20% overlap range.
pub const DEFAULT_CHUNK_OVERLAP: usize = 270;

/// Recursive, character-size-aware chunker backed by `text-splitter`.
pub struct RecursiveChunker {
    splitter: TextSplitter<text_splitter::Characters>,
    max_characters: usize,
}

impl RecursiveChunker {
    /// Build a chunker targeting `max_characters` per chunk with
    /// `overlap_characters` of overlap between consecutive chunks.
    pub fn new(max_characters: usize, overlap_characters: usize) -> Result<Self, CoreError> {
        let config = ChunkConfig::new(max_characters)
            .with_overlap(overlap_characters)
            .map_err(|e| IngestError::Chunking(e.to_string()))?;
        Ok(Self {
            splitter: TextSplitter::new(config),
            max_characters,
        })
    }

    /// Chunker configured with Phase 1's documented defaults.
    pub fn with_defaults() -> Self {
        Self::new(DEFAULT_CHUNK_CHARACTERS, DEFAULT_CHUNK_OVERLAP)
            .expect("documented defaults always yield a valid ChunkConfig")
    }
}

impl Default for RecursiveChunker {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// `chars / 4` heuristic token-count approximation — see module docs.
fn approx_token_count(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

impl Chunker for RecursiveChunker {
    fn chunk(&self, document: &RawDocument) -> Result<Vec<Chunk>, CoreError> {
        if document.text.trim().is_empty() {
            return Ok(Vec::new());
        }

        let chunks: Vec<Chunk> = self
            .splitter
            .chunks(&document.text)
            .enumerate()
            .map(|(sequence, text)| {
                let text = text.to_string();
                let token_count = approx_token_count(&text);
                Chunk {
                    id: Default::default(),
                    document_id: document.id,
                    text,
                    sequence,
                    token_count,
                }
            })
            .collect();

        for c in &chunks {
            debug_assert!(
                c.text.chars().count() <= self.max_characters * 2,
                "chunk grossly exceeds configured max_characters; a single \
                 unsplittable token in the source text may be to blame"
            );
        }

        Ok(chunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use milona_core::document::SourceKind;
    use std::collections::HashMap;

    fn doc(text: &str) -> RawDocument {
        RawDocument {
            id: Default::default(),
            text: text.to_string(),
            source_kind: SourceKind::Text,
            origin: "test".to_string(),
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn empty_document_yields_no_chunks() {
        let chunker = RecursiveChunker::with_defaults();
        let chunks = chunker.chunk(&doc("   \n\t  ")).unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn short_document_yields_single_chunk() {
        let chunker = RecursiveChunker::with_defaults();
        let text = "This is a short document that easily fits in one chunk.";
        let chunks = chunker.chunk(&doc(text)).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].sequence, 0);
        assert_eq!(chunks[0].text, text);
        assert!(chunks[0].token_count > 0);
    }

    #[test]
    fn long_document_splits_into_multiple_chunks_within_size_bound() {
        let chunker = RecursiveChunker::new(200, 20).unwrap();
        // Build a long document out of many distinct sentences so we can
        // check ordering/overlap without relying on repeated identical text.
        let paragraph: String = (0..80)
            .map(|i| format!("Sentence number {i} adds more unique content to the corpus. "))
            .collect();

        let chunks = chunker.chunk(&doc(&paragraph)).unwrap();

        assert!(
            chunks.len() > 1,
            "expected multiple chunks, got {}",
            chunks.len()
        );

        // Chunks are sequential starting at 0.
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.sequence, i);
            assert!(
                c.text.chars().count() <= 200 + 20 + 50,
                "chunk {i} length {} exceeds expected bound",
                c.text.chars().count()
            );
            assert!(!c.text.is_empty());
        }

        // All chunks reference the same document id.
        let doc_ref = doc(&paragraph);
        for c in &chunks {
            assert_eq!(c.document_id, chunks[0].document_id);
            let _ = &doc_ref;
        }
    }

    #[test]
    fn chunk_count_is_stable_for_fixed_input() {
        let chunker = RecursiveChunker::new(100, 10).unwrap();
        let text = "Alpha beta gamma delta. ".repeat(50);
        let first = chunker.chunk(&doc(&text)).unwrap();
        let second = chunker.chunk(&doc(&text)).unwrap();
        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(a.text, b.text);
        }
    }

    #[test]
    fn approx_token_count_scales_with_length() {
        assert_eq!(approx_token_count(""), 0);
        assert_eq!(approx_token_count("abcd"), 1);
        assert_eq!(approx_token_count("abcdefgh"), 2);
    }
}
